//! Aurora MPEG-H 3D Audio backend.
//!
//! The default workspace build carries no native C linkage. Enabling
//! `native-mpegh` activates a narrow unsafe FFI boundary around the pinned
//! Ittiam libmpegh external-render interface. Packet data types remain available
//! in every build so callers do not get a feature-dependent public API.

#![cfg_attr(not(feature = "native-mpegh"), forbid(unsafe_code))]

/// One external-render access unit copied out of libmpegh before the next C
/// decoder call. These byte planes stay codec-native until Aurora admits a
/// verified MPEG-H channel/OAM/HOA -> Spatial IR V2 mapper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpeghExternalFrame {
    pub channel_metadata: Vec<u8>,
    pub object_metadata: Vec<u8>,
    pub hoa_metadata: Vec<u8>,
    pub prerender_pcm: Vec<u8>,
    pub pcm_bit_depth: i32,
    pub sample_rate: i32,
    pub oam_sample_offset: i32,
    pub hoa_sample_offset: i32,
    pub speaker_layout: MpeghSpeakerLayout,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpeghSpeakerLayout {
    pub cicp_index: i32,
    pub layout_code: i32,
    pub speakers: Vec<MpeghSpeaker>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MpeghSpeaker {
    pub is_lfe: bool,
    pub azimuth_degrees: i16,
    pub elevation_degrees: i16,
}

#[cfg(feature = "native-mpegh")]
mod ffi;
#[cfg(feature = "native-mpegh")]
mod native;

#[cfg(feature = "native-mpegh")]
pub use native::{MpeghNativeError, NativeMpeghDecoder};

/// Whether this build contains the native libmpegh external-render backend.
pub const fn native_backend_enabled() -> bool {
    cfg!(feature = "native-mpegh")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_build_reports_native_boundary_truthfully() {
        assert_eq!(native_backend_enabled(), cfg!(feature = "native-mpegh"));
    }

    #[test]
    fn packet_types_exist_without_native_linkage() {
        let frame = MpeghExternalFrame {
            channel_metadata: Vec::new(),
            object_metadata: Vec::new(),
            hoa_metadata: Vec::new(),
            prerender_pcm: Vec::new(),
            pcm_bit_depth: 24,
            sample_rate: 48_000,
            oam_sample_offset: 0,
            hoa_sample_offset: 0,
            speaker_layout: MpeghSpeakerLayout {
                cicp_index: 0,
                layout_code: 0,
                speakers: Vec::new(),
            },
        };
        assert_eq!(frame.sample_rate, 48_000);
    }
}
