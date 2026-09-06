//! IEC 61937 digital audio burst demultiplexer and parser.
//!
//! Standard reference: IEC 61937-1 (General), IEC 61937-3 (AC-3, Enhanced AC-3),
//! and IEC 61937-14 (Dolby TrueHD, Dolby MAT).

use thiserror::Error;

/// Preamble Pa sync word (native 16-bit big-endian).
pub const PREAMBLE_PA: u16 = 0xF872;
/// Preamble Pb sync word (native 16-bit big-endian).
pub const PREAMBLE_PB: u16 = 0x4E1F;

/// Preamble Pa byte-swapped (little-endian byte order: 0x72, 0xF8).
pub const PREAMBLE_PA_SWAPPED: u16 = 0x72F8;
/// Preamble Pb byte-swapped (little-endian byte order: 0x1F, 0x4E).
pub const PREAMBLE_PB_SWAPPED: u16 = 0x1F4E;

/// Maximum payload length allowed for a single IEC 61937 burst.
pub const MAX_BURST_PAYLOAD_BYTES: usize = 65536;

/// Standard IEC 61937 data types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Iec61937DataType {
    /// Null / Silence burst (type 0).
    NullData,
    /// Standard AC-3 (Dolby Digital, type 1).
    Ac3,
    /// Pause data (type 3).
    Pause,
    /// MPEG-1 or MPEG-2 audio (types 4..=6).
    Mpeg,
    /// AAC audio (types 8, 9).
    Aac,
    /// DTS audio (types 11..=13).
    Dts,
    /// DTS-HD audio (type 16).
    DtsHd,
    /// Enhanced AC-3 (Dolby Digital Plus / Atmos JOC, type 21 / 0x15).
    EnhancedAc3,
    /// Dolby TrueHD lossless (type 22 / 0x16).
    DolbyTrueHd,
    /// Dolby MAT (Metadata-enhanced Audio Transmission, type 23 / 0x17).
    DolbyMat,
    /// Other or proprietary format.
    Other(u8),
}

impl From<u8> for Iec61937DataType {
    fn from(code: u8) -> Self {
        match code {
            0 => Self::NullData,
            1 => Self::Ac3,
            3 => Self::Pause,
            4 | 5 | 6 => Self::Mpeg,
            8 | 9 => Self::Aac,
            11 | 12 | 13 => Self::Dts,
            16 => Self::DtsHd,
            21 => Self::EnhancedAc3,
            22 => Self::DolbyTrueHd,
            23 => Self::DolbyMat,
            other => Self::Other(other),
        }
    }
}

impl Iec61937DataType {
    /// Returns the raw IEC 61937 data type code.
    pub fn raw_code(&self) -> u8 {
        match self {
            Self::NullData => 0,
            Self::Ac3 => 1,
            Self::Pause => 3,
            Self::Mpeg => 4,
            Self::Aac => 8,
            Self::Dts => 11,
            Self::DtsHd => 16,
            Self::EnhancedAc3 => 21,
            Self::DolbyTrueHd => 22,
            Self::DolbyMat => 23,
            Self::Other(code) => *code,
        }
    }

    /// Returns true if this data type carries immersive or spatial audio (E-AC-3 JOC, MAT, TrueHD).
    pub fn is_immersive_capable(&self) -> bool {
        matches!(self, Self::EnhancedAc3 | Self::DolbyMat | Self::DolbyTrueHd)
    }
}

/// A parsed IEC 61937 audio burst.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Iec61937Burst {
    /// Data type of the payload bitstream.
    pub data_type: Iec61937DataType,
    /// Substream number (bits 5..=6 of Pc).
    pub substream_num: u8,
    /// Error indicator from bit 7 of Pc.
    pub error_flag: bool,
    /// Data type dependent info (bits 8..=12 of Pc).
    pub data_type_info: u8,
    /// Raw payload length in bytes specified in Pd.
    pub payload_bytes_length: usize,
    /// Payload bytes unpadded and normalized to native byte order.
    pub payload: Vec<u8>,
    /// Whether the wire burst was byte-swapped (e.g. S/PDIF 16-bit LE).
    pub was_byte_swapped: bool,
}

/// Errors occurring during IEC 61937 burst extraction.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum Iec61937Error {
    /// Burst payload exceeds safety limit.
    #[error("burst payload length {0} exceeds limit {MAX_BURST_PAYLOAD_BYTES}")]
    PayloadTooLarge(usize),
    /// Invalid header or checksum.
    #[error("corrupted IEC 61937 header")]
    CorruptedHeader,
}

