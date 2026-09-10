//! Opt-in end-to-end software proof using OpenJOC's published synthetic JOC fixture.
//!
//! The fixture is intentionally not vendored into Aurora. CI downloads the exact
//! 32 KiB `joc.ec3` blob from OpenJOC 0.17.0 at the same revision used by this
//! crate, verifies its Git blob identity, and points this test at the local file.
//! Passing this test proves Aurora's synthetic JOC software path only; it does
//! not prove Netflix, TV eARC, HDCP/DRM, receiver hardware or commercial Atmos
//! interoperability.

use std::{fs, path::PathBuf};

use aurora_core::{AudioFormat, SampleType};
use aurora_decoder_open::{
    joc_access_unit::JocAccessUnitAssembler,
    joc_probe::{JocAdmission, JocAdmissionProbe},
    openjoc_native::OpenJocNativeRenderer,
};

const FIXTURE_ENV: &str = "AURORA_OPENJOC_SYNTHETIC_FIXTURE";
const EXPECTED_BYTES: usize = 32_768;
const EXPECTED_ACCESS_UNITS: usize = 8;

fn format_7_1_4() -> AudioFormat {
    AudioFormat {
        sample_rate: 48_000,
        channel_count: 12,
        sample_type: SampleType::F32,
        block_size: 40,
    }
}

#[test]
#[ignore = "requires exact OpenJOC 0.17 synthetic joc.ec3 fixture via AURORA_OPENJOC_SYNTHETIC_FIXTURE"]
fn synthetic_openjoc_fixture_renders_aurora_7_1_4() {
    let path = PathBuf::from(
        std::env::var(FIXTURE_ENV)
            .unwrap_or_else(|_| panic!("set {FIXTURE_ENV} to the verified OpenJOC joc.ec3 fixture")),
    );
    let fixture = fs::read(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    assert_eq!(fixture.len(), EXPECTED_BYTES, "unexpected fixture size");

    // Exercise Aurora's streaming AU boundary logic with deliberately awkward
    // read boundaries rather than handing the full elementary stream to OpenJOC.
    let mut assembler = JocAccessUnitAssembler::new();
    let mut units = Vec::new();
    for chunk in fixture.chunks(97) {
        units.extend(assembler.push(chunk).expect("assemble synthetic JOC chunk"));
    }
    units.extend(assembler.finish().expect("finish synthetic JOC assembly"));
    assert_eq!(units.len(), EXPECTED_ACCESS_UNITS);

    let mut admission = JocAdmissionProbe::new();
    let mut renderer = OpenJocNativeRenderer::new(format_7_1_4(), Some("7.1.4"))
        .expect("create OpenJOC 7.1.4 renderer");
    let mut output_frames = 0_usize;
    let mut output_samples = 0_usize;

    for unit in &units {
        assert_eq!(
            admission.inspect(unit),
            JocAdmission::Validated,
            "every published synthetic AU must pass Aurora's authoritative OpenJOC admission"
        );
        renderer
            .push_access_unit(unit)
            .expect("render synthetic JOC access unit");
        while let Some(frame) = renderer.take_block() {
            assert_eq!(frame.audio.channels.len(), 12);
            assert_eq!(frame.audio.frame_count, 40);
            assert!(frame.audio.channels.iter().all(|channel| channel.len() == 40));
            assert!(
                frame
                    .audio
                    .channels
                    .iter()
                    .flatten()
                    .all(|sample| sample.is_finite())
            );
            output_frames += frame.audio.frame_count;
            output_samples += frame.audio.frame_count.saturating_mul(12);
        }
    }

    for frame in renderer.drain().expect("drain synthetic JOC renderer") {
        assert_eq!(frame.audio.channels.len(), 12);
        assert!(frame.audio.frame_count > 0 && frame.audio.frame_count <= 40);
        assert!(
            frame
                .audio
                .channels
                .iter()
                .all(|channel| channel.len() == frame.audio.frame_count)
        );
        assert!(
            frame
                .audio
                .channels
                .iter()
                .flatten()
                .all(|sample| sample.is_finite())
        );
        output_frames += frame.audio.frame_count;
        output_samples += frame.audio.frame_count.saturating_mul(12);
    }

    assert!(output_frames > 0, "synthetic JOC path produced no PCM");
    assert_eq!(output_samples, output_frames.saturating_mul(12));

    let info = renderer.render_info();
    assert_eq!(info.layout_name, "7.1.4");
    assert_eq!(info.channel_count, 12);
    assert!(info.latency_samples > 0);
    assert_eq!(info.object_count, Some(1));
    assert!(info.max_total_us.is_some());
}
