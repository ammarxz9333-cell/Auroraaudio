use aurora_core::{AudioBlock, ChannelRole};
use aurora_decoder_api::DecodedFrame;
use aurora_decoder_mpegh::{
    evaluate_exact_mpegh_scene_candidate, render_exact_mpegh_scene_candidate,
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

fn update(offset: u32, azimuth: f32) -> SpatialObjectUpdate {
    SpatialObjectUpdate {
        object_id: "o0".into(),
        active: true,
        coordinate_space: CoordinateSpace::SphericalDegrees,
        position: SpatialPosition::Spherical {
            azimuth_degrees: azimuth,
            elevation_degrees: 0.0,
            distance: 1.0,
        },
        gain_db: 0.0,
        spread: 0.0,
        metadata_sample_offset: offset,
        ramp_duration_samples: 0,
        priority: Some(1.0),
        rendering: SpatialRenderingProperties::default(),
    }
}

fn pair(reference: Option<MpeghRenderedPcm>) -> MpeghPairedEvidence {
    MpeghPairedEvidence {
        scene: SpatialTransportFrame {
            frame: SpatialDecodedFrame {
                decoded: DecodedFrame {
                    audio: AudioBlock {
                        channels: vec![
                            vec![0.4, -0.2, 0.1, -0.3], // LFE bed
                            vec![1.0; FRAMES],           // object
                            vec![0.0; FRAMES],           // HOA transport placeholder
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
                    object_updates: vec![update(0, 30.0), update(2, -30.0)],
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
                channels: vec![vec![0.1; FRAMES]],
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
        reference_layout: MpeghSpeakerLayout {
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
        },
    }
}

#[test]
fn exact_full_scene_preserves_lfe_and_moves_object_on_sample_boundary() {
    let rendered = render_exact_mpegh_scene_candidate(&pair(None), 0.01).unwrap();
    assert_eq!(rendered.channels[6], vec![0.4, -0.2, 0.1, -0.3]);
    assert!(rendered.channels[0][0] > rendered.channels[1][0]);
    assert!(rendered.channels[0][1] > rendered.channels[1][1]);
    assert!(rendered.channels[1][2] > rendered.channels[0][2]);
    assert!(rendered.channels[1][3] > rendered.channels[0][3]);
}

#[test]
fn exact_full_scene_is_still_reference_gated() {
    let reference = MpeghRenderedPcm {
        bytes: vec![0; 7 * FRAMES * 2],
        bit_depth: 16,
        channel_count: 7,
        frame_count: FRAMES,
        sample_rate: 48_000,
    };
    let decision = evaluate_exact_mpegh_scene_candidate(
        &pair(Some(reference)),
        0.01,
        MpeghConformancePolicy::near_reference(),
    )
    .unwrap();
    assert_eq!(decision.playback.evidence.choice, MpeghPlaybackChoice::LibmpeghReference);
}
