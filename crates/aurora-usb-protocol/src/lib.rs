#![forbid(unsafe_code)]

pub const MAGIC: [u8; 4] = *b"AUR0";
pub const VERSION: u16 = 1;
pub const HEADER_LEN: usize = 32;
pub const SAMPLE_RATE_HZ: u32 = 48_000;
pub const DEFAULT_PERIOD_FRAMES: u16 = 256;
pub const DEFAULT_CHANNELS_7_1_4: u16 = 12;

pub const FLAG_PTS_VALID: u32 = 1 << 0;
pub const FLAG_DISCONTINUITY: u32 = 1 << 1;
pub const FLAG_END_OF_STREAM: u32 = 1 << 2;
pub const FLAG_XRUN_RECOVERY: u32 = 1 << 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Kind {
    EncodedIec61937 = 1,
    PcmS32Le = 2,
    ClockReport = 3,
    Config = 4,
    Ack = 5,
    Error = 6,
    Ping = 7,
    Pong = 8,
}

impl TryFrom<u16> for Kind {
    type Error = ProtocolError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::EncodedIec61937),
            2 => Ok(Self::PcmS32Le),
            3 => Ok(Self::ClockReport),
            4 => Ok(Self::Config),
            5 => Ok(Self::Ack),
            6 => Ok(Self::Error),
            7 => Ok(Self::Ping),
            8 => Ok(Self::Pong),
            other => Err(ProtocolError::UnknownKind(other)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub kind: Kind,
    pub flags: u32,
    pub sequence: u32,
    pub pts_48k: u64,
    pub payload_len: u32,
    pub aux: u32,
}

impl Header {
    pub const fn new(kind: Kind, payload_len: u32) -> Self {
        Self {
            kind,
            flags: 0,
            sequence: 0,
            pts_48k: 0,
            payload_len,
            aux: 0,
        }
    }

    pub const fn pcm_aux(channels: u16, frames: u16) -> u32 {
        ((channels as u32) << 16) | frames as u32
    }

    pub const fn pcm_channels(aux: u32) -> u16 {
        (aux >> 16) as u16
    }

    pub const fn pcm_frames(aux: u32) -> u16 {
        (aux & 0xffff) as u16
    }

