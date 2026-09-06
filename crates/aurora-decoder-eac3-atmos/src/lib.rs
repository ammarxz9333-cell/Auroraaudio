//! Production Dolby Atmos, E-AC-3 JOC, and IEC 61937 live streaming ingest engine.
//!
//! Provides standard IEC 61937 burst parsing, ATSC A/52:2018 Annex E bitstream
//! decoding, ETSI TS 103 420 OAMD 3D object metadata extraction, and real-time
//! streaming ingest management for the Aurora Spatial Audio Platform.

pub mod decoder;
pub mod eac3;
pub mod iec61937;
pub mod ingest;
pub mod oamd;

pub use decoder::Eac3AtmosDecoder;
pub use eac3::{parse_eac3_header, BitReader, Eac3AudioCodingMode, Eac3Header, Eac3StreamType};
pub use iec61937::{Iec61937Burst, Iec61937DataType, Iec61937Error, Iec61937Parser};
pub use ingest::{IngestError, IngestTelemetry, LiveStreamIngest};
pub use oamd::{
    parse_oamd_metadata, serialize_oamd_metadata, AtmosBedLayout, AtmosFrameMetadata,
    AtmosObjectMetadata,
};
