//! Immersive compressed-format inspection and external-decoder adapters.
//!
//! IEC 61937 and elementary header inspection are implemented. Aurora does
//! not bundle E-AC-3/JOC, Dolby MAT, DTS-HD or DTS:X decoding. The OAMD module
//! is an Aurora test-fixture format and is not a Dolby metadata implementation.

pub mod autoswitch;
pub mod decoder;
pub mod dts;
pub mod eac3;
pub mod iec61937;
pub mod ingest;
pub mod mat;
pub mod oamd;

pub use autoswitch::{AutoSwitchStats, DetectedStreamFormat, FormatAutoSwitch};
pub use decoder::Eac3AtmosDecoder;
pub use dts::{parse_dts_frame, DtsChannelMode, DtsFrame, DtsHeader};
pub use eac3::{parse_eac3_header, BitReader, Eac3AudioCodingMode, Eac3Header, Eac3StreamType};
pub use iec61937::{Iec61937Burst, Iec61937DataType, Iec61937Error, Iec61937Parser};
pub use ingest::{IngestError, IngestTelemetry, LiveStreamIngest};
pub use mat::{parse_dolby_mat_payload, DolbyMatError, DolbyMatFrame};
pub use oamd::{parse_oamd_metadata, serialize_oamd_metadata, AtmosBedLayout, AtmosFrameMetadata, AtmosObjectMetadata};
