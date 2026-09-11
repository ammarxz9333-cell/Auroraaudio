//! Source-defined wider-layout validation through Aurora's real IEC61937 parser.
//!
//! This is still synthetic software evidence: it does not emulate HDMI/eARC
//! signalling or prove a commercial streaming source. It does prove that the
//! canonical E-AC-3 carrier boundary can preserve complete JOC access units into
//! the Aurora 11.1.4 reference render/DSP/TDM path without lane inference.

use std::{fs, path::PathBuf};

use aurora_alsa_output::{encode_f32_to_s32_padded, f32_to_s32};
use aurora_dsp_basic::output::{OutputDspConfig, SpeakerPostProcessor};
use aurora_dsp_basic::output_layout::{
    OutputLayoutContract, AURORA_ELEVEN_ONE_FOUR_REFERENCE_NAME, AURORA_ROLE_FRONT_WIDE_LEFT,
    AURORA_ROLE_FRONT_WIDE_RIGHT, AURORA_ROLE_REAR_SIDE_LEFT, AURORA_ROLE_REAR_SIDE_RIGHT,
};
use aurora_iec61937::{BurstParser, CodecFilter, TransportCodec, DATA_TYPE_EAC3};
use openjoc_api::{
    OpenJocConfig, OpenJocPacket, OpenJocPcmFrame, OpenJocSession, PcmSampleFormat, RenderMode,
};
use openjoc_eac3::{parse_access_unit_bounds, AccessUnitParse};
use openjoc_scene::{SpeakerGeometry, SpeakerLayout};

const FIXTURE_ENV: &str = "AURORA_OPENJOC_SYNTHETIC_FIXTURE";
const CHANNELS: usize = 16;
const EAC3_PERIOD_BYTES: usize = 24_576;
const LABELS: [&str; CHANNELS] = [
    "FL",
    "FR",
    "FC",
    "LFE",
    "Ls",
    "Rs",
    "Lb",
    "Rb",
    AURORA_ROLE_FRONT_WIDE_LEFT,
    AURORA_ROLE_FRONT_WIDE_RIGHT,
    AURORA_ROLE_REAR_SIDE_LEFT,
    AURORA_ROLE_REAR_SIDE_RIGHT,
    "TFL",
    "TFR",
    "TBL",
    "TBR",
];

fn reference_geometry() -> SpeakerLayout {
    SpeakerLayout::custom(
        AURORA_ELEVEN_ONE_FOUR_REFERENCE_NAME,
        vec![
            SpeakerGeometry::full_range("FL", -30.0, 0.0),
            SpeakerGeometry::full_range("FR", 30.0, 0.0),
            SpeakerGeometry::full_range("FC", 0.0, 0.0),
            SpeakerGeometry::lfe("LFE", 0.0, -30.0),
            SpeakerGeometry::full_range("Ls", -90.0, 0.0),
            SpeakerGeometry::full_range("Rs", 90.0, 0.0),
            SpeakerGeometry::full_range("Lb", -150.0, 0.0),
            SpeakerGeometry::full_range("Rb", 150.0, 0.0),
            SpeakerGeometry::full_range(AURORA_ROLE_FRONT_WIDE_LEFT, -60.0, 0.0),
            SpeakerGeometry::full_range(AURORA_ROLE_FRONT_WIDE_RIGHT, 60.0, 0.0),
            SpeakerGeometry::full_range(AURORA_ROLE_REAR_SIDE_LEFT, -120.0, 0.0),
            SpeakerGeometry::full_range(AURORA_ROLE_REAR_SIDE_RIGHT, 120.0, 0.0),
            SpeakerGeometry::full_range("TFL", -30.0, 45.0),
            SpeakerGeometry::full_range("TFR", 30.0, 45.0),
            SpeakerGeometry::full_range("TBL", -135.0, 45.0),
            SpeakerGeometry::full_range("TBR", 135.0, 45.0),
        ],
    )
    .expect("construct Aurora 11.1.4 reference OpenJOC geometry")
}

fn split_access_units(stream: &[u8]) -> Vec<Vec<u8>> {
    let mut offset = 0_usize;
    let mut units = Vec::new();
    while offset < stream.len() {
        let remaining = &stream[offset..];
        let length = match parse_access_unit_bounds(remaining, true)
            .expect("parse synthetic JOC access-unit boundary")
        {
            AccessUnitParse::Complete(length) => length,
            AccessUnitParse::NeedMore => {
                panic!("synthetic fixture ended with a partial access unit")
            }
        };
        assert!(length > 0 && length <= remaining.len());
        units.push(remaining[..length].to_vec());
        offset = offset.saturating_add(length);
    }
    assert_eq!(offset, stream.len());
    units
}

