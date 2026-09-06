//! Enhanced AC-3 (E-AC-3 / Dolby Digital Plus) bitstream parser.
//!
//! Compliant with ATSC A/52:2018 Annex E and ETSI TS 102 366 Annex E.

use thiserror::Error;

/// Standard E-AC-3 syncword: 0x0B77.
pub const EAC3_SYNCWORD: u16 = 0x0B77;

/// E-AC-3 stream type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eac3StreamType {
    /// Independent substream (primary audio program).
    Independent,
    /// Dependent substream (channel extension, e.g. 7.1).
    Dependent,
    /// Associated audio (e.g. commentary or visual description).
    IndependentAssociated,
    /// Reserved or unsupported.
    Reserved(u8),
}

impl From<u8> for Eac3StreamType {
    fn from(val: u8) -> Self {
        match val {
            0 => Self::Independent,
            1 => Self::Dependent,
            2 => Self::IndependentAssociated,
            other => Self::Reserved(other),
        }
    }
}

/// E-AC-3 audio coding mode (channel arrangement).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eac3AudioCodingMode {
    /// 1+0: Mono / Center
    Mono = 0,
    /// 1/0: 1 channel
    OneZero = 1,
    /// 2/0: Left, Right (Stereo)
    Stereo = 2,
    /// 3/0: Left, Center, Right (3.0)
    ThreeZero = 3,
    /// 2/1: Left, Right, Surround Mono (3.0 surround)
    TwoOne = 4,
    /// 3/1: Left, Center, Right, Surround Mono (4.0 surround)
    ThreeOne = 5,
    /// 2/2: Left, Right, Left Surround, Right Surround (4.0 quad)
    TwoTwo = 6,
    /// 3/2: Left, Center, Right, Left Surround, Right Surround (5.0 surround)
    ThreeTwo = 7,
}

impl From<u8> for Eac3AudioCodingMode {
    fn from(val: u8) -> Self {
        match val & 0x07 {
            0 => Self::Mono,
            1 => Self::OneZero,
            2 => Self::Stereo,
            3 => Self::ThreeZero,
            4 => Self::TwoOne,
            5 => Self::ThreeOne,
            6 => Self::TwoTwo,
            _ => Self::ThreeTwo,
        }
    }
}

impl Eac3AudioCodingMode {
    /// Returns the number of main (full-bandwidth) channels.
    pub fn main_channels_count(&self) -> usize {
        match self {
            Self::Mono | Self::OneZero => 1,
            Self::Stereo => 2,
            Self::ThreeZero => 3,
            Self::TwoOne => 3,
            Self::ThreeOne => 4,
            Self::TwoTwo => 4,
            Self::ThreeTwo => 5,
        }
    }
}

/// Parsed E-AC-3 sync frame header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Eac3Header {
    /// Stream type (independent, dependent, etc.).
    pub stream_type: Eac3StreamType,
    /// Substream ID (0..=7). Primary audio is usually 0.
    pub substream_id: u8,
    /// Frame size in bytes: (frmsiz + 1) * 2.
    pub frame_size_bytes: usize,
    /// Sample rate in Hertz (e.g. 48000, 44100).
    pub sample_rate: u32,
    /// Number of audio blocks per syncframe (1, 2, 3, or 6 blocks; 256 samples per block).
    pub num_audio_blocks: usize,
    /// Audio coding mode (channel arrangement).
    pub audio_coding_mode: Eac3AudioCodingMode,
    /// Whether the Low Frequency Effects (subwoofer) channel is present.
    pub lfe_present: bool,
    /// Bitstream identification (16 for E-AC-3).
    pub bsid: u8,
    /// Dialogue normalization value in dBFS (-1 to -31 dBFS).
    pub dialnorm_db: i8,
    /// Total channel count (main channels + LFE).
    pub total_channels: usize,
    /// Total PCM audio samples per channel in this syncframe.
    pub samples_per_channel: usize,
}

/// Bitstream reader helper for arbitrary bit alignment.
#[derive(Debug)]
pub struct BitReader<'a> {
    data: &'a [u8],
    bit_pos: usize,
}

