use aurora_core::{AudioBlock, ChannelRole};
use aurora_decoder_api::DecodedFrame;
use aurora_decoder_mpegh::{
    render_mpegh_hoa_candidate, render_pure_mpegh_hoa_candidate, MpeghHoaCandidateError,
    MpeghPairedEvidence, MpeghSpeaker, MpeghSpeakerLayout,
};
use aurora_hoa_ir::{
    HoaCoefficientBinding, HoaCoefficientConvention, HoaCoefficientFrame,
};
use aurora_spatial_ir_v2::{SpatialDecodedFrame, SpatialDomain, SpatialFrameMetadata};
use aurora_spatial_transport_v2::{
    BedSignalTarget, ExplicitSpeakerGeometry, HoaGroupBinding, HoaSignalBinding,
    SpeakerElevation, SpatialTransportFrame, TransportBedSignalBinding, TransportSceneDomain,
};

fn coefficient_frame(samples: Vec<f32>) -> HoaCoefficientFrame {
    let frame_count = samples.len();
    HoaCoefficientFrame {
        audio: AudioBlock {
            channels: vec![samples],
            frame_count,
            presentation_time_seconds: 0.0,
            discontinuity: false,
        },
        order: 0,
        convention: HoaCoefficientConvention::AcnN3d,
        coefficients: vec![HoaCoefficientBinding {
            pcm_channel_index: 0,
            coefficient_index: 0,
        }],
    }
}

fn reference_layout() -> MpeghSpeakerLayout {
    MpeghSpeakerLayout {
        cicp_index: 0,
        layout_code: 0,
        speakers: vec![
            MpeghSpeaker {
                is_lfe: false,
                azimuth_degrees: 30,
                elevation_degrees: 0,
            },
            MpeghSpeaker {
                is_lfe: true,
                azimuth_degrees: 0,
                elevation_degrees: 0,
            },
        ],
    }
}

