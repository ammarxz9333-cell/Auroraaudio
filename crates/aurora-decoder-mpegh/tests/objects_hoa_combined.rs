use aurora_core::AudioBlock;
use aurora_decoder_api::DecodedFrame;
use aurora_decoder_mpegh::{
    evaluate_static_mpegh_objects_hoa_candidate, render_static_mpegh_object_plane,
    render_static_mpegh_objects_hoa_candidate, MpeghConformancePolicy, MpeghPairedEvidence,
    MpeghPlaybackChoice, MpeghRenderedPcm, MpeghSpeaker, MpeghSpeakerLayout,
};
use aurora_hoa_ir::{HoaCoefficientBinding, HoaCoefficientConvention, HoaCoefficientFrame};
use aurora_spatial_ir_v2::{
    CoordinateSpace, ObjectSignalBinding, SpatialDecodedFrame, SpatialDomain,
    SpatialFrameMetadata, SpatialObjectUpdate, SpatialPosition, SpatialRenderingProperties,
};
use aurora_spatial_transport_v2::{
    HoaGroupBinding, HoaSignalBinding, SpatialTransportFrame, TransportSceneDomain,
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
        ],
    }
}

fn pair(reference: Option<MpeghRenderedPcm>) -> MpeghPairedEvidence {
    MpeghPairedEvidence {
        scene: SpatialTransportFrame {
            frame: SpatialDecodedFrame {
                decoded: DecodedFrame {
                    audio: AudioBlock {
                        channels: vec![vec![0.25; FRAMES], vec![0.0; FRAMES]],
                        frame_count: FRAMES,
                        presentation_time_seconds: 0.0,
                        discontinuity: true,
                    },
                    objects: Vec::new(),
                },
                spatial: SpatialFrameMetadata {
                    domain: SpatialDomain::ObjectSignals,
                    bed_signals: Vec::new(),
                    object_signals: vec![ObjectSignalBinding {
                        id: "o0".into(),
                        pcm_channel_index: 0,
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
            domain: TransportSceneDomain::ObjectsAndHoa,
            bed_signals: Vec::new(),
            hoa_signals: vec![HoaSignalBinding {
                pcm_channel_index: 1,
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
fn combined_candidate_contains_object_and_hoa_energy() {
    let pair = pair(None);
    let object_only = render_static_mpegh_object_plane(&pair).unwrap();
    let combined = render_static_mpegh_objects_hoa_candidate(&pair, 0.01).unwrap();
    assert_eq!(combined.channels.len(), object_only.channels.len());
    let object_energy: f32 = object_only.channels.iter().flatten().map(|s| s * s).sum();
    let combined_energy: f32 = combined.channels.iter().flatten().map(|s| s * s).sum();
    assert!(combined_energy > object_energy);
}

#[test]
fn wrong_reference_forces_libmpegh_fallback() {
    let reference = MpeghRenderedPcm {
        bytes: vec![0; 6 * FRAMES * 2],
        bit_depth: 16,
        channel_count: 6,
        frame_count: FRAMES,
        sample_rate: 48_000,
    };
    let pair = pair(Some(reference));
    let decision = evaluate_static_mpegh_objects_hoa_candidate(
        &pair,
        0.01,
        MpeghConformancePolicy::near_reference(),
    )
    .unwrap();
    assert_eq!(decision.playback.evidence.choice, MpeghPlaybackChoice::LibmpeghReference);
}