impl<'a> BitReader<'a> {
    /// Creates a new bit reader wrapping the supplied byte slice.
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, bit_pos: 0 }
    }

    /// Returns the current bit position in the stream.
    pub fn bit_position(&self) -> usize {
        self.bit_pos
    }

    /// Returns remaining bits in the stream.
    pub fn remaining_bits(&self) -> usize {
        (self.data.len() * 8).saturating_sub(self.bit_pos)
    }

    /// Reads up to 32 bits from the bitstream.
    pub fn read_bits(&mut self, count: usize) -> Result<u32, Eac3ParseError> {
        if count == 0 {
            return Ok(0);
        }
        if count > 32 {
            return Err(Eac3ParseError::BitReadOverflow);
        }
        if self.bit_pos + count > self.data.len() * 8 {
            return Err(Eac3ParseError::UnexpectedEof);
        }

        let mut result = 0_u32;
        for _ in 0..count {
            let byte_idx = self.bit_pos / 8;
            let bit_idx = 7 - (self.bit_pos % 8);
            let bit = (self.data[byte_idx] >> bit_idx) & 1;
            result = (result << 1) | (bit as u32);
            self.bit_pos += 1;
        }

        Ok(result)
    }

    /// Reads a single boolean bit (1 = true, 0 = false).
    pub fn read_bool(&mut self) -> Result<bool, Eac3ParseError> {
        self.read_bits(1).map(|v| v == 1)
    }

    /// Skips a given number of bits.
    pub fn skip_bits(&mut self, count: usize) -> Result<(), Eac3ParseError> {
        if self.bit_pos + count > self.data.len() * 8 {
            return Err(Eac3ParseError::UnexpectedEof);
        }
        self.bit_pos += count;
        Ok(())
    }

    /// Returns a slice of the remaining bytes from the current byte boundary.
    pub fn remaining_bytes(&self) -> &'a [u8] {
        let byte_idx = (self.bit_pos + 7) / 8;
        if byte_idx < self.data.len() {
            &self.data[byte_idx..]
        } else {
            &[]
        }
    }
}

/// Errors during E-AC-3 bitstream parsing.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum Eac3ParseError {
    /// Stream does not begin with syncword 0x0B77.
    #[error("invalid E-AC-3 syncword: found {0:#06x}, expected 0x0b77")]
    InvalidSyncword(u16),
    /// Data ended prematurely.
    #[error("unexpected end of E-AC-3 bitstream")]
    UnexpectedEof,
    /// Bit read requested more than 32 bits.
    #[error("cannot read more than 32 bits in a single read")]
    BitReadOverflow,
    /// Unsupported sample rate code.
    #[error("unsupported sample rate code: {0}")]
    UnsupportedSampleRate(u8),
    /// Invalid frame size.
    #[error("invalid frame size: {0} bytes")]
    InvalidFrameSize(usize),
}

