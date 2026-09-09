use thiserror::Error;

const MAX_HOA_GROUPS: usize = 32;
const MAX_HOA_ORDER: u16 = 29;
const MAX_HOA_MATRIX_BITS: usize = 6_144;
const MAX_SCREEN_PRESETS: usize = 31;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpeghHoaPacket {
    pub frame_length_samples: u16,
    pub audio_truncation_code: u8,
    pub truncated_samples: Option<u16>,
    pub groups: Vec<MpeghHoaGroup>,
    /// The external interface conditionally appends production metadata based
    /// on decoder-side configuration rather than an in-band presence flag.
    /// Preserve all remaining bits exactly instead of guessing that syntax.
    pub trailing_metadata: PackedBits,
    pub consumed_bits: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpeghHoaGroup {
    pub fixed_position: bool,
    pub priority: u8,
    pub order: u16,
    pub uses_nfc: bool,
    /// libmpegh writes `(UWORD32)nfc_ref_distance`; retain that exact transport
    /// value rather than inventing physical units.
    pub nfc_reference_distance_raw: Option<u32>,
    pub matrix: Option<MpeghHoaMatrixPayload>,
    pub screen_relative: bool,
    pub screen: Option<MpeghHoaScreenMetadata>,
}

impl MpeghHoaGroup {
    pub fn coefficient_count(&self) -> usize {
        let side = usize::from(self.order) + 1;
        side * side
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpeghHoaMatrixPayload {
    pub bit_length: usize,
    pub bits: PackedBits,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MpeghHoaScreenMetadata {
    pub base: MpeghProductionScreenSize,
    pub extension: MpeghProductionScreenExtension,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MpeghProductionScreenSize {
    pub has_non_standard_size: bool,
    pub azimuth_code: Option<u16>,
    pub top_elevation_code: Option<u16>,
    pub bottom_elevation_code: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MpeghProductionScreenExtension {
    pub overwrite_default: bool,
    pub default_left_azimuth_code: Option<u16>,
    pub default_right_azimuth_code: Option<u16>,
    pub presets: Vec<MpeghProductionScreenPreset>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpeghProductionScreenPreset {
    pub group_preset_id: u8,
    pub has_non_standard_size: bool,
    pub centered_in_azimuth: Option<bool>,
    pub left_azimuth_code: Option<u16>,
    pub right_azimuth_code: Option<u16>,
    pub top_elevation_code: Option<u16>,
    pub bottom_elevation_code: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackedBits {
    pub bit_length: usize,
    /// MSB-first packed bits. The unused low bits of the final byte are zero.
    pub bytes: Vec<u8>,
}

pub fn parse_external_hoa(bytes: &[u8]) -> Result<MpeghHoaPacket, MpeghHoaParseError> {
    let mut bits = BitReader::new(bytes);
    let frame_units = bits.read_u32(6)? as u16;
    let frame_length_samples = frame_units
        .checked_mul(64)
        .ok_or(MpeghHoaParseError::NumericOverflow)?;
    if frame_length_samples == 0 {
        return Err(MpeghHoaParseError::InvalidFrameLength);
    }

    let audio_truncation_code = bits.read_u32(2)? as u8;
    let truncated_samples = if audio_truncation_code > 0 {
        Some(bits.read_u32(13)? as u16)
    } else {
        None
    };

    let group_count = bits.read_u32(9)? as usize;
    if group_count == 0 || group_count > MAX_HOA_GROUPS {
        return Err(MpeghHoaParseError::InvalidGroupCount {
            actual: group_count,
            maximum: MAX_HOA_GROUPS,
        });
    }

    let mut groups = Vec::with_capacity(group_count);
    for _ in 0..group_count {
        groups.push(parse_group(&mut bits)?);
    }

    let trailing_metadata = bits.read_remaining_packed()?;
    let consumed_bits = bits.position();
    Ok(MpeghHoaPacket {
        frame_length_samples,
        audio_truncation_code,
        truncated_samples,
        groups,
        trailing_metadata,
        consumed_bits,
    })
}

fn parse_group(bits: &mut BitReader<'_>) -> Result<MpeghHoaGroup, MpeghHoaParseError> {
    let fixed_position = bits.read_bool()?;
    let priority = bits.read_u32(3)? as u8;
    let order = bits.read_u32(9)? as u16;
    if order > MAX_HOA_ORDER {
        return Err(MpeghHoaParseError::HoaOrderTooHigh {
            actual: order,
            maximum: MAX_HOA_ORDER,
        });
    }

    let uses_nfc = bits.read_bool()?;
    let nfc_reference_distance_raw = uses_nfc.then(|| bits.read_u32(32)).transpose()?;

    let matrix = if bits.read_bool()? {
        let matrix_len_bits = read_escape_value(bits, 8, 8, 12)? as usize;
        if matrix_len_bits == 0 || matrix_len_bits > MAX_HOA_MATRIX_BITS {
            return Err(MpeghHoaParseError::InvalidMatrixLength {
                bits: matrix_len_bits,
                maximum: MAX_HOA_MATRIX_BITS,
            });
        }
        Some(MpeghHoaMatrixPayload {
            bit_length: matrix_len_bits,
            bits: bits.read_packed(matrix_len_bits)?,
        })
    } else {
        None
    };

    let screen_relative = bits.read_bool()?;
    let screen = screen_relative.then(|| parse_screen(bits)).transpose()?;

    Ok(MpeghHoaGroup {
        fixed_position,
        priority,
        order,
        uses_nfc,
        nfc_reference_distance_raw,
        matrix,
        screen_relative,
        screen,
    })
}

fn parse_screen(bits: &mut BitReader<'_>) -> Result<MpeghHoaScreenMetadata, MpeghHoaParseError> {
    let has_non_standard_size = bits.read_bool()?;
    let base = if has_non_standard_size {
        MpeghProductionScreenSize {
            has_non_standard_size,
            azimuth_code: Some(bits.read_u32(9)? as u16),
            top_elevation_code: Some(bits.read_u32(9)? as u16),
            bottom_elevation_code: Some(bits.read_u32(9)? as u16),
        }
    } else {
        MpeghProductionScreenSize {
            has_non_standard_size,
            ..MpeghProductionScreenSize::default()
        }
    };

    let overwrite_default = bits.read_bool()?;
    let (default_left_azimuth_code, default_right_azimuth_code) = if overwrite_default {
        (
            Some(bits.read_u32(10)? as u16),
            Some(bits.read_u32(10)? as u16),
        )
    } else {
        (None, None)
    };

    let preset_count = bits.read_u32(5)? as usize;
    if preset_count > MAX_SCREEN_PRESETS {
        return Err(MpeghHoaParseError::TooManyScreenPresets {
            actual: preset_count,
            maximum: MAX_SCREEN_PRESETS,
        });
    }
    let mut presets = Vec::with_capacity(preset_count);
    for _ in 0..preset_count {
        let group_preset_id = bits.read_u32(5)? as u8;
        let has_non_standard_size = bits.read_bool()?;
        let mut preset = MpeghProductionScreenPreset {
            group_preset_id,
            has_non_standard_size,
            centered_in_azimuth: None,
            left_azimuth_code: None,
            right_azimuth_code: None,
            top_elevation_code: None,
            bottom_elevation_code: None,
        };
        if has_non_standard_size {
            let centered = bits.read_bool()?;
            preset.centered_in_azimuth = Some(centered);
            if centered {
                preset.left_azimuth_code = Some(bits.read_u32(9)? as u16);
            } else {
                preset.left_azimuth_code = Some(bits.read_u32(10)? as u16);
                preset.right_azimuth_code = Some(bits.read_u32(10)? as u16);
            }
            preset.top_elevation_code = Some(bits.read_u32(9)? as u16);
            preset.bottom_elevation_code = Some(bits.read_u32(9)? as u16);
        }
        presets.push(preset);
    }

    Ok(MpeghHoaScreenMetadata {
        base,
        extension: MpeghProductionScreenExtension {
            overwrite_default,
            default_left_azimuth_code,
            default_right_azimuth_code,
            presets,
        },
    })
}

fn read_escape_value(
    bits: &mut BitReader<'_>,
    first_bits: usize,
    second_bits: usize,
    third_bits: usize,
) -> Result<u32, MpeghHoaParseError> {
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
            .ok_or(MpeghHoaParseError::NumericOverflow);
    }
    let third = bits.read_u32(third_bits)?;
    first_escape
        .checked_add(second_escape)
        .and_then(|base| base.checked_add(third))
        .ok_or(MpeghHoaParseError::NumericOverflow)
}

struct BitReader<'a> {
    bytes: &'a [u8],
    bit_position: usize,
}

impl<'a> BitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            bit_position: 0,
        }
    }

    fn position(&self) -> usize {
        self.bit_position
    }

    fn remaining_bits(&self) -> usize {
        self.bytes
            .len()
            .saturating_mul(8)
            .saturating_sub(self.bit_position)
    }

    fn read_bool(&mut self) -> Result<bool, MpeghHoaParseError> {
        Ok(self.read_u32(1)? != 0)
    }

    fn read_u32(&mut self, width: usize) -> Result<u32, MpeghHoaParseError> {
        if width > 32 {
            return Err(MpeghHoaParseError::NumericOverflow);
        }
        let end = self
            .bit_position
            .checked_add(width)
            .ok_or(MpeghHoaParseError::NumericOverflow)?;
        if end > self.bytes.len().saturating_mul(8) {
            return Err(MpeghHoaParseError::UnexpectedEnd {
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

    fn read_packed(&mut self, bit_length: usize) -> Result<PackedBits, MpeghHoaParseError> {
        let byte_length = bit_length
            .checked_add(7)
            .ok_or(MpeghHoaParseError::NumericOverflow)?
            / 8;
        let mut bytes = vec![0u8; byte_length];
        for bit_index in 0..bit_length {
            if self.read_bool()? {
                bytes[bit_index / 8] |= 1 << (7 - (bit_index % 8));
            }
        }
        Ok(PackedBits { bit_length, bytes })
    }

    fn read_remaining_packed(&mut self) -> Result<PackedBits, MpeghHoaParseError> {
        self.read_packed(self.remaining_bits())
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MpeghHoaParseError {
    #[error("external HOA metadata ended at bit {bit_position}; requested {requested_bits} more bits out of {total_bits}")]
    UnexpectedEnd {
        bit_position: usize,
        requested_bits: usize,
        total_bits: usize,
    },
    #[error("external HOA frame length is zero")]
    InvalidFrameLength,
    #[error("external HOA group count {actual} is outside 1..={maximum}")]
    InvalidGroupCount { actual: usize, maximum: usize },
    #[error("HOA order {actual} exceeds admitted maximum {maximum}")]
    HoaOrderTooHigh { actual: u16, maximum: u16 },
    #[error("HOA matrix payload length {bits} bits is outside 1..={maximum}")]
    InvalidMatrixLength { bits: usize, maximum: usize },
    #[error("screen metadata preset count {actual} exceeds maximum {maximum}")]
    TooManyScreenPresets { actual: usize, maximum: usize },
    #[error("external HOA metadata arithmetic overflow")]
    NumericOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct BitWriter {
        bytes: Vec<u8>,
        bit_count: usize,
    }

    impl BitWriter {
        fn new() -> Self {
            Self {
                bytes: Vec::new(),
                bit_count: 0,
            }
        }

        fn push(&mut self, value: u32, width: usize) {
            for shift in (0..width).rev() {
                if self.bit_count % 8 == 0 {
                    self.bytes.push(0);
                }
                if ((value >> shift) & 1) != 0 {
                    let index = self.bytes.len() - 1;
                    self.bytes[index] |= 1 << (7 - (self.bit_count % 8));
                }
                self.bit_count += 1;
            }
        }
    }

    #[test]
    fn parses_basic_third_order_group() {
        let mut w = BitWriter::new();
        w.push(16, 6); // 1024 samples
        w.push(0, 2); // no truncation
        w.push(1, 9); // one HOA group
        w.push(1, 1); // fixed
        w.push(5, 3); // priority
        w.push(3, 9); // third order => 16 coefficients
        w.push(0, 1); // no NFC
        w.push(0, 1); // no matrix
        w.push(0, 1); // not screen-relative

        let parsed = parse_external_hoa(&w.bytes).unwrap();
        assert_eq!(parsed.frame_length_samples, 1024);
        assert_eq!(parsed.groups.len(), 1);
        assert_eq!(parsed.groups[0].order, 3);
        assert_eq!(parsed.groups[0].coefficient_count(), 16);
    }

    #[test]
    fn retains_matrix_payload_bits_without_interpreting_coefficients() {
        let mut w = BitWriter::new();
        w.push(16, 6);
        w.push(0, 2);
        w.push(1, 9);
        w.push(0, 1);
        w.push(0, 3);
        w.push(1, 9);
        w.push(0, 1);
        w.push(1, 1); // matrix present
        w.push(5, 8); // short escape: five matrix bits
        w.push(0b10110, 5);
        w.push(0, 1); // no screen metadata

        let parsed = parse_external_hoa(&w.bytes).unwrap();
        let matrix = parsed.groups[0].matrix.as_ref().unwrap();
        assert_eq!(matrix.bit_length, 5);
        assert_eq!(matrix.bits.bytes, vec![0b1011_0000]);
    }

    #[test]
    fn rejects_orders_above_reference_matrix_limit() {
        let mut w = BitWriter::new();
        w.push(16, 6);
        w.push(0, 2);
        w.push(1, 9);
        w.push(0, 1);
        w.push(0, 3);
        w.push(30, 9);
        assert!(matches!(
            parse_external_hoa(&w.bytes),
            Err(MpeghHoaParseError::HoaOrderTooHigh { .. })
        ));
    }
}
