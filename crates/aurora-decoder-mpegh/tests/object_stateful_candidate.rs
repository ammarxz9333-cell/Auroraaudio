use aurora_core::AudioBlock;
use aurora_decoder_api::DecodedFrame;
use aurora_decoder_mpegh::{
    render_stateful_mpegh_object_plane, MpeghObjectStateCache, MpeghPairedEvidence,
    MpeghSpeaker, MpeghSpeakerLayout, MpeghStatefulObjectError,
};
use aurora_spatial_ir_v2::{
    CoordinateSpace, ObjectSignalBinding, SpatialDecodedFrame, SpatialDomain,
    SpatialFrameMetadata, SpatialObjectUpdate, SpatialPosition, SpatialRenderingProperties,
};
use aurora_spatial_transport_v2::{SpatialTransportFrame, TransportSceneDomain};

const FRAMES: usize = 4;

fn update(offset: u32, azimuth_degrees: f32) -> SpatialObjectUpdate {
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
        metadata_sample_offset: offset,
        ramp_duration_samples: 0,
        priority: Some(1.0),
        rendering: SpatialRenderingProperties::default(),
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

fn pair(updates: Vec<SpatialObjectUpdate>, discontinuity: bool) -> MpeghPairedEvidence {
    MpeghPairedEvidence {
        scene: SpatialTransportFrame {
            frame: SpatialDecodedFrame {
                decoded: DecodedFrame {
                    audio: AudioBlock {
                        channels: vec![vec![1.0; FRAMES]],
                        frame_count: FRAMES,
                        presentation_time_seconds: 0.0,
                        discontinuity,
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
                    object_updates: updates,
                },
            },
            domain: TransportSceneDomain::ObjectSignals,
            bed_signals: Vec::new(),
            hoa_signals: Vec::new(),
            hoa_groups: Vec::new(),
            codec_metadata: Vec::new(),
        },
        hoa_coefficients: None,
        reference: None,
        reference_layout: reference_layout(),
    }
}

#[test]
fn state_carries_when_next_access_unit_omits_sample_zero_update() {
    let mut cache = MpeghObjectStateCache::new();
    let first = render_stateful_mpegh_object_plane(
        &pair(vec![update(0, 30.0)], true),
        &mut cache,
    )
    .unwrap();
    assert_eq!(cache.len(), 1);
    assert!(first.channels[0][0].abs() > first.channels[1][0].abs());

    let second = render_stateful_mpegh_object_plane(&pair(Vec::new(), false), &mut cache).unwrap();
    assert!(second.channels[0][0].abs() > second.channels[1][0].abs());
}

#[test]
fn frame_boundary_update_becomes_next_access_unit_initial_state() {
    let mut cache = MpeghObjectStateCache::new();
    let first = render_stateful_mpegh_object_plane(
        &pair(vec![update(0, 30.0), update(FRAMES as u32, -30.0)], true),
        &mut cache,
    )
    .unwrap();
    assert!(first.channels[0]
        .iter()
        .zip(&first.channels[1])
        .all(|(left, right)| left.abs() > right.abs()));

    let second = render_stateful_mpegh_object_plane(&pair(Vec::new(), false), &mut cache).unwrap();
    assert!(second.channels[1]
        .iter()
        .zip(&second.channels[0])
        .all(|(right, left)| right.abs() > left.abs()));
}

#[test]
fn discontinuity_clears_carried_object_state() {
    let mut cache = MpeghObjectStateCache::new();
    render_stateful_mpegh_object_plane(&pair(vec![update(0, 30.0)], true), &mut cache).unwrap();
    assert!(!cache.is_empty());

    assert!(matches!(
        render_stateful_mpegh_object_plane(&pair(Vec::new(), true), &mut cache),
        Err(MpeghStatefulObjectError::MissingCarriedObjectState { .. })
    ));
    assert!(cache.is_empty());
}
