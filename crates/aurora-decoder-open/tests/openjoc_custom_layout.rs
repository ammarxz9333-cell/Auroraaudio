use std::{fs, path::PathBuf};

use aurora_alsa_output::{encode_f32_to_s32_padded, f32_to_s32};
use aurora_dsp_basic::output::{OutputDspConfig, SpeakerPostProcessor};
use aurora_dsp_basic::output_layout::{
    OutputLayoutContract, AURORA_ELEVEN_ONE_FOUR_REFERENCE_NAME, AURORA_ROLE_FRONT_WIDE_LEFT,
    AURORA_ROLE_FRONT_WIDE_RIGHT, AURORA_ROLE_REAR_SIDE_LEFT, AURORA_ROLE_REAR_SIDE_RIGHT,
};
use openjoc_api::{
    OpenJocConfig, OpenJocPacket, OpenJocPcmFrame, OpenJocSession, PcmSampleFormat, RenderMode,
};
use openjoc_eac3::{parse_access_unit_bounds, AccessUnitParse};
use openjoc_scene::{SpeakerGeometry, SpeakerLayout};

const FIXTURE_ENV: &str = "AURORA_OPENJOC_SYNTHETIC_FIXTURE";
const LAYOUT_NAME: &str = AURORA_ELEVEN_ONE_FOUR_REFERENCE_NAME;
const CHANNELS: usize = 16;
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

fn aurora_eleven_one_four_geometry() -> SpeakerLayout {
    SpeakerLayout::custom(
        LAYOUT_NAME,
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
    .expect("Aurora 11.1.4 reference geometry must satisfy the pinned OpenJOC layout contract")
}

fn output_layout_contract() -> OutputLayoutContract {
    OutputLayoutContract::aurora_eleven_one_four_reference()
        .expect("Aurora 11.1.4 reference output contract must be valid")
}

fn custom_session() -> OpenJocSession {
    let config = OpenJocConfig::default().with_speaker_layout(aurora_eleven_one_four_geometry());
    OpenJocSession::new(config).expect("create OpenJOC Aurora 11.1.4 reference session")
}

fn assert_output_contract(session: &OpenJocSession) {
    let info = session.output_info();
    let labels = info
        .channel_labels
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert_eq!(info.layout_name, LAYOUT_NAME);
    assert_eq!(info.channel_count, CHANNELS);
    assert_eq!(info.sample_format, PcmSampleFormat::F32);
    assert_eq!(info.render_mode, RenderMode::Speaker);
    assert_eq!(labels.as_slice(), LABELS.as_slice());
}

fn consume_rendered_frame(
    mut frame: OpenJocPcmFrame,
    post: &mut SpeakerPostProcessor,
) -> usize {
    assert_eq!(frame.layout_name, LAYOUT_NAME);
    assert_eq!(frame.channel_count, CHANNELS);
    assert_eq!(frame.sample_format, PcmSampleFormat::F32);
    assert_eq!(frame.render_mode, RenderMode::Speaker);
    assert_eq!(frame.sample_rate, 48_000);
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
        .expect("dynamic Aurora DSP must accept the 16-channel rendered block");
    assert!(frame.interleaved_f32.iter().all(|sample| sample.is_finite()));

    let encoded = encode_f32_to_s32_padded(&frame.interleaved_f32, CHANNELS, CHANNELS)
        .expect("16-channel Aurora PCM must enter TDM staging without padding or truncation");
    assert_eq!(encoded.len(), frame.interleaved_f32.len());
    for (source, staged) in frame
        .interleaved_f32
        .iter()
        .copied()
        .zip(encoded.iter().copied())
    {
        assert_eq!(staged, f32_to_s32(source).expect("DSP output must remain finite"));
    }
    frame.sample_count
}

#[test]
fn pinned_openjoc_accepts_aurora_reference_eleven_one_four_geometry() {
    let session = custom_session();
    assert_output_contract(&session);
}

#[test]
#[ignore = "requires exact OpenJOC synthetic joc.ec3 fixture via AURORA_OPENJOC_SYNTHETIC_FIXTURE"]
fn pinned_openjoc_renders_synthetic_joc_through_aurora_sixteen_channel_output() {
    let path = PathBuf::from(
        std::env::var(FIXTURE_ENV)
            .unwrap_or_else(|_| panic!("set {FIXTURE_ENV} to the verified OpenJOC joc.ec3 fixture")),
    );
    let fixture = fs::read(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    let mut session = custom_session();
    assert_output_contract(&session);
    let mut post = SpeakerPostProcessor::new_for_layout(
        OutputDspConfig::default(),
        output_layout_contract(),
    )
    .expect("construct dynamic Aurora 11.1.4 reference output DSP");

    let mut offset = 0_usize;
    let mut rendered_frames = 0_usize;
    let mut rendered_samples = 0_usize;
    while offset < fixture.len() {
        let remaining = &fixture[offset..];
        let length = match parse_access_unit_bounds(remaining, true)
            .expect("parse complete synthetic JOC access unit")
        {
            AccessUnitParse::Complete(length) => length,
            AccessUnitParse::NeedMore => {
                panic!("verified synthetic fixture ended with a partial access unit")
            }
        };
        assert!(length > 0 && length <= remaining.len());
        let unit = &remaining[..length];
        session
            .push_packet(OpenJocPacket {
                data: unit,
                pts_samples: None,
                discontinuity: false,
                preroll: false,
            })
            .expect("decode/render Aurora 11.1.4 reference synthetic JOC access unit");
        while let Some(frame) = session.receive_frame() {
            rendered_samples = rendered_samples
                .saturating_add(consume_rendered_frame(frame, &mut post));
            rendered_frames = rendered_frames.saturating_add(1);
        }
        offset = offset.saturating_add(length);
    }

    session.drain().expect("drain Aurora 11.1.4 reference OpenJOC tail");
    while let Some(frame) = session.receive_frame() {
        rendered_samples =
            rendered_samples.saturating_add(consume_rendered_frame(frame, &mut post));
        rendered_frames = rendered_frames.saturating_add(1);
    }

    assert_eq!(offset, fixture.len());
    assert!(rendered_frames > 0, "custom OpenJOC renderer emitted no PCM frames");
    assert!(rendered_samples > 0, "custom OpenJOC renderer emitted no PCM samples");
    assert_eq!(session.diagnostics().object_count, Some(1));
}