/// Parses an ATSC A/52 Annex E frame header from a slice starting with syncword 0x0B77.
pub fn parse_eac3_header(data: &[u8]) -> Result<Eac3Header, Eac3ParseError> {
    if data.len() < 6 {
        return Err(Eac3ParseError::UnexpectedEof);
    }

    let syncword = u16::from_be_bytes([data[0], data[1]]);
    if syncword != EAC3_SYNCWORD {
        return Err(Eac3ParseError::InvalidSyncword(syncword));
    }

    let mut reader = BitReader::new(&data[2..]);

    let strmtyp_raw = reader.read_bits(2)? as u8;
    let stream_type = Eac3StreamType::from(strmtyp_raw);
    let substream_id = reader.read_bits(3)? as u8;
    let frmsiz = reader.read_bits(11)? as usize;
    let frame_size_bytes = (frmsiz + 1) * 2;

    if frame_size_bytes < 6 {
        return Err(Eac3ParseError::InvalidFrameSize(frame_size_bytes));
    }

    let fscod = reader.read_bits(2)? as u8;
    let (sample_rate, num_audio_blocks) = if fscod == 0x03 {
        let fscod2 = reader.read_bits(2)? as u8;
        let rate = match fscod2 {
            0 => 24_000,
            1 => 22_050,
            2 => 16_000,
            _ => return Err(Eac3ParseError::UnsupportedSampleRate(fscod2)),
        };
        // Half-rate frames always have 6 blocks
        (rate, 6)
    } else {
        let rate = match fscod {
            0 => 48_000,
            1 => 44_100,
            2 => 32_000,
            _ => unreachable!(),
        };
        let numblkscod = reader.read_bits(2)? as u8;
        let blocks = match numblkscod {
            0 => 1,
            1 => 2,
            2 => 3,
            3 => 6,
            _ => unreachable!(),
        };
        (rate, blocks)
    };

    let acmod_raw = reader.read_bits(3)? as u8;
    let audio_coding_mode = Eac3AudioCodingMode::from(acmod_raw);
    let lfe_present = reader.read_bool()?;

    let bsid = reader.read_bits(5)? as u8;
    let dialnorm_raw = reader.read_bits(5)? as i8;
    // dialnorm code 0..=31 represents -1 to -31 dBFS
    let dialnorm_db = if dialnorm_raw == 0 { -31 } else { -dialnorm_raw };

    let mut total_channels = audio_coding_mode.main_channels_count();
    if lfe_present {
        total_channels += 1;
    }

    let samples_per_channel = num_audio_blocks * 256;

    Ok(Eac3Header {
        stream_type,
        substream_id,
        frame_size_bytes,
        sample_rate,
        num_audio_blocks,
        audio_coding_mode,
        lfe_present,
        bsid,
        dialnorm_db,
        total_channels,
        samples_per_channel,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_5_1_eac3_header() {
        // Construct standard 5.1 E-AC-3 header at 48kHz, 6 blocks
        // Byte 0,1: Syncword 0x0B77
        // Bits:
        // strmtyp: 00 (Independent)
        // substreamid: 000 (Substream 0)
        // frmsiz: 11 bits -> say 767 (0x2FF -> frame size = (767+1)*2 = 1536 bytes)
        //   Bits: [strmtyp: 00][substream: 000][frmsiz upper 3: 010] -> 00 000 010 = 0x02
        //   Byte 3: [frmsiz lower 8: 11111111] = 0xFF
        // fscod: 00 (48kHz)
        // numblkscod: 11 (6 blocks)
        // acmod: 111 (3/2, 5.0)
        // lfeon: 1 (LFE on -> 5.1)
        //   Byte 4: [fscod: 00][numblkscod: 11][acmod: 111][lfeon: 1] = 00 11 111 1 = 0x3F
        // bsid: 10000 (16, E-AC-3)
        // dialnorm: 11100 (28 -> -28 dBFS)
        //   Byte 5: [bsid: 10000][dialnorm upper 3: 111] = 10000 111 = 0x87
        //   Byte 6: [dialnorm lower 2: 00][padding: 000000] = 0x00

        let header_bytes = [
            0x0B, 0x77, // syncword
            0x02, 0xFF, // strmtyp=0, substream=0, frmsiz=767
            0x3F, // fscod=0 (48k), numblkscod=3 (6 blk), acmod=7 (3/2), lfeon=1
            0x87, 0x00, // bsid=16, dialnorm=28
        ];

        let header = parse_eac3_header(&header_bytes).expect("parse succeeded");

        assert_eq!(header.stream_type, Eac3StreamType::Independent);
        assert_eq!(header.substream_id, 0);
        assert_eq!(header.frame_size_bytes, 1536);
        assert_eq!(header.sample_rate, 48_000);
        assert_eq!(header.num_audio_blocks, 6);
        assert_eq!(header.audio_coding_mode, Eac3AudioCodingMode::ThreeTwo);
        assert!(header.lfe_present);
        assert_eq!(header.total_channels, 6); // 5.1
        assert_eq!(header.samples_per_channel, 1536);
        assert_eq!(header.bsid, 16);
        assert_eq!(header.dialnorm_db, -28);
    }
}
