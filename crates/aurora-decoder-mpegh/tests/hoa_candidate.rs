use aurora_core::AudioBlock;
use aurora_decoder_api::DecodedFrame;
use aurora_decoder_mpegh::{
    render_pure_mpegh_hoa_candidate, MpeghHoaCandidateError, MpeghPairedEvidence, MpeghSpeaker,
    MpeghSpeakerLayout,
};
use aurora_hoa_ir::{
    HoaCoefficientBinding, HoaCoefficientConvention, HoaCoefficientFrame,
};
use aurora_spatial_ir_v2::{SpatialDecodedFrame, SpatialDomain, SpatialFrameMetadata};
use aurora_spatial_transport_v2::{
    HoaGroupBinding, HoaSignalBinding, SpatialTransportFrame, TransportSceneDomain,
};

fn pure_hoa_pair() -> MpeghPairedEvidence {
    let transport_audio = AudioBlock {
        channels: vec![vec![0.0; 4]],
        frame_count: 4,
        presentation_time_seconds: 2.0,
        discontinuity: true,
    };
    let scene = SpatialTransportFrame {
        frame: SpatialDecodedFrame {
            decoded: DecodedFrame {
                audio: transport_audio,
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
    let coefficients = HoaCoefficientFrame {
        audio: AudioBlock {
            channels: vec![vec![1.0, 0.5, -0.5, -1.0]],
            frame_count: 4,
            presentation_time_seconds: 0.0,
            discontinuity: false,
        },
        order: 0,
        convention: HoaCoefficientConvention::AcnN3d,
        coefficients: vec![HoaCoefficientBinding {
            pcm_channel_index: 0,
            coefficient_index: 0,
        }],
    };
    MpeghPairedEvidence {
        scene,
        hoa_coefficients: Some(coefficients),
        reference: None,
        reference_layout: MpeghSpeakerLayout {
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
                    is_lfe: true,
                    azimuth_degrees: 0,
                    elevation_degrees: 0,
                },
            ],
        },
    }
}

#[test]
fn pure_hoa_candidate_keeps_lfe_silent_and_scene_timing() {
    let pair = pure_hoa_pair();
    let rendered = render_pure_mpegh_hoa_candidate(&pair, 1.0e-6).unwrap();
    assert_eq!(rendered.channels.len(), 3);
    assert_eq!(rendered.channels[2], vec![0.0; 4]);
    assert_eq!(rendered.presentation_time_seconds, 2.0);
    assert!(rendered.discontinuity);
    assert!(rendered.channels[0].iter().any(|sample| sample.abs() > 0.0));
    assert!(rendered.channels[1].iter().any(|sample| sample.abs() > 0.0));
}

#[test]
fn mixed_scene_is_rejected_before_rendering() {
    let mut pair = pure_hoa_pair();
    pair.scene.domain = TransportSceneDomain::BedAndHoa;
    assert!(matches!(
        render_pure_mpegh_hoa_candidate(&pair, 1.0e-6),
        Err(MpeghHoaCandidateError::MixedSceneUnsupported { .. })
    ));
}
