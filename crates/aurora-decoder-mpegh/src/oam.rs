use thiserror::Error;

const MAX_OBJECTS: usize = 256;
const MAX_SUBFRAMES: usize = 64;
const MAX_EXCLUSION_SECTORS: usize = 15;

#[derive(Debug, Clone, PartialEq)]
pub struct MpeghOamFrame {
    /// OAM sub-frame length exported as `(frame_length >> 6)` by libmpegh.
    pub object_frame_length_samples: u32,
    pub audio_truncation_mode: u8,
    pub truncated_samples: Option<u16>,
    pub objects: Vec<MpeghOamObject>,
    /// Extension elements are retained byte-for-byte until their production
    /// metadata semantics are mapped into Aurora Spatial IR V2.
    pub extensions: Vec<MpeghOamExtension>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MpeghOamObject {
    pub element_id: u16,
    pub dynamic_priority_present: bool,
    pub uniform_spread_present: bool,
    pub updates: Vec<MpeghOamUpdate>,
    pub fixed_position: bool,
    pub group_priority: u8,
    pub diffuseness_code: u8,
    pub divergence_code: u8,
    pub divergence_azimuth_range_code: u8,
    pub exclusion_sectors: Vec<MpeghExclusionSector>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MpeghOamUpdate {
    pub present: bool,
    pub azimuth_code: i16,
    pub elevation_code: i16,
    pub radius_code: u8,
    pub gain_code: i16,
    pub dynamic_priority: Option<u8>,
    pub spread_width_code: i16,
    pub spread_height_code: Option<i16>,
    pub spread_depth_code: Option<i16>,
}

impl MpeghOamUpdate {
    pub fn azimuth_degrees(self) -> f32 {
        f32::from(self.azimuth_code) * 1.5
    }

    pub fn elevation_degrees(self) -> f32 {
        f32::from(self.elevation_code) * 3.0
    }

    pub fn radius_meters(self) -> f32 {
        2.0_f32.powf(f32::from(self.radius_code) / 3.0) / 2.0
    }

    /// Equivalent dB value of libmpegh's gain descaling
    /// `10^((gain_code - 32)/40)`.
    pub fn gain_db(self) -> f32 {
        (f32::from(self.gain_code) - 32.0) * 0.5
    }

    pub fn gain_linear(self) -> f32 {
        10.0_f32.powf((f32::from(self.gain_code) - 32.0) / 40.0)
    }

    pub fn spread_width_degrees(self) -> f32 {
        f32::from(self.spread_width_code) * 1.5
    }

    pub fn spread_height_degrees(self) -> Option<f32> {
        self.spread_height_code.map(|value| f32::from(value) * 3.0)
    }

    pub fn spread_depth_meters(self) -> Option<f32> {
        self.spread_depth_code
            .map(|value| 2.0_f32.powf(f32::from(value) / 3.0) / 2.0 - 0.5)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MpeghExclusionSector {
    Predefined { index: u8 },
    Explicit {
        min_azimuth_code: u8,
        max_azimuth_code: u8,
        min_elevation_code: u8,
        max_elevation_code: u8,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpeghOamExtension {
    pub element_type: u8,
    pub payload: Vec<u8>,
}

pub fn parse_external_oam(bytes: &[u8]) -> Result<MpeghOamFrame, MpeghOamParseError> {
    let mut bits = BitReader::new(bytes);
    let frame_length_code = bits.read_u32(6)?;
    if frame_length_code == 0 {
        return Err(MpeghOamParseError::InvalidFrameLengthCode);
    }
    let object_frame_length_samples = frame_length_code
        .checked_mul(64)
        .ok_or(MpeghOamParseError::ArithmeticOverflow)?;
    let audio_truncation_mode = bits.read_u8(2)?;
    let truncated_samples = if audio_truncation_mode > 0 {
        Some(bits.read_u16(13)?)
    } else {
        None
    };

    let object_count = usize::from(bits.read_u16(9)?);
    if object_count > MAX_OBJECTS {
        return Err(MpeghOamParseError::ObjectLimitExceeded {
            count: object_count,
            maximum: MAX_OBJECTS,
        });
    }
    let mut objects = Vec::with_capacity(object_count);
    for _ in 0..object_count {
        let element_id = bits.read_u16(9)?;
        let dynamic_priority_present = bits.read_bool()?;
        let uniform_spread_present = bits.read_bool()?;
        let subframe_count = usize::from(bits.read_u8(6)?);
        if subframe_count > MAX_SUBFRAMES {
            return Err(MpeghOamParseError::SubframeLimitExceeded(subframe_count));
        }

        let mut updates = Vec::with_capacity(subframe_count);
        for _ in 0..subframe_count {
            let present = bits.read_bool()?;
            if !present {
                updates.push(MpeghOamUpdate {
                    present: false,
                    azimuth_code: 0,
                    elevation_code: 0,
                    radius_code: 0,
                    gain_code: 0,
                    dynamic_priority: None,
                    spread_width_code: 0,
                    spread_height_code: None,
                    spread_depth_code: None,
                });
                continue;
            }
            let azimuth_code = bits.read_signed(8)? as i16;
            let elevation_code = bits.read_signed(6)? as i16;
            let radius_code = bits.read_u8(4)?;
            let gain_code = bits.read_signed(7)? as i16;
            let dynamic_priority = dynamic_priority_present.then(|| bits.read_u8(3)).transpose()?;
            let spread_width_code = bits.read_signed(7)? as i16;
            let (spread_height_code, spread_depth_code) = if uniform_spread_present {
                (None, None)
            } else {
                (
                    Some(bits.read_signed(5)? as i16),
                    Some(bits.read_signed(4)? as i16),
                )
            };
            updates.push(MpeghOamUpdate {
                present,
                azimuth_code,
                elevation_code,
                radius_code,
                gain_code,
                dynamic_priority,
                spread_width_code,
                spread_height_code,
                spread_depth_code,
            });
        }

        let fixed_position = bits.read_bool()?;
        let group_priority = bits.read_u8(3)?;
        let diffuseness_code = bits.read_u8(7)?;
        let divergence_code = bits.read_u8(7)?;
        let divergence_azimuth_range_code = bits.read_u8(6)?;
        let exclusion_count = usize::from(bits.read_u8(4)?);
        if exclusion_count > MAX_EXCLUSION_SECTORS {
            return Err(MpeghOamParseError::ExclusionSectorLimitExceeded(
                exclusion_count,
            ));
        }
        let mut exclusion_sectors = Vec::with_capacity(exclusion_count);
        for _ in 0..exclusion_count {
            if bits.read_bool()? {
                exclusion_sectors.push(MpeghExclusionSector::Predefined {
                    index: bits.read_u8(4)?,
                });
            } else {
                exclusion_sectors.push(MpeghExclusionSector::Explicit {
                    min_azimuth_code: bits.read_u8(7)?,
                    max_azimuth_code: bits.read_u8(7)?,
                    min_elevation_code: bits.read_u8(5)?,
                    max_elevation_code: bits.read_u8(5)?,
                });
            }
        }
        objects.push(MpeghOamObject {
            element_id,
            dynamic_priority_present,
            uniform_spread_present,
            updates,
            fixed_position,
            group_priority,
            diffuseness_code,
            divergence_code,
            divergence_azimuth_range_code,
            exclusion_sectors,
        });
    }

    let extension_count = usize::from(bits.read_u8(3)?);
    let mut extensions = Vec::with_capacity(extension_count);
    for _ in 0..extension_count {
        let element_type = bits.read_u8(3)?;
        let payload_length = usize::from(bits.read_u16(10)?);
        let payload = bits.read_octets(payload_length)?;
        extensions.push(MpeghOamExtension {
            element_type,
            payload,
        });
    }

    // libmpegh rounds the written bit count up to bytes. Remaining bits may only
    // be zero padding; non-zero tail data indicates schema drift/corruption.
    if bits.remaining_bits() > 0 && bits.remaining_nonzero()? {
        return Err(MpeghOamParseError::NonZeroPadding);
    }

    Ok(MpeghOamFrame {
        object_frame_length_samples,
        audio_truncation_mode,
        truncated_samples,
        objects,
        extensions,
    })
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MpeghOamParseError {
    #[error("MPEG-H OAM external payload ended before the declared fields")]
    UnexpectedEnd,
    #[error("MPEG-H OAM external payload has an invalid zero frame-length code")]
    InvalidFrameLengthCode,
    #[error("MPEG-H OAM arithmetic overflow")]
    ArithmeticOverflow,
    #[error("MPEG-H OAM declares {count} objects, above Aurora limit {maximum}")]
    ObjectLimitExceeded { count: usize, maximum: usize },
    #[error("MPEG-H OAM declares invalid sub-frame count {0}")]
    SubframeLimitExceeded(usize),
    #[error("MPEG-H OAM declares invalid exclusion-sector count {0}")]
    ExclusionSectorLimitExceeded(usize),
    #[error("MPEG-H OAM has non-zero data after the declared payload")]
    NonZeroPadding,
}

struct BitReader<'a> {
    bytes: &'a [u8],
    bit: usize,
}

impl<'a> BitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, bit: 0 }
    }

    fn remaining_bits(&self) -> usize {
        self.bytes.len().saturating_mul(8).saturating_sub(self.bit)
    }

    fn read_bool(&mut self) -> Result<bool, MpeghOamParseError> {
        Ok(self.read_u32(1)? != 0)
    }

    fn read_u8(&mut self, width: u8) -> Result<u8, MpeghOamParseError> {
        Ok(self.read_u32(width)? as u8)
    }

    fn read_u16(&mut self, width: u8) -> Result<u16, MpeghOamParseError> {
        Ok(self.read_u32(width)? as u16)
    }

    fn read_signed(&mut self, width: u8) -> Result<i32, MpeghOamParseError> {
        let raw = self.read_u32(width)?;
        let sign = 1_u32 << (width - 1);
        if raw & sign == 0 {
            Ok(raw as i32)
        } else {
            Ok(raw as i32 - (1_i32 << width))
        }
    }

    fn read_u32(&mut self, width: u8) -> Result<u32, MpeghOamParseError> {
        if width == 0 || width > 32 || self.remaining_bits() < usize::from(width) {
            return Err(MpeghOamParseError::UnexpectedEnd);
        }
        let mut value = 0_u32;
        for _ in 0..width {
            let byte = self.bytes[self.bit / 8];
            let shift = 7 - (self.bit % 8);
            value = (value << 1) | u32::from((byte >> shift) & 1);
            self.bit += 1;
        }
        Ok(value)
    }

    fn read_octets(&mut self, count: usize) -> Result<Vec<u8>, MpeghOamParseError> {
        let required = count
            .checked_mul(8)
            .ok_or(MpeghOamParseError::ArithmeticOverflow)?;
        if self.remaining_bits() < required {
            return Err(MpeghOamParseError::UnexpectedEnd);
        }
        (0..count).map(|_| self.read_u8(8)).collect()
    }

    fn remaining_nonzero(&mut self) -> Result<bool, MpeghOamParseError> {
        while self.remaining_bits() > 0 {
            if self.read_u32(1)? != 0 {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct BitWriter {
        bytes: Vec<u8>,
        bit: usize,
    }

    impl BitWriter {
        fn new() -> Self {
            Self { bytes: vec![0], bit: 0 }
        }

        fn write(&mut self, value: u32, width: u8) {
            for index in (0..width).rev() {
                let bit_value = (value >> index) & 1;
                if self.bit / 8 == self.bytes.len() {
                    self.bytes.push(0);
                }
                if bit_value != 0 {
                    self.bytes[self.bit / 8] |= 1 << (7 - self.bit % 8);
                }
                self.bit += 1;
            }
        }

        fn signed(&mut self, value: i32, width: u8) {
            let mask = (1_u32 << width) - 1;
            self.write((value as u32) & mask, width);
        }

        fn finish(mut self) -> Vec<u8> {
            let used = self.bit.div_ceil(8);
            self.bytes.truncate(used);
            self.bytes
        }
    }

    #[test]
    fn parses_one_object_and_matches_libmpegh_descaling() {
        let mut w = BitWriter::new();
        w.write(16, 6); // 1024-sample object frame
        w.write(0, 2); // no truncation
        w.write(1, 9); // one object
        w.write(7, 9); // element id
        w.write(1, 1); // dynamic priority present
        w.write(0, 1); // non-uniform spread
        w.write(1, 6); // one metadata subframe
        w.write(1, 1); // metadata present
        w.signed(-20, 8); // -30 degrees azimuth
        w.signed(10, 6); // +30 degrees elevation
        w.write(3, 4); // radius = 1 m
        w.signed(32, 7); // 0 dB
        w.write(5, 3); // priority
        w.signed(20, 7); // 30 degree width
        w.signed(10, 5); // 30 degree height
        w.signed(3, 4); // 0.5 m depth
        w.write(0, 1); // not fixed
        w.write(4, 3); // group priority
        w.write(32, 7); // diffuseness
        w.write(64, 7); // divergence
        w.write(12, 6); // divergence az range
        w.write(0, 4); // no exclusion sectors
        w.write(0, 3); // no extensions

        let parsed = parse_external_oam(&w.finish()).unwrap();
        assert_eq!(parsed.object_frame_length_samples, 1024);
        assert_eq!(parsed.objects.len(), 1);
        let update = parsed.objects[0].updates[0];
        assert!((update.azimuth_degrees() + 30.0).abs() < 1.0e-6);
        assert!((update.elevation_degrees() - 30.0).abs() < 1.0e-6);
        assert!((update.radius_meters() - 1.0).abs() < 1.0e-6);
        assert!(update.gain_db().abs() < 1.0e-6);
        assert!((update.spread_width_degrees() - 30.0).abs() < 1.0e-6);
        assert!((update.spread_height_degrees().unwrap() - 30.0).abs() < 1.0e-6);
        assert!((update.spread_depth_meters().unwrap() - 0.5).abs() < 1.0e-6);
    }

    #[test]
    fn preserves_unknown_extension_bytes() {
        let mut w = BitWriter::new();
        w.write(16, 6);
        w.write(0, 2);
        w.write(0, 9); // zero objects is valid metadata-only scene
        w.write(1, 3); // one extension
        w.write(6, 3);
        w.write(2, 10);
        w.write(0xab, 8);
        w.write(0xcd, 8);
        let parsed = parse_external_oam(&w.finish()).unwrap();
        assert_eq!(parsed.extensions[0].element_type, 6);
        assert_eq!(parsed.extensions[0].payload, vec![0xab, 0xcd]);
    }
}
