//! Aurora MPEG-H 3D Audio backend.
//!
//! The default workspace build carries no native C linkage. Enabling
//! `native-mpegh` activates a narrow unsafe FFI boundary around the pinned
//! Ittiam libmpegh external-render interface. The public packet contract keeps
//! the same names and fields in either build mode.

#![cfg_attr(not(feature = "native-mpegh"), forbid(unsafe_code))]

mod external_channels;
mod external_oam;
mod external_pcm;
mod spatial_transport;
pub use external_channels::{
    parse_external_channel_metadata, MpeghAngularPrecision, MpeghChannelGroup,
    MpeghChannelMetadataPacket, MpeghChannelParseError, MpeghExplicitSpeaker,
    MpeghFlexibleSpeaker, MpeghSpeakerConfig,
};
pub use external_oam::{
    parse_external_oam, MpeghExclusionSector, MpeghOamExtension, MpeghOamObject,
    MpeghOamObjectFrame, MpeghOamPacket, MpeghOamParseError, MpeghOamRawCodes,
};
pub use external_pcm::{
    decode_prerender_pcm, MpeghExternalLane, MpeghExternalTopology, MpeghPcmTopologyError,
    MpeghPrerenderPcm,
};
pub use spatial_transport::{build_spatial_transport_v2, MpeghSpatialTransportError};

#[cfg(not(feature = "native-mpegh"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpeghExternalFrame {
    pub channel_metadata: Vec<u8>,
    pub object_metadata: Vec<u8>,
    pub hoa_metadata: Vec<u8>,
    pub prerender_pcm: Vec<u8>,
    pub pcm_bit_depth: i32,
    pub sample_rate: i32,
    /// Upstream calls this a sample offset, but libmpegh assigns `ch_offset`:
    /// it is the first object PCM lane index in the pre-render buffer.
    pub oam_sample_offset: i32,
    /// Upstream calls this a sample offset, but it is the first HOA transport
    /// PCM lane index in the pre-render buffer.
    pub hoa_sample_offset: i32,
    pub speaker_layout: MpeghSpeakerLayout,
}

#[cfg(not(feature = "native-mpegh"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpeghSpeakerLayout {
    pub cicp_index: i32,
    pub layout_code: i32,
    pub speakers: Vec<MpeghSpeaker>,
}

#[cfg(not(feature = "native-mpegh"))]
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
pub use native::{
    MpeghExternalFrame, MpeghNativeError, MpeghSpeaker, MpeghSpeakerLayout,
    NativeMpeghDecoder,
};

impl MpeghExternalFrame {
    /// Parse libmpegh's external-render OAM plane using the exact bit layout
    /// mirrored from its official writer/reader utilities.
    pub fn parse_object_metadata(&self) -> Result<MpeghOamPacket, MpeghOamParseError> {
        parse_external_oam(&self.object_metadata)
    }

    /// Parse libmpegh's external-render channel metadata without collapsing
    /// CICP/flexible geometry into Aurora speaker roles prematurely.
    pub fn parse_channel_metadata(
        &self,
    ) -> Result<MpeghChannelMetadataPacket, MpeghChannelParseError> {
        parse_external_channel_metadata(&self.channel_metadata)
    }
}

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

    #[cfg(not(feature = "native-mpegh"))]
    #[test]
    fn packet_contract_exists_without_native_linkage() {
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
