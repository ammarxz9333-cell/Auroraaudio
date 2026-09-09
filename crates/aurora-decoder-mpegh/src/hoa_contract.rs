use aurora_hoa_ir::{coefficient_count_for_order, HoaCoefficientConvention};
use aurora_spatial_transport_v2::SpatialTransportFrame;
use thiserror::Error;

/// Proven structural requirements for converting one MPEG-H HOA transport
/// group into decoded HOA coefficients.
///
/// This is intentionally *not* a transport-to-coefficient conversion. MPEG-H
/// transport channels require the stateful spatial synthesis defined by the
/// codec (inverse dynamic correction, ambience synthesis, channel
/// reassignment, predominant-sound synthesis). Treating transport lanes as
/// coefficients is therefore never admitted by this contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpeghHoaDecodeContract {
    pub group_index: usize,
    pub order: u16,
    pub expected_coefficient_count: usize,
    pub transport_channel_count: usize,
    pub convention: HoaCoefficientConvention,
    pub uses_nfc: bool,
    pub rendering_matrix_present: bool,
    pub requires_stateful_transport_synthesis: bool,
}

impl MpeghHoaDecodeContract {
    pub fn from_transport_scene(
        scene: &SpatialTransportFrame,
    ) -> Result<Option<Self>, MpeghHoaContractError> {
        scene
            .validate()
            .map_err(|error| MpeghHoaContractError::InvalidTransportScene(error.to_string()))?;

        if scene.hoa_signals.is_empty() {
            return Ok(None);
        }
        if scene.hoa_groups.len() != 1 {
            return Err(MpeghHoaContractError::UnsupportedGroupCount {
                groups: scene.hoa_groups.len(),
            });
        }

        let group = &scene.hoa_groups[0];
        let expected_coefficient_count = coefficient_count_for_order(group.order)
            .map_err(|error| MpeghHoaContractError::InvalidOrder(error.to_string()))?;
        if group.transport_indices.len() != scene.hoa_signals.len() {
            return Err(MpeghHoaContractError::TransportOwnershipMismatch {
                group: group.transport_indices.len(),
                scene: scene.hoa_signals.len(),
            });
        }

        Ok(Some(Self {
            group_index: group.group_index,
            order: group.order,
            expected_coefficient_count,
            transport_channel_count: scene.hoa_signals.len(),
            convention: HoaCoefficientConvention::MpegHNativeIndexed,
            uses_nfc: group.uses_nfc,
            rendering_matrix_present: group.matrix.is_some(),
            requires_stateful_transport_synthesis: true,
        }))
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MpeghHoaContractError {
    #[error("invalid MPEG-H Spatial Transport V2 scene: {0}")]
    InvalidTransportScene(String),
    #[error("MPEG-H HOA scene contains {groups} groups; current coefficient contract requires exactly one")]
    UnsupportedGroupCount { groups: usize },
    #[error("MPEG-H HOA order is invalid: {0}")]
    InvalidOrder(String),
    #[error("HOA group owns {group} transport indices but scene exposes {scene} HOA transport lanes")]
    TransportOwnershipMismatch { group: usize, scene: usize },
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::AudioBlock;
    use aurora_decoder_api::DecodedFrame;
    use aurora_spatial_ir_v2::{SpatialDecodedFrame, SpatialDomain, SpatialFrameMetadata};
    use aurora_spatial_transport_v2::{
        HoaGroupBinding, HoaSignalBinding, SpatialTransportFrame, TransportSceneDomain,
    };

    fn hoa_scene(order: u16, transport_channels: usize) -> SpatialTransportFrame {
        let audio = AudioBlock {
            channels: vec![vec![0.0; 40]; transport_channels],
            frame_count: 40,
            presentation_time_seconds: 0.0,
            discontinuity: false,
        };
        let hoa_signals = (0..transport_channels)
            .map(|index| HoaSignalBinding {
                pcm_channel_index: index,
                transport_index: index,
            })
            .collect::<Vec<_>>();
        SpatialTransportFrame {
            frame: SpatialDecodedFrame {
                decoded: DecodedFrame { audio, objects: Vec::new() },
                spatial: SpatialFrameMetadata {
                    domain: SpatialDomain::DiscreteBed,
                    bed_signals: Vec::new(),
                    object_signals: Vec::new(),
                    object_updates: Vec::new(),
                },
            },
            domain: TransportSceneDomain::HoaTransport,
            bed_signals: Vec::new(),
            hoa_signals,
            hoa_groups: vec![HoaGroupBinding {
                group_index: 0,
                transport_indices: (0..transport_channels).collect(),
                order,
                fixed_position: false,
                priority: 0,
                uses_nfc: false,
                nfc_reference_distance_raw: None,
                matrix: None,
                screen_relative: false,
            }],
            codec_metadata: Vec::new(),
        }
    }

    #[test]
    fn order_three_contract_requires_sixteen_output_coefficients() {
        let contract = MpeghHoaDecodeContract::from_transport_scene(&hoa_scene(3, 6))
            .unwrap()
            .unwrap();
        assert_eq!(contract.expected_coefficient_count, 16);
        assert_eq!(contract.transport_channel_count, 6);
        assert!(contract.requires_stateful_transport_synthesis);
        assert_eq!(contract.convention, HoaCoefficientConvention::MpegHNativeIndexed);
    }

    #[test]
    fn no_hoa_returns_no_contract() {
        let mut scene = hoa_scene(1, 1);
        scene.hoa_signals.clear();
        scene.hoa_groups.clear();
        scene.frame.decoded.audio.channels.clear();
        scene.frame.decoded.audio.frame_count = 0;
        assert!(MpeghHoaDecodeContract::from_transport_scene(&scene).is_err());
    }
}
