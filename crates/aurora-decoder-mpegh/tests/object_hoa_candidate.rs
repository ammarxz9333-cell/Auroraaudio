use aurora_core::AudioBlock;
use aurora_decoder_api::DecodedFrame;
use aurora_decoder_mpegh::{
    render_static_mpegh_object_plane, MpeghObjectCandidateError, MpeghPairedEvidence,
    MpeghSpeaker, MpeghSpeakerLayout,
};
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

fn pair(updates: Vec<SpatialObjectUpdate>) -> MpeghPairedEvidence {
    MpeghPairedEvidence {
        scene: SpatialTransportFrame {
            frame: SpatialDecodedFrame {
                decoded: DecodedFrame {
                    audio: AudioBlock {
                        channels: vec![vec![1.0; FRAMES], vec![0.0; FRAMES]],
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
                    object_updates: updates,
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
        hoa_coefficients: None,
        reference: None,
        reference_layout: layout(),
    }
}

fn static_update(spread: f32) -> SpatialObjectUpdate {
    SpatialObjectUpdate {
        object_id: "o0".into(),
        active: true,
        coordinate_space: CoordinateSpace::SphericalDegrees,
        position: SpatialPosition::Spherical {
            azimuth_degrees: 30.0,
            elevation_degrees: 0.0,
            distance: 1.0,
        },
        gain_db: 0.0,
        spread,
        metadata_sample_offset: 0,
        ramp_duration_samples: 0,
        priority: Some(1.0),
        rendering: SpatialRenderingProperties::default(),
    }
}

#[test]
fn static_point_object_renders_in_reference_speaker_order() {
    let rendered = render_static_mpegh_object_plane(&pair(vec![static_update(0.0)])).unwrap();
    assert_eq!(rendered.channels.len(), 6);
    assert_eq!(rendered.frame_count, FRAMES);
    let fl_energy: f32 = rendered.channels[0].iter().map(|sample| sample * sample).sum();
    let other_energy: f32 = rendered.channels[1..]
        .iter()
        .flat_map(|channel| channel.iter())
        .map(|sample| sample * sample)
        .sum();
    assert!(fl_energy > other_energy);
}

#[test]
fn dynamic_object_metadata_fails_closed() {
    let mut second = static_update(0.0);
    second.metadata_sample_offset = 1;
    let error = render_static_mpegh_object_plane(&pair(vec![static_update(0.0), second]))
        .unwrap_err();
    assert!(matches!(
        error,
        MpeghObjectCandidateError::DynamicObjectMetadata { .. }
    ));
}

#[test]
fn spread_object_fails_closed() {
    let error = render_static_mpegh_object_plane(&pair(vec![static_update(0.25)])).unwrap_err();
    assert!(matches!(
        error,
        MpeghObjectCandidateError::RichObjectMetadata { .. }
    ));
}
