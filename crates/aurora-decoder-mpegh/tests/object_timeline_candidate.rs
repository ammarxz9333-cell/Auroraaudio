use aurora_core::AudioBlock;
use aurora_decoder_api::DecodedFrame;
use aurora_decoder_mpegh::{
    render_exact_mpegh_object_plane, MpeghPairedEvidence, MpeghSpeaker, MpeghSpeakerLayout,
};
use aurora_spatial_ir_v2::{
    CoordinateSpace, ObjectSignalBinding, SpatialDecodedFrame, SpatialDomain,
    SpatialFrameMetadata, SpatialObjectUpdate, SpatialPosition, SpatialRenderingProperties,
};
use aurora_spatial_transport_v2::{
    HoaGroupBinding, HoaSignalBinding, SpatialTransportFrame, TransportSceneDomain,
};

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

fn pair() -> MpeghPairedEvidence {
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
                    object_updates: vec![update(0, 30.0), update(2, -30.0)],
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
            ],
        },
    }
}

#[test]
fn object_update_moves_on_the_exact_metadata_sample() {
    let rendered = render_exact_mpegh_object_plane(&pair()).unwrap();
    assert!(rendered.channels[0][0].abs() > rendered.channels[1][0].abs());
    assert!(rendered.channels[0][1].abs() > rendered.channels[1][1].abs());
    assert!(rendered.channels[1][2].abs() > rendered.channels[0][2].abs());
    assert!(rendered.channels[1][3].abs() > rendered.channels[0][3].abs());
}
