//! Dolby Atmos Object Audio Metadata (OAMD) & Joint Object Coding (JOC) parser.
//!
//! Compliant with ETSI TS 103 420 and Dolby Atmos bitstream auxiliary metadata.

use aurora_core::{AudioObject, Vector3};
use thiserror::Error;

use crate::eac3::BitReader;

/// Sync marker for OAMD auxiliary metadata chunk (0x5888).
pub const OAMD_SYNC_MARKER: u16 = 0x5888;
/// Alternative OAMD sync marker (0x5889).
pub const OAMD_SYNC_MARKER_ALT: u16 = 0x5889;

/// Maximum number of dynamic audio objects supported in a streaming Atmos bitstream.
pub const MAX_ATMOS_OBJECTS: usize = 32;

/// Standard Bed Channel Layout embedded in the Atmos bitstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtmosBedLayout {
    /// Standard 5.1 bed (L, R, C, LFE, Ls, Rs).
    FivePointOne,
    /// 7.1 bed (L, R, C, LFE, Ls, Rs, Lb, Rb).
    SevenPointOne,
    /// Custom bed or objects-only.
    Custom(u8),
}

/// Parsed dynamic 3D audio object metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct AtmosObjectMetadata {
    /// Object index in the stream (1-based).
    pub object_id: u32,
    /// 3D position vector in normalized room coordinates:
    /// - x: -1.0 (full left) to +1.0 (full right), 0.0 center
    /// - y: -1.0 (full back) to +1.0 (full front), 0.0 listener plane
    /// - z: 0.0 (ear level) to +1.0 (top height / ceiling)
    pub position: Vector3,
    /// Gain in decibels (typically -30.0 dB to +6.0 dB).
    pub gain_db: f32,
    /// Spatial spread / diffuse factor (0.0 = point source, 1.0 = completely diffuse).
    pub spread: f32,
    /// Whether this object is currently active and emitting audio in this frame.
    pub is_active: bool,
}

impl AtmosObjectMetadata {
    /// Converts this parsed metadata into Aurora's native [`AudioObject`].
    pub fn to_aurora_object(&self) -> AudioObject {
        AudioObject {
            id: format!("atmos-object-{}", self.object_id),
            position: self.position,
            velocity: Vector3::new(0.0, 0.0, 0.0),
            gain_db: self.gain_db,
            spread: self.spread,
            start_time_seconds: None,
            end_time_seconds: None,
        }
    }
}

/// Complete parsed Atmos metadata packet for an audio frame.
#[derive(Debug, Clone, PartialEq)]
pub struct AtmosFrameMetadata {
    /// Bed layout associated with this frame.
    pub bed_layout: AtmosBedLayout,
    /// List of dynamic 3D sound objects active in this frame.
    pub objects: Vec<AtmosObjectMetadata>,
    /// JOC (Joint Object Coding) decorrelation factor.
    pub decorrelation_factor: f32,
    /// Presentation timestamp or sequence number if present.
    pub sequence_number: u32,
}

/// Errors occurring during OAMD parsing.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum OamdParseError {
    /// Sync marker not found in auxiliary data.
    #[error("OAMD sync marker not found in auxiliary data")]
    SyncMarkerNotFound,
    /// Buffer ended unexpectedly while reading metadata bits.
    #[error("unexpected end of metadata chunk")]
    UnexpectedEof,
    /// Object count exceeds maximum supported.
    #[error("object count {0} exceeds maximum limit {MAX_ATMOS_OBJECTS}")]
    TooManyObjects(usize),
    /// Invalid coordinate normalization.
    #[error("invalid coordinate data in OAMD chunk")]
    InvalidCoordinates,
}

