use thiserror::Error;

const MAX_CHANNEL_GROUPS: usize = 32;
const MAX_SIGNALS_PER_GROUP: usize = 32;
const MAX_TOTAL_CHANNEL_SIGNALS: usize = 32;
const MAX_FLEX_SPEAKERS: usize = 32;
const MAX_EXTENSIONS: usize = 7;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpeghChannelMetadataPacket {
    pub frame_length_samples: u16,
    pub audio_truncation_code: u8,
    pub truncated_samples: Option<u16>,
    pub groups: Vec<MpeghChannelGroup>,
    pub consumed_bits: usize,
}

impl MpeghChannelMetadataPacket {
    pub fn total_signal_count(&self) -> usize {
        self.groups.iter().map(|group| group.signal_count).sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpeghChannelGroup {
    pub signal_count: usize,
    pub layout: MpeghSpeakerConfig,
    pub element_ids: Vec<u16>,
    pub fixed_position: bool,
    pub priority: u8,
    pub gain_code: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MpeghSpeakerConfig {
    CicpLayout { cicp_layout_index: u8 },
    CicpSpeakerList { speaker_indices: Vec<u8> },
    Flexible {
        angular_precision: MpeghAngularPrecision,
        speakers: Vec<MpeghFlexibleSpeaker>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpeghAngularPrecision {
    FiveDegrees,
    OneDegree,
}

impl MpeghAngularPrecision {
    pub fn degrees_per_step(self) -> i16 {
        match self {
            Self::FiveDegrees => 5,
            Self::OneDegree => 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MpeghFlexibleSpeaker {
    Cicp { speaker_index: u8 },
    Explicit(MpeghExplicitSpeaker),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpeghExplicitSpeaker {
    pub elevation_class: u8,
    pub elevation_angle_index: Option<u8>,
    pub elevation_negative: bool,
    pub azimuth_angle_index: u8,
    pub azimuth_negative: bool,
    pub is_lfe: bool,
    /// The external-render bitstream can encode a symmetric companion without
    /// repeating its complete descriptor. Aurora expands that companion into a
    /// second descriptor and marks it here so lane counts remain deterministic.
    pub symmetric_companion: bool,
}

impl MpeghExplicitSpeaker {
    pub fn azimuth_degrees(&self, precision: MpeghAngularPrecision) -> i16 {
        let magnitude = i16::from(self.azimuth_angle_index) * precision.degrees_per_step();
        if self.azimuth_negative { -magnitude } else { magnitude }
    }

    /// Elevation class 3 carries an explicit elevation angle. Other classes are
    /// retained losslessly as class codes because their semantic mapping belongs
    /// to the MPEG-H/CICP geometry table, not to this bitstream parser.
    pub fn explicit_elevation_degrees(
        &self,
        precision: MpeghAngularPrecision,
    ) -> Option<i16> {
        self.elevation_angle_index.map(|index| {
            let magnitude = i16::from(index) * precision.degrees_per_step();
            if self.elevation_negative { -magnitude } else { magnitude }
        })
    }
}

pub fn parse_external_channel_metadata(
    bytes: &[u8],
) -> Result<MpeghChannelMetadataPacket, MpeghChannelParseError> {
    let mut bits = BitReader::new(bytes);
    let frame_units = bits.read_u32(6)? as u16;
    let audio_truncation_code = bits.read_u32(2)? as u8;
    let truncated_samples = if audio_truncation_code > 0 {
        Some(bits.read_u32(13)? as u16)
    } else {
        None
    };

    let group_count = bits.read_u32(9)? as usize;
    if group_count > MAX_CHANNEL_GROUPS {
        return Err(MpeghChannelParseError::CountLimit {
            field: "channel groups",
            actual: group_count,
            maximum: MAX_CHANNEL_GROUPS,
        });
    }

    let mut groups = Vec::with_capacity(group_count);
    let mut total_signals = 0usize;
    for _ in 0..group_count {
        let signal_count = bits.read_u32(16)? as usize;
        if signal_count == 0 || signal_count > MAX_SIGNALS_PER_GROUP {
            return Err(MpeghChannelParseError::CountLimit {
                field: "signals per channel group",
                actual: signal_count,
                maximum: MAX_SIGNALS_PER_GROUP,
            });
        }
        total_signals = total_signals
            .checked_add(signal_count)
            .ok_or(MpeghChannelParseError::NumericOverflow)?;
        if total_signals > MAX_TOTAL_CHANNEL_SIGNALS {
            return Err(MpeghChannelParseError::CountLimit {
                field: "total channel signals",
                actual: total_signals,
                maximum: MAX_TOTAL_CHANNEL_SIGNALS,
            });
        }

        let layout = parse_speaker_config(&mut bits)?;
        validate_layout_cardinality(&layout, signal_count)?;

        let mut element_ids = Vec::with_capacity(signal_count);
        for _ in 0..signal_count {
            element_ids.push(bits.read_u32(9)? as u16);
        }
        let fixed_position = bits.read_bool()?;
        let priority = bits.read_u32(3)? as u8;
        let gain_code = bits.read_u32(8)? as u8;

        // The official external writer emits a complete downmix-config syntax
        // here. It has no outer byte length, therefore blindly skipping would
        // desynchronize every field that follows. Reject until Aurora carries a
        // mirrored downmix parser.
        if bits.read_bool()? {
            return Err(MpeghChannelParseError::UnsupportedDownmixConfig);
        }

        let extension_count = bits.read_u32(3)? as usize;
        if extension_count > MAX_EXTENSIONS {
            return Err(MpeghChannelParseError::CountLimit {
                field: "channel metadata extensions",
                actual: extension_count,
                maximum: MAX_EXTENSIONS,
            });
        }
        if extension_count != 0 {
            return Err(MpeghChannelParseError::UnsupportedExtensions {
                count: extension_count,
            });
        }

        groups.push(MpeghChannelGroup {
            signal_count,
            layout,
            element_ids,
            fixed_position,
            priority,
            gain_code,
        });
    }

    Ok(MpeghChannelMetadataPacket {
        frame_length_samples: frame_units.saturating_mul(64),
        audio_truncation_code,
        truncated_samples,
        groups,
        consumed_bits: bits.position(),
    })
}

fn parse_speaker_config(bits: &mut BitReader<'_>) -> Result<MpeghSpeakerConfig, MpeghChannelParseError> {
    match bits.read_u32(2)? {
        0 => Ok(MpeghSpeakerConfig::CicpLayout {
            cicp_layout_index: bits.read_u32(6)? as u8,
        }),
        1 => {
            let count = read_escape_value(bits, 5, 8, 16)?
                .checked_add(1)
                .ok_or(MpeghChannelParseError::NumericOverflow)? as usize;
            if count == 0 || count > MAX_FLEX_SPEAKERS {
                return Err(MpeghChannelParseError::CountLimit {
                    field: "CICP speaker list",
                    actual: count,
                    maximum: MAX_FLEX_SPEAKERS,
                });
            }
            let mut speaker_indices = Vec::with_capacity(count);
            for _ in 0..count {
                speaker_indices.push(bits.read_u32(7)? as u8);
            }
            Ok(MpeghSpeakerConfig::CicpSpeakerList { speaker_indices })
        }
        2 => parse_flexible_speaker_config(bits),
        other => Err(MpeghChannelParseError::ReservedSpeakerLayoutType(other as u8)),
    }
}

fn parse_flexible_speaker_config(
    bits: &mut BitReader<'_>,
) -> Result<MpeghSpeakerConfig, MpeghChannelParseError> {
    let count = read_escape_value(bits, 5, 8, 16)?
        .checked_add(1)
        .ok_or(MpeghChannelParseError::NumericOverflow)? as usize;
    if count == 0 || count > MAX_FLEX_SPEAKERS {
        return Err(MpeghChannelParseError::CountLimit {
            field: "flexible speakers",
            actual: count,
            maximum: MAX_FLEX_SPEAKERS,
        });
    }
    let precision = if bits.read_bool()? {
        MpeghAngularPrecision::OneDegree
    } else {
        MpeghAngularPrecision::FiveDegrees
    };
    let mut speakers = Vec::with_capacity(count);
    while speakers.len() < count {
        let speaker = parse_flexible_speaker(bits, precision, false)?;
        let azimuth = match &speaker {
            MpeghFlexibleSpeaker::Cicp { .. } => None,
            MpeghFlexibleSpeaker::Explicit(value) => Some(value.azimuth_degrees(precision)),
        };
        speakers.push(speaker.clone());

        if let Some(azimuth) = azimuth {
            if azimuth != 0 && azimuth.unsigned_abs() != 180 {
                let add_pair = bits.read_bool()?;
                if add_pair {
                    if speakers.len() >= count {
                        return Err(MpeghChannelParseError::SymmetricPairExceedsSpeakerCount);
                    }
                    let pair = match speaker {
                        MpeghFlexibleSpeaker::Explicit(mut value) => {
                            value.azimuth_negative = !value.azimuth_negative;
                            value.symmetric_companion = true;
                            MpeghFlexibleSpeaker::Explicit(value)
                        }
                        MpeghFlexibleSpeaker::Cicp { .. } => unreachable!("CICP branch has no explicit azimuth"),
                    };
                    speakers.push(pair);
                }
            }
        }
    }
    Ok(MpeghSpeakerConfig::Flexible {
        angular_precision: precision,
        speakers,
    })
}

fn parse_flexible_speaker(
    bits: &mut BitReader<'_>,
    precision: MpeghAngularPrecision,
    symmetric_companion: bool,
) -> Result<MpeghFlexibleSpeaker, MpeghChannelParseError> {
    if bits.read_bool()? {
        return Ok(MpeghFlexibleSpeaker::Cicp {
            speaker_index: bits.read_u32(7)? as u8,
        });
    }

    let elevation_class = bits.read_u32(2)? as u8;
    let (elevation_angle_index, elevation_negative) = if elevation_class == 3 {
        let width = match precision {
            MpeghAngularPrecision::OneDegree => 7,
            MpeghAngularPrecision::FiveDegrees => 5,
        };
        let index = bits.read_u32(width)? as u8;
        let maximum = match precision {
            MpeghAngularPrecision::OneDegree => 90,
            MpeghAngularPrecision::FiveDegrees => 18,
        };
        if index > maximum {
            return Err(MpeghChannelParseError::InvalidElevationIndex {
                index,
                maximum,
            });
        }
        let negative = if index != 0 { bits.read_bool()? } else { false };
        (Some(index), negative)
    } else {
        (None, false)
    };

    let azimuth_width = match precision {
        MpeghAngularPrecision::OneDegree => 8,
        MpeghAngularPrecision::FiveDegrees => 6,
    };
    let azimuth_angle_index = bits.read_u32(azimuth_width)? as u8;
    let azimuth_maximum = match precision {
        MpeghAngularPrecision::OneDegree => 180,
        MpeghAngularPrecision::FiveDegrees => 36,
    };
    if azimuth_angle_index > azimuth_maximum {
        return Err(MpeghChannelParseError::InvalidAzimuthIndex {
            index: azimuth_angle_index,
            maximum: azimuth_maximum,
        });
    }
    let azimuth_negative = if azimuth_angle_index != 0 && azimuth_angle_index != azimuth_maximum {
        bits.read_bool()?
    } else {
        false
    };
    let is_lfe = bits.read_bool()?;

    Ok(MpeghFlexibleSpeaker::Explicit(MpeghExplicitSpeaker {
        elevation_class,
        elevation_angle_index,
        elevation_negative,
        azimuth_angle_index,
        azimuth_negative,
        is_lfe,
        symmetric_companion,
    }))
}

fn validate_layout_cardinality(
    layout: &MpeghSpeakerConfig,
    signal_count: usize,
) -> Result<(), MpeghChannelParseError> {
    match layout {
        MpeghSpeakerConfig::CicpLayout { .. } => Ok(()),
        MpeghSpeakerConfig::CicpSpeakerList { speaker_indices }
            if speaker_indices.len() == signal_count => Ok(()),
        MpeghSpeakerConfig::Flexible { speakers, .. } if speakers.len() == signal_count => Ok(()),
        MpeghSpeakerConfig::CicpSpeakerList { speaker_indices } => {
            Err(MpeghChannelParseError::SpeakerSignalCountMismatch {
                speakers: speaker_indices.len(),
                signals: signal_count,
            })
        }
        MpeghSpeakerConfig::Flexible { speakers, .. } => {
            Err(MpeghChannelParseError::SpeakerSignalCountMismatch {
                speakers: speakers.len(),
                signals: signal_count,
            })
        }
    }
}

fn read_escape_value(
    bits: &mut BitReader<'_>,
    first_bits: usize,
    second_bits: usize,
    third_bits: usize,
) -> Result<u32, MpeghChannelParseError> {
    let first_escape = (1u32 << first_bits) - 1;
    let second_escape = (1u32 << second_bits) - 1;
    let first = bits.read_u32(first_bits)?;
    if first != first_escape {
        return Ok(first);
    }
    let second = bits.read_u32(second_bits)?;
    if second != second_escape {
        return first_escape
            .checked_add(second)
            .ok_or(MpeghChannelParseError::NumericOverflow);
    }
    let third = bits.read_u32(third_bits)?;
    first_escape
        .checked_add(second_escape)
        .and_then(|base| base.checked_add(third))
        .ok_or(MpeghChannelParseError::NumericOverflow)
}

struct BitReader<'a> {
    bytes: &'a [u8],
    bit_position: usize,
}

impl<'a> BitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, bit_position: 0 }
    }

    fn position(&self) -> usize {
        self.bit_position
    }

    fn read_bool(&mut self) -> Result<bool, MpeghChannelParseError> {
        Ok(self.read_u32(1)? != 0)
    }

    fn read_u32(&mut self, width: usize) -> Result<u32, MpeghChannelParseError> {
        if width > 32 {
            return Err(MpeghChannelParseError::NumericOverflow);
        }
        let end = self
            .bit_position
            .checked_add(width)
            .ok_or(MpeghChannelParseError::NumericOverflow)?;
        if end > self.bytes.len().saturating_mul(8) {
            return Err(MpeghChannelParseError::UnexpectedEnd {
                bit_position: self.bit_position,
                requested_bits: width,
                total_bits: self.bytes.len().saturating_mul(8),
            });
        }
        let mut value = 0u32;
        for _ in 0..width {
            let byte_index = self.bit_position / 8;
            let bit_in_byte = 7 - (self.bit_position % 8);
            value = (value << 1) | u32::from((self.bytes[byte_index] >> bit_in_byte) & 1);
            self.bit_position += 1;
        }
        Ok(value)
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MpeghChannelParseError {
    #[error("external channel metadata ended at bit {bit_position}; requested {requested_bits} more bits out of {total_bits}")]
    UnexpectedEnd {
        bit_position: usize,
        requested_bits: usize,
        total_bits: usize,
    },
    #[error("external channel metadata {field} count {actual} exceeds maximum {maximum}")]
    CountLimit {
        field: &'static str,
        actual: usize,
        maximum: usize,
    },
    #[error("reserved MPEG-H external speaker layout type {0}")]
    ReservedSpeakerLayoutType(u8),
    #[error("flexible speaker elevation index {index} exceeds {maximum}")]
    InvalidElevationIndex { index: u8, maximum: u8 },
    #[error("flexible speaker azimuth index {index} exceeds {maximum}")]
    InvalidAzimuthIndex { index: u8, maximum: u8 },
    #[error("flexible speaker symmetric pair exceeds declared speaker count")]
    SymmetricPairExceedsSpeakerCount,
    #[error("speaker layout declares {speakers} speakers for {signals} channel signals")]
    SpeakerSignalCountMismatch { speakers: usize, signals: usize },
    #[error("channel metadata contains a downmix configuration; mirrored parser not admitted yet")]
    UnsupportedDownmixConfig,
    #[error("channel metadata contains {count} production/extension elements; mirrored parser not admitted yet")]
    UnsupportedExtensions { count: usize },
    #[error("external channel metadata arithmetic overflow")]
    NumericOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct BitWriter { bytes: Vec<u8>, bits: usize }
    impl BitWriter {
        fn new() -> Self { Self { bytes: Vec::new(), bits: 0 } }
        fn push(&mut self, value: u32, width: usize) {
            for shift in (0..width).rev() {
                if self.bits % 8 == 0 { self.bytes.push(0); }
                let bit = ((value >> shift) & 1) as u8;
                if bit != 0 {
                    let last = self.bytes.len() - 1;
                    self.bytes[last] |= 1 << (7 - (self.bits % 8));
                }
                self.bits += 1;
            }
        }
    }

    #[test]
    fn parses_cicp_layout_group_without_guessing_geometry() {
        let mut w = BitWriter::new();
        w.push(16, 6); // 1024 samples
        w.push(0, 2); // no truncation
        w.push(1, 9); // one group
        w.push(2, 16); // two signals
        w.push(0, 2); // layout type 0
        w.push(2, 6); // CICP layout index
        w.push(0, 9);
        w.push(1, 9);
        w.push(1, 1); // fixed
        w.push(5, 3); // priority
        w.push(96, 8); // hardcoded upstream group gain
        w.push(0, 1); // no downmix
        w.push(0, 3); // no extension

        let parsed = parse_external_channel_metadata(&w.bytes).unwrap();
        assert_eq!(parsed.frame_length_samples, 1024);
        assert_eq!(parsed.total_signal_count(), 2);
        assert!(matches!(
            parsed.groups[0].layout,
            MpeghSpeakerConfig::CicpLayout { cicp_layout_index: 2 }
        ));
    }

    #[test]
    fn parses_flexible_explicit_speaker_and_symmetric_companion() {
        let mut w = BitWriter::new();
        w.push(16, 6);
        w.push(0, 2);
        w.push(1, 9);
        w.push(2, 16);
        w.push(2, 2); // flexible layout
        w.push(1, 5); // escape value 1 -> two speakers
        w.push(1, 1); // one-degree precision
        w.push(0, 1); // explicit speaker
        w.push(3, 2); // explicit elevation class
        w.push(30, 7);
        w.push(0, 1); // +30 elevation
        w.push(45, 8);
        w.push(0, 1); // +45 azimuth
        w.push(0, 1); // not LFE
        w.push(1, 1); // add symmetric companion
        w.push(0, 9);
        w.push(1, 9);
        w.push(0, 1);
        w.push(0, 3);
        w.push(96, 8);
        w.push(0, 1);
        w.push(0, 3);

        let parsed = parse_external_channel_metadata(&w.bytes).unwrap();
        let MpeghSpeakerConfig::Flexible { angular_precision, speakers } = &parsed.groups[0].layout else { panic!() };
        assert_eq!(*angular_precision, MpeghAngularPrecision::OneDegree);
        assert_eq!(speakers.len(), 2);
        let MpeghFlexibleSpeaker::Explicit(first) = &speakers[0] else { panic!() };
        let MpeghFlexibleSpeaker::Explicit(second) = &speakers[1] else { panic!() };
        assert_eq!(first.azimuth_degrees(*angular_precision), 45);
        assert_eq!(second.azimuth_degrees(*angular_precision), -45);
        assert!(second.symmetric_companion);
    }
}