    pub fn encode(self) -> [u8; HEADER_LEN] {
        let mut out = [0_u8; HEADER_LEN];
        out[0..4].copy_from_slice(&MAGIC);
        out[4..6].copy_from_slice(&VERSION.to_le_bytes());
        out[6..8].copy_from_slice(&(self.kind as u16).to_le_bytes());
        out[8..12].copy_from_slice(&self.flags.to_le_bytes());
        out[12..16].copy_from_slice(&self.sequence.to_le_bytes());
        out[16..24].copy_from_slice(&self.pts_48k.to_le_bytes());
        out[24..28].copy_from_slice(&self.payload_len.to_le_bytes());
        out[28..32].copy_from_slice(&self.aux.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() < HEADER_LEN {
            return Err(ProtocolError::ShortHeader(bytes.len()));
        }
        if bytes[0..4] != MAGIC {
            return Err(ProtocolError::BadMagic);
        }
        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        if version != VERSION {
            return Err(ProtocolError::UnsupportedVersion(version));
        }
        let kind = Kind::try_from(u16::from_le_bytes([bytes[6], bytes[7]]))?;
        Ok(Self {
            kind,
            flags: u32::from_le_bytes(bytes[8..12].try_into().expect("fixed slice")),
            sequence: u32::from_le_bytes(bytes[12..16].try_into().expect("fixed slice")),
            pts_48k: u64::from_le_bytes(bytes[16..24].try_into().expect("fixed slice")),
            payload_len: u32::from_le_bytes(bytes[24..28].try_into().expect("fixed slice")),
            aux: u32::from_le_bytes(bytes[28..32].try_into().expect("fixed slice")),
        })
    }

    pub fn validate_pcm_s32le(self) -> Result<(), ProtocolError> {
        if self.kind != Kind::PcmS32Le {
            return Err(ProtocolError::ExpectedPcm);
        }
        let channels = Self::pcm_channels(self.aux);
        let frames = Self::pcm_frames(self.aux);
        if channels == 0 || frames == 0 {
            return Err(ProtocolError::InvalidPcmShape { channels, frames });
        }
        let expected = channels as u32 * frames as u32 * 4;
        if self.payload_len != expected {
            return Err(ProtocolError::PcmPayloadLength {
                declared: self.payload_len,
                expected,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockReport {
    pub sink_sample_counter: u64,
    pub source_sample_counter: u64,
    pub queued_playback_frames: u32,
    pub capture_flags: u32,
}

impl ClockReport {
    pub const LEN: usize = 24;

    pub fn encode(self) -> [u8; Self::LEN] {
        let mut out = [0_u8; Self::LEN];
        out[0..8].copy_from_slice(&self.sink_sample_counter.to_le_bytes());
        out[8..16].copy_from_slice(&self.source_sample_counter.to_le_bytes());
        out[16..20].copy_from_slice(&self.queued_playback_frames.to_le_bytes());
        out[20..24].copy_from_slice(&self.capture_flags.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() != Self::LEN {
            return Err(ProtocolError::BadClockReportLength(bytes.len()));
        }
        Ok(Self {
            sink_sample_counter: u64::from_le_bytes(bytes[0..8].try_into().expect("fixed slice")),
            source_sample_counter: u64::from_le_bytes(bytes[8..16].try_into().expect("fixed slice")),
            queued_playback_frames: u32::from_le_bytes(bytes[16..20].try_into().expect("fixed slice")),
            capture_flags: u32::from_le_bytes(bytes[20..24].try_into().expect("fixed slice")),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolError {
    ShortHeader(usize),
    BadMagic,
    UnsupportedVersion(u16),
    UnknownKind(u16),
    ExpectedPcm,
    InvalidPcmShape { channels: u16, frames: u16 },
    PcmPayloadLength { declared: u32, expected: u32 },
    BadClockReportLength(usize),
}

impl core::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::ShortHeader(n) => write!(f, "short Aurora USB header: {n} bytes"),
            Self::BadMagic => write!(f, "bad Aurora USB magic"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported Aurora USB version {v}"),
            Self::UnknownKind(k) => write!(f, "unknown Aurora USB kind {k}"),
            Self::ExpectedPcm => write!(f, "expected PCM_S32LE frame"),
            Self::InvalidPcmShape { channels, frames } => {
                write!(f, "invalid PCM shape channels={channels} frames={frames}")
            }
            Self::PcmPayloadLength { declared, expected } => {
                write!(f, "PCM payload length {declared} != expected {expected}")
            }
            Self::BadClockReportLength(n) => write!(f, "bad CLOCK_REPORT length {n}"),
        }
    }
}

impl std::error::Error for ProtocolError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrip() {
        let h = Header {
            kind: Kind::PcmS32Le,
            flags: FLAG_PTS_VALID,
            sequence: 42,
            pts_48k: 123_456,
            payload_len: 12 * 256 * 4,
            aux: Header::pcm_aux(12, 256),
        };
        let decoded = Header::decode(&h.encode()).unwrap();
        assert_eq!(decoded, h);
        decoded.validate_pcm_s32le().unwrap();
    }

    #[test]
    fn rejects_bad_pcm_size() {
        let h = Header {
            kind: Kind::PcmS32Le,
            flags: 0,
            sequence: 0,
            pts_48k: 0,
            payload_len: 1,
            aux: Header::pcm_aux(12, 256),
        };
        assert!(matches!(
            h.validate_pcm_s32le(),
            Err(ProtocolError::PcmPayloadLength { .. })
        ));
    }

    #[test]
    fn clock_report_roundtrip() {
        let c = ClockReport {
            sink_sample_counter: 10,
            source_sample_counter: 20,
            queued_playback_frames: 512,
            capture_flags: 3,
        };
        assert_eq!(ClockReport::decode(&c.encode()).unwrap(), c);
    }
}
