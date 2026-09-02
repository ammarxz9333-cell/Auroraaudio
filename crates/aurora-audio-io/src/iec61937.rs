//! IEC 61937 burst extraction used by the Aurora eARC ingress path.
//!
//! The implementation follows the framing behavior independently validated by the
//! Vibesbox SiI9437 eARC tap: the live eARC capture is S32_LE with the 16-bit
//! IEC 61937 word in the upper half of each sample; E-AC-3 uses data type 0x15
//! and its Pd length code is expressed in bytes rather than bits.

const PA_LE: [u8; 2] = [0x72, 0xF8];
const PB_LE: [u8; 2] = [0x1F, 0x4E];

/// IEC 61937 data type for AC-3.
pub const DATA_TYPE_AC3: u8 = 0x01;
/// IEC 61937 data type for E-AC-3 / Dolby Digital Plus.
pub const DATA_TYPE_EAC3: u8 = 0x15;
/// IEC 61937 data type for Dolby MAT / TrueHD.
pub const DATA_TYPE_MAT: u8 = 0x16;

/// Codec filter applied to decoded IEC 61937 bursts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecFilter {
    Ac3,
    Eac3,
    Dts,
    All,
}

impl CodecFilter {
    pub fn accepts(self, data_type: u8) -> bool {
        match self {
            Self::Ac3 => data_type == DATA_TYPE_AC3,
            Self::Eac3 => data_type == DATA_TYPE_EAC3,
            Self::Dts => matches!(data_type, 0x0B..=0x0D),
            Self::All => true,
        }
    }
}

/// One complete IEC 61937 burst after wrapper removal and 16-bit word byte swapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Burst {
    pub data_type: u8,
    pub payload: Vec<u8>,
}

/// Stateful parser for the 16-bit little-endian IEC 61937 word stream.
#[derive(Debug)]
pub struct BurstParser {
    filter: CodecFilter,
    buffer: Vec<u8>,
}

impl BurstParser {
    pub fn new(filter: CodecFilter) -> Self {
        Self {
            filter,
            buffer: Vec::with_capacity(32 * 1024),
        }
    }

    /// Compatibility API used by the standalone extractor.
    pub fn push(&mut self, input: &[u8]) -> Vec<Burst> {
        let mut bursts = Vec::new();
        self.push_each(input, |data_type, payload| {
            bursts.push(Burst {
                data_type,
                payload: payload.to_vec(),
            });
        });
        bursts
    }

    /// Adds IEC words and invokes `emit` for each complete accepted burst.
    ///
    /// The payload slice is borrowed from the parser's reusable internal buffer
    /// and is valid only for the duration of the callback. This is the R2
    /// appliance hot path: it avoids allocating/copying one payload Vec per
    /// E-AC-3 burst before passing the bytes to liborender.
    pub fn push_each<F>(&mut self, input: &[u8], mut emit: F)
    where
        F: FnMut(u8, &[u8]),
    {
        self.buffer.extend_from_slice(input);

        loop {
            let Some(sync_offset) = find_sync(&self.buffer) else {
                retain_sync_straddle(&mut self.buffer);
                break;
            };
            if sync_offset > 0 {
                self.buffer.drain(..sync_offset);
            }
            if self.buffer.len() < 8 {
                break;
            }

            let pc = u16::from_le_bytes([self.buffer[4], self.buffer[5]]);
            let pd = u16::from_le_bytes([self.buffer[6], self.buffer[7]]);
            let data_type = (pc & 0x1F) as u8;
            let payload_bytes = payload_length_bytes(data_type, pd);

            if payload_bytes == 0 {
                self.buffer.drain(..8);
                continue;
            }
            let total = 8usize.saturating_add(payload_bytes);
            if self.buffer.len() < total {
                break;
            }

            if payload_bytes % 2 == 0 && self.filter.accepts(data_type) {
                for word in self.buffer[8..total].chunks_exact_mut(2) {
                    word.swap(0, 1);
                }
                emit(data_type, &self.buffer[8..total]);
            }

            self.buffer.drain(..total);
        }
    }
}

/// Converts IEC 61937 Pd to payload bytes.
pub fn payload_length_bytes(data_type: u8, pd: u16) -> usize {
    if matches!(data_type, DATA_TYPE_EAC3 | DATA_TYPE_MAT) {
        usize::from(pd)
    } else {
        usize::from(pd) / 8
    }
}

/// Streaming adaptor for byte-oriented S32_LE input.
#[derive(Debug, Default)]
pub struct S32HighWordAdapter {
    tail: Vec<u8>,
}

impl S32HighWordAdapter {
    pub fn push(&mut self, raw: &[u8]) -> Vec<u8> {
        self.tail.extend_from_slice(raw);
        let usable = self.tail.len() - (self.tail.len() % 4);
        let mut words = Vec::with_capacity(usable / 2);
        for sample in self.tail[..usable].chunks_exact(4) {
            words.extend_from_slice(&sample[2..4]);
        }
        self.tail.drain(..usable);
        words
    }
}