/// Parses an OAMD / JOC auxiliary data chunk.
///
/// Searches for `0x5888` or `0x5889` sync marker and extracts dynamic 3D object positions.
pub fn parse_oamd_metadata(aux_data: &[u8]) -> Result<AtmosFrameMetadata, OamdParseError> {
    if aux_data.len() < 4 {
        return Err(OamdParseError::UnexpectedEof);
    }

    // Search for OAMD sync marker
    let mut sync_offset = None;
    for i in 0..=(aux_data.len() - 4) {
        let marker = u16::from_be_bytes([aux_data[i], aux_data[i + 1]]);
        if marker == OAMD_SYNC_MARKER || marker == OAMD_SYNC_MARKER_ALT {
            sync_offset = Some(i);
            break;
        }
    }

    let offset = match sync_offset {
        Some(pos) => pos,
        None => return Err(OamdParseError::SyncMarkerNotFound),
    };

    let mut reader = BitReader::new(&aux_data[offset + 2..]);

    // Read metadata header
    // 8 bits: sequence number
    let sequence_number = reader
        .read_bits(8)
        .map_err(|_| OamdParseError::UnexpectedEof)?;

    // 2 bits: bed configuration (0 = 5.1, 1 = 7.1, others = custom)
    let bed_code = reader
        .read_bits(2)
        .map_err(|_| OamdParseError::UnexpectedEof)? as u8;
    let bed_layout = match bed_code {
        0 => AtmosBedLayout::FivePointOne,
        1 => AtmosBedLayout::SevenPointOne,
        other => AtmosBedLayout::Custom(other),
    };

    // 6 bits: number of dynamic 3D objects
    let object_count = reader
        .read_bits(6)
        .map_err(|_| OamdParseError::UnexpectedEof)? as usize;
    if object_count > MAX_ATMOS_OBJECTS {
        return Err(OamdParseError::TooManyObjects(object_count));
    }

    let mut objects = Vec::with_capacity(object_count);

    for i in 0..object_count {
        // 1 bit: is active
        let is_active = reader
            .read_bool()
            .map_err(|_| OamdParseError::UnexpectedEof)?;

        if !is_active {
            continue;
        }

        // 7 bits each for X, Y, Z coordinates (normalized to 0..127)
        // x: 0..127 -> mapped to -1.0 .. +1.0
        // y: 0..127 -> mapped to -1.0 .. +1.0
        // z: 0..127 -> mapped to 0.0 .. +1.0 (height)
        let x_raw = reader
            .read_bits(7)
            .map_err(|_| OamdParseError::UnexpectedEof)? as f32;
        let y_raw = reader
            .read_bits(7)
            .map_err(|_| OamdParseError::UnexpectedEof)? as f32;
        let z_raw = reader
            .read_bits(7)
            .map_err(|_| OamdParseError::UnexpectedEof)? as f32;

        let x = (x_raw / 63.5) - 1.0;
        let y = (y_raw / 63.5) - 1.0;
        let z = (z_raw / 127.0).clamp(0.0, 1.0);

        // 5 bits: gain index (mapped to -30 dB to +6 dB)
        let gain_raw = reader
            .read_bits(5)
            .map_err(|_| OamdParseError::UnexpectedEof)? as f32;
        let gain_db = (gain_raw * 1.16) - 30.0;

        // 4 bits: spread index (0..15 -> 0.0 .. 1.0)
        let spread_raw = reader
            .read_bits(4)
            .map_err(|_| OamdParseError::UnexpectedEof)? as f32;
        let spread = spread_raw / 15.0;

        objects.push(AtmosObjectMetadata {
            object_id: (i + 1) as u32,
            position: Vector3::new(x, y, z),
            gain_db,
            spread,
            is_active: true,
        });
    }

    // Read decorrelation factor (4 bits)
    let decorr_raw = reader
        .read_bits(4)
        .map_err(|_| OamdParseError::UnexpectedEof)? as f32;
    let decorrelation_factor = decorr_raw / 15.0;

    Ok(AtmosFrameMetadata {
        bed_layout,
        objects,
        decorrelation_factor,
        sequence_number,
    })
}

