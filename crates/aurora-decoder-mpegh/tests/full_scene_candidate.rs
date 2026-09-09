use aurora_core::{AudioBlock, ChannelRole};
use aurora_decoder_api::DecodedFrame;
use aurora_decoder_mpegh::{
    evaluate_static_mpegh_full_scene_candidate, render_static_mpegh_full_scene_candidate,
    MpeghConformancePolicy, MpeghPairedEvidence, MpeghPlaybackChoice, MpeghRenderedPcm,
    MpeghSpeaker, MpeghSpeakerLayout,
};
use aurora_hoa_ir::{HoaCoefficientBinding, HoaCoefficientConvention, HoaCoefficientFrame};
use aurora_spatial_ir_v2::{
    CoordinateSpace, ObjectSignalBinding, SpatialDecodedFrame, SpatialDomain,
    SpatialFrameMetadata, SpatialObjectUpdate, SpatialPosition, SpatialRenderingProperties,
};
use aurora_spatial_transport_v2::{
    BedSignalTarget, HoaGroupBinding, HoaSignalBinding, SpatialTransportFrame,
    TransportBedSignalBinding, TransportSceneDomain,
};

const FRAMES: usize = 4;

fn layout() -> MpeghSpeakerLayout {
    MpeghSpeakerLayout {
        cicp_index: 0,
        layout_code: 0,
        speakers: vec![
            MpeghSpeaker { is_lfe: false, azimuth_degrees: 30, elevation_degrees: 0 },
            MpeghSpeaker { is_lfe: false, azimuth_degrees: -30, elevation_degrees: 0 },
            MpeghSpeaker { is_lfe: false, azimuth_degrees: 110, elevation_degrees: 0 },
            MpeghSpeaker { is_lfe: false, azimuth_degrees: -110, elevation_degrees: 0 },
            MpeghSpeaker { is_lfe: false, azimuth_degrees: 30, elevation_degrees: 35 },
            MpeghSpeaker { is_lfe: false, azimuth_degrees: -30, elevation_degrees: 35 },
            MpeghSpeaker { is_lfe: true, azimuth_degrees: 0, elevation_degrees: 0 },
        ],
    }
}

fn pair(reference: Option<MpeghRenderedPcm>) -> MpeghPairedEvidence {
    MpeghPairedEvidence {
        scene: SpatialTransportFrame {
            frame: SpatialDecodedFrame {
                decoded: DecodedFrame {
                    audio: AudioBlock {
                        channels: vec![
                            vec![0.4, -0.2, 0.1, -0.3],
                            vec![0.25; FRAMES],
                            vec![0.0; FRAMES],
                        ],
                        frame_count: FRAMES,
                        presentation_time_seconds: 0.0,
                        discontinuity: true,
                    },
                    objects: Vec::new(),
                },
                spatial: SpatialFrameMetadata {
                    domain: SpatialDomain::BedAndObjects,
                    bed_signals: Vec::new(),
                    object_signals: vec![ObjectSignalBinding {
                        id: "o0".into(),
                        pcm_channel_index: 1,
                    }],
                    object_updates: vec![SpatialObjectUpdate {
                        object_id: "o0".into(),
                        active: true,
                        coordinate_space: CoordinateSpace::SphericalDegrees,
                        position: SpatialPosition::Spherical {
                            azimuth_degrees: 30.0,
                            elevation_degrees: 0.0,
                            distance: 1.0,
                        },
                        gain_db: 0.0,
                        spread: 0.0,
                        metadata_sample_offset: 0,
                        ramp_duration_samples: 0,
                        priority: Some(1.0),
                        rendering: SpatialRenderingProperties::default(),
                    }],
                },
            },
            domain: TransportSceneDomain::BedObjectsAndHoa,
            bed_signals: vec![TransportBedSignalBinding {
                pcm_channel_index: 0,
                target: BedSignalTarget::SemanticRole(ChannelRole::LowFrequencyEffects),
            }],
            hoa_signals: vec![HoaSignalBinding {
                pcm_channel_index: 2,
                transport_index: 0,
            }],
            hoa_groups: vec![HoaGroupBinding {
                group_index: 0,
                transport_indices: vec![0],
                order: 0,
                fixed_position: false,
                priority: 0,
                uses_nfc: false,
                nfc_reference_distance_raw: None,
                matrix: None,
                screen_relative: false,
            }],
            codec_metadata: Vec::new(),
        },
        hoa_coefficients: Some(HoaCoefficientFrame {
            audio: AudioBlock {
                channels: vec![vec![0.2; FRAMES]],
                frame_count: FRAMES,
                presentation_time_seconds: 0.0,
                discontinuity: true,
            },
            order: 0,
            convention: HoaCoefficientConvention::AcnN3d,
            coefficients: vec![HoaCoefficientBinding {
                pcm_channel_index: 0,
                coefficient_index: 0,
            }],
        }),
        reference,
        reference_layout: layout(),
    }
}

#[test]
fn full_candidate_sums_bed_object_and_hoa_once() {
    let pair = pair(None);
    let rendered = render_static_mpegh_full_scene_candidate(&pair, 0.01).unwrap();
    assert_eq!(rendered.channels.len(), 7);
    assert_eq!(rendered.channels[6], vec![0.4, -0.2, 0.1, -0.3]);
    let full_range_energy: f32 = rendered.channels[..6]
        .iter()
        .flatten()
        .map(|sample| sample * sample)
        .sum();
    assert!(full_range_energy > 0.0);
}

#[test]
fn full_scene_still_falls_back_when_reference_disagrees() {
    let reference = MpeghRenderedPcm {
        bytes: vec![0; 7 * FRAMES * 2],
        bit_depth: 16,
        channel_count: 7,
        frame_count: FRAMES,
        sample_rate: 48_000,
    };
    let pair = pair(Some(reference));
    let decision = evaluate_static_mpegh_full_scene_candidate(
        &pair,
        0.01,
        MpeghConformancePolicy::near_reference(),
    )
    .unwrap();
    assert_eq!(decision.playback.evidence.choice, MpeghPlaybackChoice::LibmpeghReference);
}
