use std::{fs, path::PathBuf};

use openjoc_api::{
    OpenJocConfig, OpenJocPacket, OpenJocSession, PcmSampleFormat, RenderMode,
};
use openjoc_eac3::{parse_access_unit_bounds, AccessUnitParse};
use openjoc_scene::{SpeakerGeometry, SpeakerLayout};

const FIXTURE_ENV: &str = "AURORA_OPENJOC_SYNTHETIC_FIXTURE";
const LAYOUT_NAME: &str = "aurora-custom-16-validation";
const CHANNELS: usize = 16;
const LABELS: [&str; CHANNELS] = [
    "FL", "FR", "FC", "LFE", "Lb", "Rb", "Ls", "Rs", "Lw", "Rw", "Bc", "TFL", "TFR",
    "TML", "TMR", "TRC",
];

fn custom_sixteen_channel_layout() -> SpeakerLayout {
    SpeakerLayout::custom(
        LAYOUT_NAME,
        vec![
            SpeakerGeometry::full_range("FL", -30.0, 0.0),
            SpeakerGeometry::full_range("FR", 30.0, 0.0),
            SpeakerGeometry::full_range("FC", 0.0, 0.0),
            SpeakerGeometry::lfe("LFE", 0.0, -30.0),
            SpeakerGeometry::full_range("Lb", -150.0, 0.0),
            SpeakerGeometry::full_range("Rb", 150.0, 0.0),
            SpeakerGeometry::full_range("Ls", -90.0, 0.0),
            SpeakerGeometry::full_range("Rs", 90.0, 0.0),
            SpeakerGeometry::full_range("Lw", -60.0, 0.0),
            SpeakerGeometry::full_range("Rw", 60.0, 0.0),
            SpeakerGeometry::full_range("Bc", 180.0, 0.0),
            SpeakerGeometry::full_range("TFL", -30.0, 45.0),
            SpeakerGeometry::full_range("TFR", 30.0, 45.0),
            SpeakerGeometry::full_range("TML", -90.0, 45.0),
            SpeakerGeometry::full_range("TMR", 90.0, 45.0),
            SpeakerGeometry::full_range("TRC", 180.0, 45.0),
        ],
    )
    .expect("validation geometry must satisfy the pinned OpenJOC layout contract")
}

fn custom_session() -> OpenJocSession {
    let config = OpenJocConfig::default().with_speaker_layout(custom_sixteen_channel_layout());
    OpenJocSession::new(config).expect("create OpenJOC custom speaker session")
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
    assert_eq!(labels, LABELS);
}

#[test]
fn pinned_openjoc_accepts_explicit_sixteen_channel_geometry() {
    let session = custom_session();
    assert_output_contract(&session);
}

#[test]
#[ignore = "requires exact OpenJOC synthetic joc.ec3 fixture via AURORA_OPENJOC_SYNTHETIC_FIXTURE"]
fn pinned_openjoc_renders_synthetic_joc_to_sixteen_channel_pcm() {
    let path = PathBuf::from(
        std::env::var(FIXTURE_ENV)
            .unwrap_or_else(|_| panic!("set {FIXTURE_ENV} to the verified OpenJOC joc.ec3 fixture")),
    );
    let fixture = fs::read(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    let mut session = custom_session();
    assert_output_contract(&session);

    let mut offset = 0_usize;
    let mut rendered_frames = 0_usize;
    let mut rendered_samples = 0_usize;
    while offset < fixture.len() {
        let remaining = &fixture[offset..];
        let length = match parse_access_unit_bounds(remaining, true)
            .expect("parse complete synthetic JOC access unit")
        {
            AccessUnitParse::Complete(length) => length,
            AccessUnitParse::NeedMore => panic!("verified synthetic fixture ended with a partial access unit"),
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
            .expect("decode/render custom-layout synthetic JOC access unit");
        while let Some(frame) = session.receive_frame() {
            assert_eq!(frame.layout_name, LAYOUT_NAME);
            assert_eq!(frame.channel_count, CHANNELS);
            assert_eq!(frame.sample_format, PcmSampleFormat::F32);
            assert_eq!(frame.render_mode, RenderMode::Speaker);
            assert_eq!(frame.sample_rate, 48_000);
            assert_eq!(frame.channel_labels.iter().map(String::as_str).collect::<Vec<_>>(), LABELS);
            assert_eq!(frame.interleaved_f32.len(), frame.sample_count * CHANNELS);
            assert!(frame.interleaved_f32.iter().all(|sample| sample.is_finite()));
            rendered_frames = rendered_frames.saturating_add(1);
            rendered_samples = rendered_samples.saturating_add(frame.sample_count);
        }
        offset = offset.saturating_add(length);
    }

    session.drain().expect("drain custom-layout OpenJOC tail");
    while let Some(frame) = session.receive_frame() {
        assert_eq!(frame.layout_name, LAYOUT_NAME);
        assert_eq!(frame.channel_count, CHANNELS);
        assert_eq!(frame.interleaved_f32.len(), frame.sample_count * CHANNELS);
        assert!(frame.interleaved_f32.iter().all(|sample| sample.is_finite()));
        rendered_frames = rendered_frames.saturating_add(1);
        rendered_samples = rendered_samples.saturating_add(frame.sample_count);
    }

    assert_eq!(offset, fixture.len());
    assert!(rendered_frames > 0, "custom OpenJOC renderer emitted no PCM frames");
    assert!(rendered_samples > 0, "custom OpenJOC renderer emitted no PCM samples");
    assert_eq!(session.diagnostics().object_count, Some(1));
}
