//! Aurora MPEG-H 3D Audio backend.
//!
//! The default workspace build carries no native C linkage. Enabling
//! `native-mpegh` activates a narrow unsafe FFI boundary around the pinned
//! Ittiam libmpegh external-render interface. The public packet contract keeps
//! the same names and fields in either build mode.

#![cfg_attr(not(feature = "native-mpegh"), forbid(unsafe_code))]

mod conformance;
mod conformance_roles;
mod evidence_gate;
mod external_channels;
mod external_hoa;
mod external_oam;
mod external_pcm;
mod rendered_pcm;
mod spatial_transport;
pub use conformance::{
    compare_mpegh_render_to_reference, MpeghChannelConformance, MpeghConformanceError,
    MpeghConformancePolicy, MpeghConformanceReport,
};
pub use conformance_roles::{
    compare_mpegh_render_to_reference_by_role, reference_roles, MpeghRoleConformanceError,
};
pub use evidence_gate::{
    evaluate_mpegh_render_evidence, mpegh_reference_audio_block, MpeghEvidenceGateOutcome,
    MpeghPlaybackChoice,
};
pub use external_channels::{
    parse_external_channel_metadata, MpeghAngularPrecision, MpeghChannelGroup,
    MpeghChannelMetadataPacket, MpeghChannelParseError, MpeghExplicitSpeaker,
    MpeghFlexibleSpeaker, MpeghSpeakerConfig,
};
pub use external_hoa::{
    parse_external_hoa, MpeghHoaGroup, MpeghHoaMatrixPayload, MpeghHoaPacket,
    MpeghHoaParseError, MpeghHoaScreenMetadata, MpeghProductionScreenExtension,
    MpeghProductionScreenPreset, MpeghProductionScreenSize, PackedBits,
};
pub use external_oam::{
    parse_external_oam, MpeghExclusionSector, MpeghOamExtension, MpeghOamObject,
    MpeghOamObjectFrame, MpeghOamPacket, MpeghOamParseError, MpeghOamRawCodes,
};
pub use external_pcm::{
    decode_prerender_pcm, MpeghExternalLane, MpeghExternalTopology, MpeghPcmTopologyError,
    MpeghPrerenderPcm,
};
pub use rendered_pcm::{MpeghRenderedPcm, MpeghRenderedPcmError};
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
    pub oam_sample_offset: i32,
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
    pub fn parse_object_metadata(&self) -> Result<MpeghOamPacket, MpeghOamParseError> {
        parse_external_oam(&self.object_metadata)
    }

    pub fn parse_channel_metadata(
        &self,
    ) -> Result<MpeghChannelMetadataPacket, MpeghChannelParseError> {
        parse_external_channel_metadata(&self.channel_metadata)
    }

    pub fn parse_hoa_metadata(&self) -> Result<MpeghHoaPacket, MpeghHoaParseError> {
        parse_external_hoa(&self.hoa_metadata)
    }
}

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
