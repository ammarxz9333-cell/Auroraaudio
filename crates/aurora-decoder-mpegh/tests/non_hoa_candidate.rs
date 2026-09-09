use aurora_core::{AudioBlock, ChannelRole};
use aurora_decoder_api::DecodedFrame;
use aurora_decoder_mpegh::{
    evaluate_mpegh_non_hoa_candidate, render_mpegh_non_hoa_candidate,
    MpeghConformancePolicy, MpeghNonHoaCandidateError, MpeghPairedEvidence,
    MpeghPlaybackChoice, MpeghRenderedPcm, MpeghSpeaker, MpeghSpeakerLayout,
};
use aurora_spatial_ir_v2::{
    CoordinateSpace, ObjectSignalBinding, SpatialDecodedFrame, SpatialDomain,
    SpatialFrameMetadata, SpatialObjectUpdate, SpatialPosition, SpatialRenderingProperties,
};
use aurora_spatial_transport_v2::{
    BedSignalTarget, ExplicitSpeakerGeometry, SpatialTransportFrame, SpeakerElevation,
    TransportBedSignalBinding, TransportSceneDomain,
};

const FRAMES: usize = 4;

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
                is_lfe: false,
                azimuth_degrees: -30,
                elevation_degrees: 0,
            },
            MpeghSpeaker {
                is_lfe: false,
                azimuth_degrees: 110,
                elevation_degrees: 0,
            },
            MpeghSpeaker {
                is_lfe: false,
                azimuth_degrees: -110,
                elevation_degrees: 0,
            },
            MpeghSpeaker {
                is_lfe: false,
                azimuth_degrees: 30,
                elevation_degrees: 35,
            },
            MpeghSpeaker {
                is_lfe: false,
                azimuth_degrees: -30,
                elevation_degrees: 35,
            },
        ],
    }
}

fn update(azimuth_degrees: f32) -> SpatialObjectUpdate {
    SpatialObjectUpdate {
        object_id: "o0".into(),
        active: true,
        coordinate_space: CoordinateSpace::SphericalDegrees,
        position: SpatialPosition::Spherical {
            azimuth_degrees,
            elevation_degrees: 0.0,
            distance: 1.0,
        },
        gain_db: 0.0,
        spread: 0.0,
        metadata_sample_offset: 0,
        ramp_duration_samples: 0,
        priority: Some(1.0),
        rendering: SpatialRenderingProperties::default(),
    }
}

fn pair(
    domain: TransportSceneDomain,
    channels: Vec<Vec<f32>>,
    bed_signals: Vec<TransportBedSignalBinding>,
    object_lane: Option<usize>,
    reference: Option<MpeghRenderedPcm>,
) -> MpeghPairedEvidence {
    let spatial_domain = match (bed_signals.is_empty(), object_lane.is_some()) {
        (false, true) => SpatialDomain::BedAndObjects,
        (true, true) => SpatialDomain::ObjectSignals,
        _ => SpatialDomain::DiscreteBed,
    };
    let object_signals = object_lane
        .map(|pcm_channel_index| {
            vec![ObjectSignalBinding {
                id: "o0".into(),
                pcm_channel_index,
            }]
        })
        .unwrap_or_default();
    let object_updates = object_lane.map(|_| vec![update(-30.0)]).unwrap_or_default();

    MpeghPairedEvidence {
        scene: SpatialTransportFrame {
            frame: SpatialDecodedFrame {
                decoded: DecodedFrame {
                    audio: AudioBlock {
                        channels,
                        frame_count: FRAMES,
                        presentation_time_seconds: 1.25,
                        discontinuity: true,
                    },
                    objects: Vec::new(),
                },
                spatial: SpatialFrameMetadata {
                    domain: spatial_domain,
                    bed_signals: Vec::new(),
                    object_signals,
                    object_updates,
                },
            },
            domain,
            bed_signals,
            hoa_signals: Vec::new(),
            hoa_groups: Vec::new(),
            codec_metadata: Vec::new(),
        },
        hoa_coefficients: None,
        reference,
        reference_layout: reference_layout(),
    }
}

fn silent_reference() -> MpeghRenderedPcm {
    let channel_count = reference_layout().speakers.len();
    MpeghRenderedPcm {
        bytes: vec![0_u8; FRAMES * channel_count * 2],
        bit_depth: 16,
        channel_count,
        frame_count: FRAMES,
        sample_rate: 48_000,
    }
}