fn pure_pair(samples: Vec<f32>) -> MpeghPairedEvidence {
    let frame_count = samples.len();
    let scene = SpatialTransportFrame {
        frame: SpatialDecodedFrame {
            decoded: DecodedFrame {
                audio: AudioBlock {
                    channels: vec![vec![0.0; frame_count]],
                    frame_count,
                    presentation_time_seconds: 2.0,
                    discontinuity: true,
                },
                objects: Vec::new(),
            },
            spatial: SpatialFrameMetadata {
                domain: SpatialDomain::DiscreteBed,
                bed_signals: Vec::new(),
                object_signals: Vec::new(),
                object_updates: Vec::new(),
            },
        },
        domain: TransportSceneDomain::HoaTransport,
        bed_signals: Vec::new(),
        hoa_signals: vec![HoaSignalBinding {
            pcm_channel_index: 0,
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
    };
    MpeghPairedEvidence {
        scene,
        hoa_coefficients: Some(coefficient_frame(samples)),
        reference: None,
        reference_layout: reference_layout(),
    }
}

fn bed_hoa_pair(
    bed_planes: Vec<Vec<f32>>,
    bed_targets: Vec<BedSignalTarget>,
    hoa_samples: Vec<f32>,
) -> MpeghPairedEvidence {
    assert_eq!(bed_planes.len(), bed_targets.len());
    let frame_count = hoa_samples.len();
    let bed_count = bed_planes.len();
    let mut scene_channels = bed_planes;
    scene_channels.push(vec![0.0; frame_count]);

    let bed_signals = bed_targets
        .into_iter()
        .enumerate()
        .map(|(pcm_channel_index, target)| TransportBedSignalBinding {
            pcm_channel_index,
            target,
        })
        .collect();

    let scene = SpatialTransportFrame {
        frame: SpatialDecodedFrame {
            decoded: DecodedFrame {
                audio: AudioBlock {
                    channels: scene_channels,
                    frame_count,
                    presentation_time_seconds: 2.0,
                    discontinuity: true,
                },
                objects: Vec::new(),
            },
            spatial: SpatialFrameMetadata {
                domain: SpatialDomain::DiscreteBed,
                bed_signals: Vec::new(),
                object_signals: Vec::new(),
                object_updates: Vec::new(),
            },
        },
        domain: TransportSceneDomain::BedAndHoa,
        bed_signals,
        hoa_signals: vec![HoaSignalBinding {
            pcm_channel_index: bed_count,
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
    };

    MpeghPairedEvidence {
        scene,
        hoa_coefficients: Some(coefficient_frame(hoa_samples)),
        reference: None,
        reference_layout: reference_layout(),
    }
}

#[test]
fn pure_hoa_candidate_keeps_lfe_silent_and_scene_timing() {
    let pair = pure_pair(vec![1.0, 0.5, -0.5, -1.0]);
    let rendered = render_pure_mpegh_hoa_candidate(&pair, 1.0e-6).unwrap();
    assert_eq!(rendered.channels.len(), 2);
    assert_eq!(rendered.channels[1], vec![0.0; 4]);
    assert_eq!(rendered.presentation_time_seconds, 2.0);
    assert!(rendered.discontinuity);
    assert!(rendered.channels[0].iter().any(|sample| sample.abs() > 0.0));
}

#[test]
fn pure_wrapper_rejects_mixed_scene() {
    let pair = bed_hoa_pair(
        vec![vec![0.1; 4]],
        vec![BedSignalTarget::SemanticRole(ChannelRole::FrontLeft)],
        vec![1.0; 4],
    );
    assert!(matches!(
        render_pure_mpegh_hoa_candidate(&pair, 1.0e-6),
        Err(MpeghHoaCandidateError::MixedSceneUnsupported { .. })
    ));
}

#[test]
fn bed_and_hoa_add_in_the_same_reference_speaker_order() {
    let hoa_samples = vec![0.2, -0.1];
    let baseline = render_mpegh_hoa_candidate(&pure_pair(hoa_samples.clone()), 1.0e-4).unwrap();
    let mixed = render_mpegh_hoa_candidate(
        &bed_hoa_pair(
            vec![vec![0.5, 0.25]],
            vec![BedSignalTarget::SemanticRole(ChannelRole::FrontLeft)],
            hoa_samples,
        ),
        1.0e-4,
    )
    .unwrap();

    assert_eq!(mixed.channels.len(), 2);
    assert!((mixed.channels[0][0] - (baseline.channels[0][0] + 0.5)).abs() < 1.0e-6);
    assert!((mixed.channels[0][1] - (baseline.channels[0][1] + 0.25)).abs() < 1.0e-6);
}

#[test]
fn lfe_bed_passes_through_while_hoa_lfe_contribution_stays_zero() {
    let bed = vec![0.75, -0.25];
    let mixed = render_mpegh_hoa_candidate(
        &bed_hoa_pair(
            vec![bed.clone()],
            vec![BedSignalTarget::SemanticRole(
                ChannelRole::LowFrequencyEffects,
            )],
            vec![1.0, 1.0],
        ),
        1.0e-4,
    )
    .unwrap();

    assert_eq!(mixed.channels[1], bed);
}

#[test]
fn duplicate_bed_destination_is_rejected() {
    let error = render_mpegh_hoa_candidate(
        &bed_hoa_pair(
            vec![vec![0.1, 0.1], vec![0.2, 0.2]],
            vec![
                BedSignalTarget::SemanticRole(ChannelRole::LowFrequencyEffects),
                BedSignalTarget::SemanticRole(ChannelRole::LowFrequencyEffects),
            ],
            vec![1.0, 1.0],
        ),
        1.0e-4,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        MpeghHoaCandidateError::DuplicateBedDestination { destination: 1 }
    ));
}

#[test]
fn unmatched_explicit_bed_geometry_is_rejected() {
    let error = render_mpegh_hoa_candidate(
        &bed_hoa_pair(
            vec![vec![0.1, 0.1]],
            vec![BedSignalTarget::ExplicitGeometry(ExplicitSpeakerGeometry {
                azimuth_degrees: 60.0,
                elevation: SpeakerElevation::Degrees(0.0),
                is_lfe: false,
            })],
            vec![1.0, 1.0],
        ),
        1.0e-4,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        MpeghHoaCandidateError::MissingBedGeometry { .. }
    ));
}