/// Extract the upper 16-bit IEC words from an aligned ALSA S32_LE capture into
/// a caller-owned reusable byte buffer. No allocation occurs after `out` has
/// reached the required capacity.
pub fn s32_samples_to_iec_words(samples: &[i32], out: &mut Vec<u8>) {
    out.clear();
    out.reserve(samples.len().saturating_mul(2).saturating_sub(out.capacity()));
    for sample in samples {
        let bytes = sample.to_le_bytes();
        out.extend_from_slice(&bytes[2..4]);
    }
}

fn find_sync(buffer: &[u8]) -> Option<usize> {
    buffer
        .windows(4)
        .position(|window| window[0..2] == PA_LE && window[2..4] == PB_LE)
}

fn retain_sync_straddle(buffer: &mut Vec<u8>) {
    const MAX_STRADDLE: usize = 3;
    if buffer.len() > MAX_STRADDLE {
        let keep_from = buffer.len() - MAX_STRADDLE;
        buffer.drain(..keep_from);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eac3_pd_is_bytes_and_payload_round_trips() {
        let native = vec![0x0B, 0x77, 0x12, 0x34, 0xAB, 0xCD, 0xEF, 0x01];
        let captured = make_burst(DATA_TYPE_EAC3, &native);
        let mut parser = BurstParser::new(CodecFilter::Eac3);
        let bursts = parser.push(&captured);
        assert_eq!(bursts.len(), 1);
        assert_eq!(bursts[0].data_type, DATA_TYPE_EAC3);
        assert_eq!(bursts[0].payload, native);
    }

    #[test]
    fn zero_copy_callback_emits_native_payload() {
        let native = vec![0x0B, 0x77, 0x12, 0x34, 0x55, 0x66];
        let captured = make_burst(DATA_TYPE_EAC3, &native);
        let mut parser = BurstParser::new(CodecFilter::Eac3);
        let mut seen = Vec::new();
        parser.push_each(&captured, |data_type, payload| {
            assert_eq!(data_type, DATA_TYPE_EAC3);
            seen.extend_from_slice(payload);
        });
        assert_eq!(seen, native);
    }

    #[test]
    fn parser_handles_one_byte_chunks_without_losing_phase() {
        let native = vec![0x0B, 0x77, 0x00, 0x02, 0x44, 0x55, 0x66, 0x77];
        let captured = make_burst(DATA_TYPE_EAC3, &native);
        let mut parser = BurstParser::new(CodecFilter::Eac3);
        let mut out = Vec::new();
        for byte in captured {
            out.extend(parser.push(&[byte]));
        }
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].payload, native);
    }

    #[test]
    fn s32_adapter_survives_unaligned_reads() {
        let words = [0x72, 0xF8, 0x1F, 0x4E, 0x15, 0x00, 0x08, 0x00];
        let mut raw = Vec::new();
        for word in words.chunks_exact(2) {
            raw.extend_from_slice(&[0xA5, 0x5A, word[0], word[1]]);
        }
        let mut adapter = S32HighWordAdapter::default();
        let mut recovered = Vec::new();
        for chunk in raw.chunks(3) {
            recovered.extend(adapter.push(chunk));
        }
        assert_eq!(recovered, words);
    }

    #[test]
    fn aligned_s32_helper_uses_high_word_only() {
        let samples = [0xF872_1234_u32 as i32, 0x4E1F_ABCD_u32 as i32];
        let mut words = Vec::new();
        s32_samples_to_iec_words(&samples, &mut words);
        assert_eq!(words, [0x72, 0xF8, 0x1F, 0x4E]);
    }

    #[test]
    fn ac3_pd_is_interpreted_as_bits() {
        assert_eq!(payload_length_bytes(DATA_TYPE_AC3, 20_480), 2_560);
    }

    #[test]
    fn codec_filter_does_not_leak_other_burst_types() {
        let native = vec![0x0B, 0x77, 0x12, 0x34];
        let captured = make_burst(DATA_TYPE_AC3, &native);
        let mut parser = BurstParser::new(CodecFilter::Eac3);
        assert!(parser.push(&captured).is_empty());
    }

    fn make_burst(data_type: u8, native_payload: &[u8]) -> Vec<u8> {
        assert_eq!(native_payload.len() % 2, 0);
        let pd = if matches!(data_type, DATA_TYPE_EAC3 | DATA_TYPE_MAT) {
            native_payload.len() as u16
        } else {
            (native_payload.len() * 8) as u16
        };
        let mut burst = Vec::new();
        burst.extend_from_slice(&PA_LE);
        burst.extend_from_slice(&PB_LE);
        burst.extend_from_slice(&u16::from(data_type).to_le_bytes());
        burst.extend_from_slice(&pd.to_le_bytes());
        for word in native_payload.chunks_exact(2) {
            burst.extend_from_slice(&[word[1], word[0]]);
        }
        burst
    }
}
