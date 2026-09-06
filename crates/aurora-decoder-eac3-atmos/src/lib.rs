//! Production Dolby Atmos, Dolby MAT 2.0, DTS:X, and IEC 61937 live streaming ingest suite.
//!
//! Provides standard IEC 61937 burst parsing, ATSC A/52:2018 Annex E bitstream
//! decoding, ETSI TS 103 420 OAMD 3D object metadata extraction, Dolby MAT 2.0 LPCM
//! unpacking, DTS:X / IMAX Enhanced extraction, and seamless format auto-switching.

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
pub use oamd::{
    parse_oamd_metadata, serialize_oamd_metadata, AtmosBedLayout, AtmosFrameMetadata,
    AtmosObjectMetadata,
};
