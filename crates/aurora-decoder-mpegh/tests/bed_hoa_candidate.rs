use aurora_core::{AudioBlock, ChannelRole};
use aurora_decoder_api::DecodedFrame;
use aurora_decoder_mpegh::{
    render_mpegh_hoa_candidate, MpeghHoaCandidateError, MpeghPairedEvidence, MpeghSpeaker,
    MpeghSpeakerLayout,
};
use aurora_hoa_ir::{HoaCoefficientBinding, HoaCoefficientConvention, HoaCoefficientFrame};
use aurora_spatial_ir_v2::{SpatialDecodedFrame, SpatialDomain, SpatialFrameMetadata};
use aurora_spatial_transport_v2::{
    BedSignalTarget, ExplicitSpeakerGeometry, HoaGroupBinding, HoaSignalBinding,
    SpatialTransportFrame, SpeakerElevation, TransportBedSignalBinding, TransportSceneDomain,
};

const FRAMES: usize = 4;

fn zero_order_coefficients(value: f32) -> HoaCoefficientFrame {
    HoaCoefficientFrame {
        audio: AudioBlock {
            channels: vec![vec![value; FRAMES]],
            frame_count: FRAMES,
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

fn bed_hoa_pair(
    bed_channels: Vec<Vec<f32>>,
    bed_targets: Vec<BedSignalTarget>,
    coefficient_value: f32,
) -> MpeghPairedEvidence {
    assert_eq!(bed_channels.len(), bed_targets.len());
    let bed_count = bed_channels.len();
    let mut channels = bed_channels;
    channels.push(vec![0.0; FRAMES]);

    let bed_signals = bed_targets
        .into_iter()
        .enumerate()
        .map(|(pcm_channel_index, target)| TransportBedSignalBinding {
            pcm_channel_index,
            target,
        })
        .collect::<Vec<_>>();

    MpeghPairedEvidence {
        scene: SpatialTransportFrame {
            frame: SpatialDecodedFrame {
                decoded: DecodedFrame {
                    audio: AudioBlock {
                        channels,
                        frame_count: FRAMES,
                        presentation_time_seconds: 0.0,
                        discontinuity: false,
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
        },
        hoa_coefficients: Some(zero_order_coefficients(coefficient_value)),
        reference: None,
        reference_layout: reference_layout(),
    }
}

#[test]
fn bed_and_hoa_mix_adds_direct_bed_to_hoa_output() {
    let pair = bed_hoa_pair(
        vec![vec![0.25; FRAMES]],
        vec![BedSignalTarget::SemanticRole(ChannelRole::FrontLeft)],
        0.5,
    );

    let rendered = render_mpegh_hoa_candidate(&pair, 0.01).unwrap();
    assert_eq!(rendered.channels.len(), 2);
    assert!(rendered.channels[0].iter().all(|sample| *sample > 0.25));
    assert!(rendered.channels[1].iter().all(|sample| sample.abs() < 1.0e-7));
}

#[test]
fn lfe_bed_is_passthrough_and_hoa_never_enters_lfe() {
    let bed = vec![0.125, -0.25, 0.5, -0.75];
    let pair = bed_hoa_pair(
        vec![bed.clone()],
        vec![BedSignalTarget::SemanticRole(
            ChannelRole::LowFrequencyEffects,
        )],
        0.5,
    );

    let rendered = render_mpegh_hoa_candidate(&pair, 0.01).unwrap();
    assert_eq!(rendered.channels[1], bed);
    assert!(rendered.channels[0].iter().any(|sample| sample.abs() > 1.0e-6));
}

#[test]
fn duplicate_bed_destination_is_rejected() {
    let pair = bed_hoa_pair(
        vec![vec![0.1; FRAMES], vec![0.2; FRAMES]],
        vec![
            BedSignalTarget::SemanticRole(ChannelRole::FrontLeft),
            BedSignalTarget::SemanticRole(ChannelRole::FrontLeft),
        ],
        0.0,
    );

    assert!(matches!(
        render_mpegh_hoa_candidate(&pair, 0.01),
        Err(MpeghHoaCandidateError::DuplicateBedDestination { destination: 0 })
    ));
}

#[test]
fn unmatched_explicit_bed_geometry_is_rejected() {
    let pair = bed_hoa_pair(
        vec![vec![0.1; FRAMES]],
        vec![BedSignalTarget::ExplicitGeometry(ExplicitSpeakerGeometry {
            azimuth_degrees: 60.0,
            elevation: SpeakerElevation::Degrees(0.0),
            is_lfe: false,
        })],
        0.0,
    );

    assert!(matches!(
        render_mpegh_hoa_candidate(&pair, 0.01),
        Err(MpeghHoaCandidateError::MissingBedGeometry { .. })
    ));
}