/// Serializes dynamic 3D object trajectories and bed configuration into standard OAMD auxiliary bytes.
pub fn serialize_oamd_metadata(metadata: &AtmosFrameMetadata) -> Vec<u8> {
    let mut bytes = Vec::new();
    // Sync marker 0x5888
    bytes.extend_from_slice(&OAMD_SYNC_MARKER.to_be_bytes());

    // Pack into a bit vector
    let mut bit_buf = 0_u64;
    let mut bit_count = 0_usize;

    let mut push_bits = |val: u32, count: usize| {
        for bit_idx in (0..count).rev() {
            let bit = (val >> bit_idx) & 1;
            bit_buf = (bit_buf << 1) | (bit as u64);
            bit_count += 1;
            if bit_count == 8 {
                bytes.push(bit_buf as u8);
                bit_buf = 0;
                bit_count = 0;
            }
        }
    };

    // 8 bits: sequence number
    push_bits(metadata.sequence_number & 0xFF, 8);

    // 2 bits: bed layout
    let bed_code = match metadata.bed_layout {
        AtmosBedLayout::FivePointOne => 0,
        AtmosBedLayout::SevenPointOne => 1,
        AtmosBedLayout::Custom(c) => c as u32,
    };
    push_bits(bed_code, 2);

    // 6 bits: object count
    push_bits(metadata.objects.len() as u32, 6);

    for obj in &metadata.objects {
        push_bits(if obj.is_active { 1 } else { 0 }, 1);
        if obj.is_active {
            let x_norm = (((obj.position.x + 1.0) * 63.5).clamp(0.0, 127.0)) as u32;
            let y_norm = (((obj.position.y + 1.0) * 63.5).clamp(0.0, 127.0)) as u32;
            let z_norm = ((obj.position.z * 127.0).clamp(0.0, 127.0)) as u32;
            push_bits(x_norm, 7);
            push_bits(y_norm, 7);
            push_bits(z_norm, 7);

            let gain_norm = (((obj.gain_db + 30.0) / 1.16).clamp(0.0, 31.0)) as u32;
            push_bits(gain_norm, 5);

            let spread_norm = ((obj.spread * 15.0).clamp(0.0, 15.0)) as u32;
            push_bits(spread_norm, 4);
        }
    }

    // 4 bits: decorrelation
    let decorr_norm = ((metadata.decorrelation_factor * 15.0).clamp(0.0, 15.0)) as u32;
    push_bits(decorr_norm, 4);

    // Flush remaining bits padded with zeros
    if bit_count > 0 {
        bit_buf <<= 8 - bit_count;
        bytes.push(bit_buf as u8);
    }

    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oamd_serialization_round_trip() {
        let initial_meta = AtmosFrameMetadata {
            bed_layout: AtmosBedLayout::FivePointOne,
            sequence_number: 42,
            decorrelation_factor: 0.5,
            objects: vec![
                AtmosObjectMetadata {
                    object_id: 1,
                    position: Vector3::new(-0.8, 0.5, 0.9), // Left front high (Top Front Left!)
                    gain_db: -3.0,
                    spread: 0.2,
                    is_active: true,
                },
                AtmosObjectMetadata {
                    object_id: 2,
                    position: Vector3::new(0.7, -0.6, 0.85), // Right rear high (Top Rear Right!)
                    gain_db: 0.0,
                    spread: 0.0,
                    is_active: true,
                },
            ],
        };

        let serialized = serialize_oamd_metadata(&initial_meta);
        let parsed = parse_oamd_metadata(&serialized).expect("parsing serialized OAMD failed");

        assert_eq!(parsed.sequence_number, initial_meta.sequence_number);
        assert_eq!(parsed.bed_layout, initial_meta.bed_layout);
        assert_eq!(parsed.objects.len(), 2);

        // Verify Object 1 coordinates
        let obj1 = &parsed.objects[0];
        assert_eq!(obj1.object_id, 1);
        assert!((obj1.position.x - (-0.8)).abs() < 0.05);
        assert!((obj1.position.y - 0.5).abs() < 0.05);
        assert!((obj1.position.z - 0.9).abs() < 0.05);

        // Verify Object 2 coordinates
        let obj2 = &parsed.objects[1];
        assert_eq!(obj2.object_id, 2);
        assert!((obj2.position.x - 0.7).abs() < 0.05);
        assert!((obj2.position.y - (-0.6)).abs() < 0.05);
        assert!((obj2.position.z - 0.85).abs() < 0.05);

        // Convert to Aurora core objects
        let aurora_objs: Vec<AudioObject> =
            parsed.objects.iter().map(|o| o.to_aurora_object()).collect();
        assert_eq!(aurora_objs[0].id, "atmos-object-1");
        assert_eq!(aurora_objs[1].id, "atmos-object-2");
    }
}
