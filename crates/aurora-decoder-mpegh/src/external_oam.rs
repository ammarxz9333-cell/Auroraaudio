use thiserror::Error;

const MAX_OAM_OBJECTS: usize = 24;
const MAX_OAM_FRAMES: usize = 4;
const MAX_EXCLUSION_SECTORS: usize = 15;
const MAX_EXTENSIONS: usize = 7;

#[derive(Debug, Clone, PartialEq)]
pub struct MpeghOamPacket {
    pub frame_length_samples: u32,
    pub audio_truncation_code: u8,
    pub truncated_samples: Option<u16>,
    pub objects: Vec<MpeghOamObject>,
    pub extensions: Vec<MpeghOamExtension>,
    pub consumed_bits: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MpeghOamObject {
    pub element_id: u16,
    pub dynamic_priority_present: bool,
    pub uniform_spread_present: bool,
    pub frames: Vec<MpeghOamObjectFrame>,
    pub fixed_position: bool,
    pub group_priority: u8,
    pub diffuseness_code: u8,
    pub divergence_code: u8,
    pub divergence_azimuth_range_code: u8,
    pub exclusion_sectors: Vec<MpeghExclusionSector>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MpeghOamObjectFrame {
    pub has_metadata: bool,
    pub azimuth_degrees: Option<f32>,
    pub elevation_degrees: Option<f32>,
    pub radius: Option<f32>,
    pub gain_linear: Option<f32>,
    pub gain_db: Option<f32>,
    pub dynamic_priority: Option<u8>,
    pub spread_width_degrees: Option<f32>,
    pub spread_height_degrees: Option<f32>,
    pub spread_depth: Option<f32>,
    pub raw: Option<MpeghOamRawCodes>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MpeghOamRawCodes {
    pub azimuth: i16,
    pub elevation: i8,
    pub radius: u8,
    pub gain: i8,
    pub spread_width: i8,
    pub spread_height: Option<i8>,
    pub spread_depth: Option<i8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MpeghExclusionSector {
    Predefined { index: u8 },
    Custom {
        min_azimuth_code: i8,
        max_azimuth_code: i8,
        min_elevation_code: i8,
        max_elevation_code: i8,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpeghOamExtension {
    pub extension_type: u8,
    pub payload_length_bytes: u16,
    /// Payload bits repacked MSB-first into bytes. The final byte is padded with
    /// zeroes only when a future external interface emits a non-byte-aligned
    /// extension, while the current libmpegh writer declares byte lengths.
    pub payload: Vec<u8>,
}

pub fn parse_external_oam(bytes: &[u8]) -> Result<MpeghOamPacket, MpeghOamParseError> {
    let mut reader = BitReader::new(bytes);
    let frame_length_code = reader.read_u32(6)?;
    let frame_length_samples = frame_length_code
        .checked_shl(6)
        .ok_or(MpeghOamParseError::NumericOverflow)?;
    if frame_length_samples == 0 {
        return Err(MpeghOamParseError::InvalidFrameLength);
    }

    let audio_truncation_code = reader.read_u8(2)?;
    let truncated_samples = if audio_truncation_code > 0 {
        Some(reader.read_u16(13)?)
    } else {
        None
    };

    let object_count = usize::from(reader.read_u16(9)?);
    if object_count > MAX_OAM_OBJECTS {
        return Err(MpeghOamParseError::TooManyObjects {
            actual: object_count,
            maximum: MAX_OAM_OBJECTS,
        });
    }

    let mut objects = Vec::with_capacity(object_count);
    for _ in 0..object_count {
        objects.push(parse_object(&mut reader)?);
    }

    let extension_count = usize::from(reader.read_u8(3)?);
    if extension_count > MAX_EXTENSIONS {
        return Err(MpeghOamParseError::TooManyExtensions {
            actual: extension_count,
            maximum: MAX_EXTENSIONS,
        });
    }
    let mut extensions = Vec::with_capacity(extension_count);
    for _ in 0..extension_count {
        let extension_type = reader.read_u8(3)?;
        let payload_length_bytes = reader.read_u16(10)?;
        let payload_bits = usize::from(payload_length_bytes)
            .checked_mul(8)
            .ok_or(MpeghOamParseError::NumericOverflow)?;
        let payload = reader.read_packed_bits(payload_bits)?;
        extensions.push(MpeghOamExtension {
            extension_type,
            payload_length_bytes,
            payload,
        });
    }

    Ok(MpeghOamPacket {
        frame_length_samples,
        audio_truncation_code,
        truncated_samples,
        objects,
        extensions,
        consumed_bits: reader.position_bits(),
    })
}

fn parse_object(reader: &mut BitReader<'_>) -> Result<MpeghOamObject, MpeghOamParseError> {
    let element_id = reader.read_u16(9)?;
    let dynamic_priority_present = reader.read_bool()?;
    let uniform_spread_present = reader.read_bool()?;
    let frame_count = usize::from(reader.read_u8(6)?);
    if frame_count == 0 || frame_count > MAX_OAM_FRAMES {
        return Err(MpeghOamParseError::InvalidObjectFrameCount {
            actual: frame_count,
            maximum: MAX_OAM_FRAMES,
        });
    }

    let mut frames = Vec::with_capacity(frame_count);
    for _ in 0..frame_count {
        frames.push(parse_object_frame(
            reader,
            dynamic_priority_present,
            uniform_spread_present,
        )?);
    }

    let fixed_position = reader.read_bool()?;
    let group_priority = reader.read_u8(3)?;
    let diffuseness_code = reader.read_u8(7)?;
    let divergence_code = reader.read_u8(7)?;
    let divergence_azimuth_range_code = reader.read_u8(6)?;
    let exclusion_count = usize::from(reader.read_u8(4)?);
    if exclusion_count > MAX_EXCLUSION_SECTORS {
        return Err(MpeghOamParseError::TooManyExclusionSectors {
            actual: exclusion_count,
            maximum: MAX_EXCLUSION_SECTORS,
        });
    }
    let mut exclusion_sectors = Vec::with_capacity(exclusion_count);
    for _ in 0..exclusion_count {
        if reader.read_bool()? {
            exclusion_sectors.push(MpeghExclusionSector::Predefined {
                index: reader.read_u8(4)?,
            });
        } else {
            exclusion_sectors.push(MpeghExclusionSector::Custom {
                min_azimuth_code: reader.read_signed(7)? as i8,
                max_azimuth_code: reader.read_signed(7)? as i8,
                min_elevation_code: reader.read_signed(5)? as i8,
                max_elevation_code: reader.read_signed(5)? as i8,
            });
        }
    }

    Ok(MpeghOamObject {
        element_id,
        dynamic_priority_present,
        uniform_spread_present,
        frames,
        fixed_position,
        group_priority,
        diffuseness_code,
        divergence_code,
        divergence_azimuth_range_code,
        exclusion_sectors,
    })
}

fn parse_object_frame(
    reader: &mut BitReader<'_>,
    dynamic_priority_present: bool,
    uniform_spread_present: bool,
) -> Result<MpeghOamObjectFrame, MpeghOamParseError> {
    if !reader.read_bool()? {
        return Ok(MpeghOamObjectFrame {
            has_metadata: false,
            azimuth_degrees: None,
            elevation_degrees: None,
            radius: None,
            gain_linear: None,
            gain_db: None,
            dynamic_priority: None,
            spread_width_degrees: None,
            spread_height_degrees: None,
            spread_depth: None,
            raw: None,
        });
    }

    let azimuth = reader.read_signed(8)? as i16;
    let elevation = reader.read_signed(6)? as i8;
    let radius = reader.read_u8(4)?;
    let gain = reader.read_signed(7)? as i8;
    let dynamic_priority = dynamic_priority_present.then(|| reader.read_u8(3)).transpose()?;
    let spread_width = reader.read_signed(7)? as i8;
    let (spread_height, spread_depth) = if uniform_spread_present {
        (None, None)
    } else {
        (
            Some(reader.read_signed(5)? as i8),
            Some(reader.read_signed(4)? as i8),
        )
    };

    let azimuth_degrees = (f32::from(azimuth) * 1.5).clamp(-180.0, 180.0);
    let elevation_degrees = (f32::from(elevation) * 3.0).clamp(-90.0, 90.0);
    let radius_value = (2.0_f32).powf(f32::from(radius) / 3.0) / 2.0;
    let gain_db = (f32::from(gain) - 32.0) / 2.0;
    let gain_linear = 10.0_f32.powf(gain_db / 20.0).clamp(0.004, 5.957);
    let spread_width_degrees = (f32::from(spread_width) * 1.5).clamp(0.0, 180.0);
    let spread_height_degrees = spread_height
        .map(|code| (f32::from(code) * 3.0).clamp(0.0, 90.0));
    let spread_depth_value = spread_depth.map(|code| {
        ((2.0_f32).powf(f32::from(code) / 3.0) / 2.0 - 0.5).clamp(0.0, 15.5)
    });

    Ok(MpeghOamObjectFrame {
        has_metadata: true,
        azimuth_degrees: Some(azimuth_degrees),
        elevation_degrees: Some(elevation_degrees),
        radius: Some(radius_value.clamp(0.5, 16.0)),
        gain_linear: Some(gain_linear),
        gain_db: Some(gain_db),
        dynamic_priority,
        spread_width_degrees: Some(spread_width_degrees),
        spread_height_degrees,
        spread_depth: spread_depth_value,
        raw: Some(MpeghOamRawCodes {
            azimuth,
            elevation,
            radius,
            gain,
            spread_width,
            spread_height,
            spread_depth,
        }),
    })
}

struct BitReader<'a> {
    bytes: &'a [u8],
    bit: usize,
}

impl<'a> BitReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, bit: 0 }
    }

    fn position_bits(&self) -> usize {
        self.bit
    }

    fn read_bool(&mut self) -> Result<bool, MpeghOamParseError> {
        Ok(self.read_u8(1)? != 0)
    }

    fn read_u8(&mut self, bits: u8) -> Result<u8, MpeghOamParseError> {
        u8::try_from(self.read_unsigned(bits)?)
            .map_err(|_| MpeghOamParseError::NumericOverflow)
    }

    fn read_u16(&mut self, bits: u8) -> Result<u16, MpeghOamParseError> {
        u16::try_from(self.read_unsigned(bits)?)
            .map_err(|_| MpeghOamParseError::NumericOverflow)
    }

    fn read_u32(&mut self, bits: u8) -> Result<u32, MpeghOamParseError> {
        u32::try_from(self.read_unsigned(bits)?)
            .map_err(|_| MpeghOamParseError::NumericOverflow)
    }

    fn read_unsigned(&mut self, bits: u8) -> Result<u64, MpeghOamParseError> {
        if bits > 64 {
            return Err(MpeghOamParseError::NumericOverflow);
        }
        let end = self
            .bit
            .checked_add(usize::from(bits))
            .ok_or(MpeghOamParseError::NumericOverflow)?;
        if end > self.bytes.len().saturating_mul(8) {
            return Err(MpeghOamParseError::UnexpectedEnd {
                bit_offset: self.bit,
                requested_bits: bits,
            });
        }
        let mut value = 0_u64;
        for _ in 0..bits {
            let byte = self.bytes[self.bit / 8];
            let shift = 7 - (self.bit % 8);
            value = (value << 1) | u64::from((byte >> shift) & 1);
            self.bit += 1;
        }
        Ok(value)
    }

    fn read_signed(&mut self, bits: u8) -> Result<i64, MpeghOamParseError> {
        if bits == 0 || bits > 63 {
            return Err(MpeghOamParseError::NumericOverflow);
        }
        let raw = self.read_unsigned(bits)?;
        let sign = 1_u64 << (bits - 1);
        if raw & sign == 0 {
            Ok(raw as i64)
        } else {
            Ok(raw as i64 - (1_i64 << bits))
        }
    }

    fn read_packed_bits(&mut self, bits: usize) -> Result<Vec<u8>, MpeghOamParseError> {
        let end = self
            .bit
            .checked_add(bits)
            .ok_or(MpeghOamParseError::NumericOverflow)?;
        if end > self.bytes.len().saturating_mul(8) {
            return Err(MpeghOamParseError::UnexpectedEnd {
                bit_offset: self.bit,
                requested_bits: u8::try_from(bits.min(255)).unwrap_or(255),
            });
        }
        let mut output = vec![0_u8; bits.div_ceil(8)];
        for output_bit in 0..bits {
            let source_byte = self.bytes[self.bit / 8];
            let source_shift = 7 - (self.bit % 8);
            let value = (source_byte >> source_shift) & 1;
            let destination_shift = 7 - (output_bit % 8);
            output[output_bit / 8] |= value << destination_shift;
            self.bit += 1;
        }
        Ok(output)
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MpeghOamParseError {
    #[error("external OAM payload ended at bit {bit_offset} while reading {requested_bits} bits")]
    UnexpectedEnd {
        bit_offset: usize,
        requested_bits: u8,
    },
    #[error("external OAM frame length code resolves to zero samples")]
    InvalidFrameLength,
    #[error("external OAM declares {actual} objects; libmpegh supports at most {maximum}")]
    TooManyObjects { actual: usize, maximum: usize },
    #[error("external OAM object declares {actual} subframes; supported range is 1..={maximum}")]
    InvalidObjectFrameCount { actual: usize, maximum: usize },
    #[error("external OAM declares {actual} exclusion sectors; maximum is {maximum}")]
    TooManyExclusionSectors { actual: usize, maximum: usize },
    #[error("external OAM declares {actual} extensions; maximum encoded count is {maximum}")]
    TooManyExtensions { actual: usize, maximum: usize },
    #[error("external OAM numeric conversion overflow")]
    NumericOverflow,
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
            Self { bytes: Vec::new(), bit: 0 }
        }

        fn write(&mut self, value: u64, bits: u8) {
            for index in (0..bits).rev() {
                if self.bit % 8 == 0 {
                    self.bytes.push(0);
                }
                let one = ((value >> index) & 1) as u8;
                let shift = 7 - (self.bit % 8);
                let last = self.bytes.len() - 1;
                self.bytes[last] |= one << shift;
                self.bit += 1;
            }
        }

        fn signed(&mut self, value: i64, bits: u8) {
            let mask = (1_u64 << bits) - 1;
            self.write((value as u64) & mask, bits);
        }
    }

    #[test]
    fn parses_one_object_using_libmpegh_external_scaling() {
        let mut writer = BitWriter::new();
        writer.write(16, 6); // 1024 samples
        writer.write(0, 2);
        writer.write(1, 9); // objects
        writer.write(5, 9); // element id
        writer.write(1, 1); // dynamic priority
        writer.write(0, 1); // non-uniform spread
        writer.write(1, 6); // one OAM subframe
        writer.write(1, 1); // has metadata
        writer.signed(60, 8); // 90 deg azimuth
        writer.signed(10, 6); // 30 deg elevation
        writer.write(3, 4); // radius 1.0
        writer.signed(32, 7); // 0 dB
        writer.write(7, 3);
        writer.signed(20, 7); // 30 deg width
        writer.signed(10, 5); // 30 deg height
        writer.signed(3, 4); // depth 0.5
        writer.write(0, 1); // fixed position
        writer.write(6, 3);
        writer.write(64, 7); // diffuseness
        writer.write(32, 7); // divergence
        writer.write(12, 6);
        writer.write(0, 4); // exclusions
        writer.write(0, 3); // extensions

        let parsed = parse_external_oam(&writer.bytes).unwrap();
        assert_eq!(parsed.frame_length_samples, 1024);
        assert_eq!(parsed.objects.len(), 1);
        let object = &parsed.objects[0];
        assert_eq!(object.element_id, 5);
        let frame = &object.frames[0];
        assert_eq!(frame.azimuth_degrees, Some(90.0));
        assert_eq!(frame.elevation_degrees, Some(30.0));
        assert!((frame.radius.unwrap() - 0.5 * 2.0_f32.powf(1.0)).abs() < 1.0e-6);
        assert!((frame.gain_db.unwrap() - 0.0).abs() < 1.0e-6);
        assert_eq!(frame.dynamic_priority, Some(7));
        assert_eq!(frame.spread_width_degrees, Some(30.0));
        assert_eq!(frame.spread_height_degrees, Some(30.0));
        assert!((frame.spread_depth.unwrap() - 0.5).abs() < 1.0e-6);
    }

    #[test]
    fn rejects_object_count_beyond_upstream_limit_before_allocating_objects() {
        let mut writer = BitWriter::new();
        writer.write(16, 6);
        writer.write(0, 2);
        writer.write(25, 9);
        assert!(matches!(
            parse_external_oam(&writer.bytes),
            Err(MpeghOamParseError::TooManyObjects { actual: 25, .. })
        ));
    }
}