/// Streaming parser for continuous, potentially fragmented IEC 61937 streams.
#[derive(Debug, Default)]
pub struct Iec61937Parser {
    buffer: Vec<u8>,
}

impl Iec61937Parser {
    /// Creates a new IEC 61937 parser with an empty ingest buffer.
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(32768),
        }
    }

    /// Feeds raw incoming bytes into the parser buffer.
    pub fn push_bytes(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    /// Returns the number of unparsed bytes in the internal buffer.
    pub fn buffered_bytes(&self) -> usize {
        self.buffer.len()
    }

    /// Clears the internal stream buffer and state.
    pub fn reset(&mut self) {
        self.buffer.clear();
    }

    /// Attempts to extract the next complete IEC 61937 burst from the buffer.
    ///
    /// Returns `None` if more data is needed to complete the current burst.
    pub fn next_burst(&mut self) -> Option<Result<Iec61937Burst, Iec61937Error>> {
        loop {
            if self.buffer.len() < 8 {
                return None;
            }

            // Look for Pa and Pb preambles
            let mut sync_index = None;
            let mut is_swapped = false;

            for i in 0..=(self.buffer.len() - 8) {
                let pa_be = u16::from_be_bytes([self.buffer[i], self.buffer[i + 1]]);
                let pb_be = u16::from_be_bytes([self.buffer[i + 2], self.buffer[i + 3]]);

                if pa_be == PREAMBLE_PA && pb_be == PREAMBLE_PB {
                    sync_index = Some(i);
                    is_swapped = false;
                    break;
                }

                let pa_le = u16::from_be_bytes([self.buffer[i], self.buffer[i + 1]]);
                let pb_le = u16::from_be_bytes([self.buffer[i + 2], self.buffer[i + 3]]);
                if pa_le == PREAMBLE_PA_SWAPPED && pb_le == PREAMBLE_PB_SWAPPED {
                    sync_index = Some(i);
                    is_swapped = true;
                    break;
                }
            }

            let start = match sync_index {
                Some(idx) => idx,
                None => {
                    // No sync word found in searched region; keep last 7 bytes in case
                    // a syncword is split across chunk boundaries.
                    let keep = self.buffer.len().min(7);
                    let drain_len = self.buffer.len() - keep;
                    self.buffer.drain(0..drain_len);
                    return None;
                }
            };

            // Discard unaligned bytes preceding sync preamble
            if start > 0 {
                self.buffer.drain(0..start);
            }

            if self.buffer.len() < 8 {
                return None;
            }

            // Read Pc and Pd
            let (pc, pd) = if !is_swapped {
                let pc = u16::from_be_bytes([self.buffer[4], self.buffer[5]]);
                let pd = u16::from_be_bytes([self.buffer[6], self.buffer[7]]);
                (pc, pd)
            } else {
                let pc = u16::from_le_bytes([self.buffer[4], self.buffer[5]]);
                let pd = u16::from_le_bytes([self.buffer[6], self.buffer[7]]);
                (pc, pd)
            };

            let data_type_code = (pc & 0x001F) as u8;
            let substream_num = ((pc >> 5) & 0x03) as u8;
            let error_flag = (pc & 0x0080) != 0;
            let data_type_info = ((pc >> 8) & 0x1F) as u8;

            let data_type = Iec61937DataType::from(data_type_code);

            // In IEC 61937-3 (E-AC-3), Pd is the number of payload bytes.
            // In standard IEC 61937-1/3 (AC-3), Pd is the number of bits (so divide by 8).
            let payload_bytes = match data_type {
                Iec61937DataType::EnhancedAc3
                | Iec61937DataType::DolbyMat
                | Iec61937DataType::DolbyTrueHd => pd as usize,
                _ => {
                    // For standard AC-3 / PCM, Pd is usually in bits
                    if pd % 8 == 0 && (pd as usize / 8) <= self.buffer.len() {
                        (pd as usize) / 8
                    } else {
                        pd as usize
                    }
                }
            };

            if payload_bytes > MAX_BURST_PAYLOAD_BYTES {
                // Malformed packet length; skip past sync word
                self.buffer.drain(0..4);
                return Some(Err(Iec61937Error::PayloadTooLarge(payload_bytes)));
            }

            let total_burst_len = 8 + payload_bytes;
            if self.buffer.len() < total_burst_len {
                // More bytes needed to complete this burst
                return None;
            }

            // Extract payload
            let raw_payload = &self.buffer[8..total_burst_len];
            let mut payload = Vec::with_capacity(payload_bytes);

            if !is_swapped {
                payload.extend_from_slice(raw_payload);
            } else {
                // Unswap 16-bit words to native order
                for chunk in raw_payload.chunks(2) {
                    if chunk.len() == 2 {
                        payload.push(chunk[1]);
                        payload.push(chunk[0]);
                    } else {
                        payload.push(chunk[0]);
                    }
                }
            }

            // Drain processed burst from buffer
            self.buffer.drain(0..total_burst_len);

            return Some(Ok(Iec61937Burst {
                data_type,
                substream_num,
                error_flag,
                data_type_info,
                payload_bytes_length: payload_bytes,
                payload,
                was_byte_swapped: is_swapped,
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_native_eac3_burst() {
        let mut parser = Iec61937Parser::new();

        let mut packet = Vec::new();
        // Pa = 0xF872, Pb = 0x4E1F
        packet.extend_from_slice(&PREAMBLE_PA.to_be_bytes());
        packet.extend_from_slice(&PREAMBLE_PB.to_be_bytes());
        // Pc: data_type = 21 (0x15: E-AC-3), substream = 0, error = 0 -> 0x0015
        packet.extend_from_slice(&0x0015_u16.to_be_bytes());
        // Pd: 6 bytes payload
        packet.extend_from_slice(&0x0006_u16.to_be_bytes());
        // Payload: 6 dummy bytes
        packet.extend_from_slice(&[0x0B, 0x77, 0x01, 0x02, 0x03, 0x04]);

        parser.push_bytes(&packet);
        let burst = parser.next_burst().expect("burst expected").unwrap();

        assert_eq!(burst.data_type, Iec61937DataType::EnhancedAc3);
        assert_eq!(burst.substream_num, 0);
        assert!(!burst.error_flag);
        assert_eq!(burst.payload_bytes_length, 6);
        assert_eq!(burst.payload, vec![0x0B, 0x77, 0x01, 0x02, 0x03, 0x04]);
        assert!(!burst.was_byte_swapped);
    }

    #[test]
    fn parses_swapped_eac3_burst() {
        let mut parser = Iec61937Parser::new();

        let mut packet = Vec::new();
        // Pa = 0x72F8, Pb = 0x1F4E (byte swapped)
        packet.extend_from_slice(&PREAMBLE_PA_SWAPPED.to_be_bytes());
        packet.extend_from_slice(&PREAMBLE_PB_SWAPPED.to_be_bytes());
        // Pc: swapped 0x0015 -> 0x15, 0x00
        packet.extend_from_slice(&0x0015_u16.to_le_bytes());
        // Pd: swapped 4 bytes -> 0x04, 0x00
        packet.extend_from_slice(&0x0004_u16.to_le_bytes());
        // Swapped payload: [0x77, 0x0B, 0x02, 0x01] -> unswaps to [0x0B, 0x77, 0x01, 0x02]
        packet.extend_from_slice(&[0x77, 0x0B, 0x02, 0x01]);

        parser.push_bytes(&packet);
        let burst = parser.next_burst().expect("burst expected").unwrap();

        assert_eq!(burst.data_type, Iec61937DataType::EnhancedAc3);
        assert!(burst.was_byte_swapped);
        assert_eq!(burst.payload, vec![0x0B, 0x77, 0x01, 0x02]);
    }

    #[test]
    fn handles_fragmented_stream_and_noise() {
        let mut parser = Iec61937Parser::new();

        // Feed prefix garbage
        parser.push_bytes(&[0x00, 0x11, 0x22, 0x33, 0x44]);
        assert!(parser.next_burst().is_none());

        // Feed half of preamble
        parser.push_bytes(&[0xF8, 0x72]);
        assert!(parser.next_burst().is_none());

        // Feed rest of packet
        let mut rest = Vec::new();
        rest.extend_from_slice(&PREAMBLE_PB.to_be_bytes());
        rest.extend_from_slice(&0x0015_u16.to_be_bytes()); // E-AC-3
        rest.extend_from_slice(&0x0002_u16.to_be_bytes()); // 2 bytes
        rest.extend_from_slice(&[0x0B, 0x77]);
        parser.push_bytes(&rest);

        let burst = parser.next_burst().expect("burst expected").unwrap();
        assert_eq!(burst.data_type, Iec61937DataType::EnhancedAc3);
        assert_eq!(burst.payload, vec![0x0B, 0x77]);
    }
}