#[test]
fn discrete_bed_routes_directly_in_reference_order() {
    let source = vec![0.125, -0.25, 0.5, -0.75];
    let pair = pair(
        TransportSceneDomain::DiscreteBed,
        vec![source.clone()],
        vec![TransportBedSignalBinding {
            pcm_channel_index: 0,
            target: BedSignalTarget::SemanticRole(ChannelRole::FrontLeft),
        }],
        None,
        None,
    );

    let rendered = render_mpegh_non_hoa_candidate(&pair).unwrap();
    assert_eq!(rendered.channels.len(), 6);
    assert_eq!(rendered.channels[0], source);
    assert!(rendered.channels[1..]
        .iter()
        .flatten()
        .all(|sample| sample.abs() < 1.0e-7));
    assert_eq!(rendered.presentation_time_seconds, 1.25);
    assert!(rendered.discontinuity);
}

#[test]
fn object_only_candidate_needs_no_hoa_coefficients() {
    let pair = pair(
        TransportSceneDomain::ObjectSignals,
        vec![vec![1.0; FRAMES]],
        Vec::new(),
        Some(0),
        None,
    );

    let rendered = render_mpegh_non_hoa_candidate(&pair).unwrap();
    assert_eq!(rendered.channels.len(), 6);
    assert!(rendered.channels[1]
        .iter()
        .any(|sample| sample.abs() > 0.5));
}

#[test]
fn bed_and_objects_sum_into_one_reference_order_candidate() {
    let pair = pair(
        TransportSceneDomain::BedAndObjects,
        vec![vec![0.25; FRAMES], vec![1.0; FRAMES]],
        vec![TransportBedSignalBinding {
            pcm_channel_index: 0,
            target: BedSignalTarget::SemanticRole(ChannelRole::FrontLeft),
        }],
        Some(1),
        None,
    );

    let rendered = render_mpegh_non_hoa_candidate(&pair).unwrap();
    assert!(rendered.channels[0]
        .iter()
        .all(|sample| *sample >= 0.25 - 1.0e-6));
    assert!(rendered.channels[1]
        .iter()
        .any(|sample| sample.abs() > 0.5));
}

#[test]
fn duplicate_bed_destination_is_rejected() {
    let pair = pair(
        TransportSceneDomain::DiscreteBed,
        vec![vec![0.1; FRAMES], vec![0.2; FRAMES]],
        vec![
            TransportBedSignalBinding {
                pcm_channel_index: 0,
                target: BedSignalTarget::SemanticRole(ChannelRole::FrontLeft),
            },
            TransportBedSignalBinding {
                pcm_channel_index: 1,
                target: BedSignalTarget::SemanticRole(ChannelRole::FrontLeft),
            },
        ],
        None,
        None,
    );

    assert!(matches!(
        render_mpegh_non_hoa_candidate(&pair),
        Err(MpeghNonHoaCandidateError::DuplicateBedDestination { destination: 0 })
    ));
}

#[test]
fn unmatched_explicit_bed_geometry_is_rejected() {
    let pair = pair(
        TransportSceneDomain::DiscreteBed,
        vec![vec![0.1; FRAMES]],
        vec![TransportBedSignalBinding {
            pcm_channel_index: 0,
            target: BedSignalTarget::ExplicitGeometry(ExplicitSpeakerGeometry {
                azimuth_degrees: 60.0,
                elevation: SpeakerElevation::Degrees(0.0),
                is_lfe: false,
            }),
        }],
        None,
        None,
    );

    assert!(matches!(
        render_mpegh_non_hoa_candidate(&pair),
        Err(MpeghNonHoaCandidateError::MissingBedGeometry { .. })
    ));
}

#[test]
fn numerically_wrong_non_hoa_candidate_selects_reference_fallback() {
    let pair = pair(
        TransportSceneDomain::DiscreteBed,
        vec![vec![0.5; FRAMES]],
        vec![TransportBedSignalBinding {
            pcm_channel_index: 0,
            target: BedSignalTarget::SemanticRole(ChannelRole::FrontLeft),
        }],
        None,
        Some(silent_reference()),
    );

    let decision = evaluate_mpegh_non_hoa_candidate(
        &pair,
        MpeghConformancePolicy::near_reference(),
    )
    .unwrap();
    assert_eq!(
        decision.playback.evidence.choice,
        MpeghPlaybackChoice::LibmpeghReference
    );
    assert!(decision.playback.reference_fallback.is_some());
}