fn canonical_eac3_period(payload: &[u8]) -> Vec<u8> {
    assert!(!payload.is_empty());
    assert!(payload.len() <= 24_560);
    let mut burst = Vec::with_capacity(EAC3_PERIOD_BYTES);
    burst.extend_from_slice(&[0x72, 0xF8, 0x1F, 0x4E]);
    burst.extend_from_slice(&u16::from(DATA_TYPE_EAC3).to_le_bytes());
    burst.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    let mut wire = payload.to_vec();
    if wire.len() % 2 != 0 {
        wire.push(0);
    }
    for word in wire.chunks_exact_mut(2) {
        word.swap(0, 1);
    }
    burst.extend_from_slice(&wire);
    assert!(burst.len() <= EAC3_PERIOD_BYTES);
    burst.resize(EAC3_PERIOD_BYTES, 0);
    burst
}

fn consume_frame(mut frame: OpenJocPcmFrame, post: &mut SpeakerPostProcessor) -> usize {
    assert_eq!(frame.layout_name, AURORA_ELEVEN_ONE_FOUR_REFERENCE_NAME);
    assert_eq!(frame.channel_count, CHANNELS);
    assert_eq!(frame.sample_format, PcmSampleFormat::F32);
    assert_eq!(frame.sample_rate, 48_000);
    assert_eq!(frame.render_mode, RenderMode::Speaker);
    let labels = frame
        .channel_labels
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert_eq!(labels.as_slice(), LABELS.as_slice());
    assert!(frame.sample_count > 0);
    assert_eq!(frame.interleaved_f32.len(), frame.sample_count * CHANNELS);
    assert!(frame.interleaved_f32.iter().all(|sample| sample.is_finite()));
    post.process_block(&mut frame.interleaved_f32)
        .expect("process Aurora 11.1.4 reference PCM");
    let staged = encode_f32_to_s32_padded(&frame.interleaved_f32, CHANNELS, CHANNELS)
        .expect("stage Aurora 11.1.4 reference PCM into TDM16");
    assert_eq!(staged.len(), frame.interleaved_f32.len());
    for (pcm, s32) in frame
        .interleaved_f32
        .iter()
        .copied()
        .zip(staged.iter().copied())
    {
        assert_eq!(s32, f32_to_s32(pcm).expect("processed PCM must stay finite"));
    }
    frame.sample_count
}

#[test]
#[ignore = "requires exact OpenJOC synthetic joc.ec3 fixture via AURORA_OPENJOC_SYNTHETIC_FIXTURE"]
fn canonical_earc_carrier_reaches_aurora_eleven_one_four_reference_tdm16() {
    let path = PathBuf::from(
        std::env::var(FIXTURE_ENV)
            .unwrap_or_else(|_| panic!("set {FIXTURE_ENV} to the verified OpenJOC joc.ec3 fixture")),
    );
    let fixture = fs::read(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    let units = split_access_units(&fixture);
    assert!(!units.is_empty());

    let mut carrier = Vec::with_capacity(units.len() * EAC3_PERIOD_BYTES);
    for unit in &units {
        carrier.extend_from_slice(&canonical_eac3_period(unit));
    }

    let config = OpenJocConfig::default().with_speaker_layout(reference_geometry());
    let mut session = OpenJocSession::new(config).expect("create reference OpenJOC session");
    let mut post = SpeakerPostProcessor::new_for_layout(
        OutputDspConfig::default(),
        OutputLayoutContract::aurora_eleven_one_four_reference()
            .expect("construct Aurora 11.1.4 reference output contract"),
    )
    .expect("construct Aurora 11.1.4 reference DSP");
    let mut parser = BurstParser::new(CodecFilter::Eac3);
    let mut observed_units = 0_usize;
    let mut rendered_samples = 0_usize;

    for chunk in carrier.chunks(997) {
        for observation in parser.push(chunk) {
            assert_eq!(observation.burst.codec, TransportCodec::Eac3);
            assert_eq!(observation.burst.data_type, DATA_TYPE_EAC3);
            assert!(observation.format_change.is_none());
            assert_eq!(
                observation.carrier_offset_bytes,
                (observed_units * EAC3_PERIOD_BYTES) as u64
            );
            assert_eq!(
                observation.burst.payload.as_slice(),
                units[observed_units].as_slice()
            );
            session
                .push_packet(OpenJocPacket {
                    data: &observation.burst.payload,
                    pts_samples: None,
                    discontinuity: false,
                    preroll: false,
                })
                .expect("render transport-authenticated JOC access unit");
            while let Some(frame) = session.receive_frame() {
                rendered_samples =
                    rendered_samples.saturating_add(consume_frame(frame, &mut post));
            }
            observed_units = observed_units.saturating_add(1);
        }
    }

    parser.finish().expect("finish canonical IEC61937 carrier");
    assert_eq!(observed_units, units.len());
    assert_eq!(parser.malformed_headers(), 0);

    session.drain().expect("drain reference OpenJOC session");
    while let Some(frame) = session.receive_frame() {
        rendered_samples = rendered_samples.saturating_add(consume_frame(frame, &mut post));
    }
    assert!(rendered_samples > 0);
    assert_eq!(session.diagnostics().object_count, Some(1));
}
