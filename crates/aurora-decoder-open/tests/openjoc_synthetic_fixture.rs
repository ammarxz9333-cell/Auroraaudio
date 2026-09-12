//! Opt-in end-to-end software proof using OpenJOC's published synthetic JOC fixture.
//!
//! The fixture is intentionally not vendored into Aurora. CI downloads the exact
//! 32 KiB `joc.ec3` blob from OpenJOC at the same revision used by this crate,
//! verifies its Git blob identity, and points this test at the local file.
//! Passing this test proves Aurora's synthetic JOC software path only; it does
//! not prove Netflix, TV eARC, HDCP/DRM, receiver hardware or commercial Atmos
//! interoperability.

use std::{fs, path::PathBuf};

use aurora_core::{AudioFormat, SampleType, StandardLayout};
use aurora_decoder_api::{DecodedFrame, Decoder};
use aurora_decoder_open::{OpenCodecKind, OpenDecoderConfig, UniversalOpenDecoder};

const FIXTURE_ENV: &str = "AURORA_OPENJOC_SYNTHETIC_FIXTURE";
const EXPECTED_BYTES: usize = 32_768;
// Exact PCM frame count produced by the pinned OpenJOC revision for this exact
// verified fixture in 7.1.4 Speaker mode, including the renderer tail. The
// streaming-parity integration test independently proves Aurora matches the
// same direct OpenJOC session instead of inferring duration from EC-3 substream
// metadata.
const EXPECTED_PINNED_OPENJOC_PCM_FRAMES: usize = 1_568;

fn format_7_1_4() -> AudioFormat {
    AudioFormat {
        sample_rate: 48_000,
        channel_count: 12,
        sample_type: SampleType::F32,
        block_size: 40,
    }
}

fn observe_frame(frame: DecodedFrame, output_frames: &mut usize, full_blocks: &mut usize) {
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
    if frame.audio.frame_count == 40 {
        *full_blocks = full_blocks.saturating_add(1);
    }
    *output_frames = output_frames.saturating_add(frame.audio.frame_count);
}

fn drain_ready(
    decoder: &mut UniversalOpenDecoder,
    output_frames: &mut usize,
    full_blocks: &mut usize,
) {
    while let Some(frame) = decoder
        .decode_chunk(&[])
        .expect("drain ready synthetic JOC PCM")
    {
        observe_frame(frame, output_frames, full_blocks);
    }
}

#[test]
#[ignore = "requires exact OpenJOC synthetic joc.ec3 fixture via AURORA_OPENJOC_SYNTHETIC_FIXTURE"]
fn synthetic_openjoc_fixture_renders_aurora_7_1_4() {
    let path = PathBuf::from(
        std::env::var(FIXTURE_ENV)
            .unwrap_or_else(|_| panic!("set {FIXTURE_ENV} to the verified OpenJOC joc.ec3 fixture")),
    );
    let fixture = fs::read(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    assert_eq!(fixture.len(), EXPECTED_BYTES, "unexpected fixture size");

    let config = OpenDecoderConfig {
        codec_hint: Some(OpenCodecKind::Eac3),
        joc_stereo_reference: false,
        ..OpenDecoderConfig::default()
    }
    .with_standard_joc_layout(StandardLayout::SevenOneFour)
    .expect("typed Aurora 7.1.4 layout must map to an admitted OpenJOC preset");
    let mut decoder = UniversalOpenDecoder::new(config);
    decoder
        .configure(format_7_1_4())
        .expect("configure Aurora 7.1.4 decoder output");

    let mut output_frames = 0_usize;
    let mut full_blocks = 0_usize;
    for chunk in fixture.chunks(97) {
        if let Some(frame) = decoder
            .decode_chunk(chunk)
            .expect("decode synthetic JOC stream chunk")
        {
            observe_frame(frame, &mut output_frames, &mut full_blocks);
        }
        drain_ready(&mut decoder, &mut output_frames, &mut full_blocks);
    }

    assert_eq!(decoder.detected_codec(), Some(OpenCodecKind::Eac3Joc));
    assert!(decoder.last_joc_error().is_none());
    let info = decoder
        .joc_render_info()
        .cloned()
        .expect("positive JOC admission must create an active OpenJOC renderer");
    assert_eq!(info.layout_name, "7.1.4");
    assert_eq!(info.channel_count, 12);
    assert!(info.latency_samples > 0);
    assert_eq!(info.object_count, Some(1));
    assert!(info.max_total_time_us.is_some());

    decoder
        .flush_packets()
        .expect("finalize synthetic JOC stream without dropping renderer tail");
    drain_ready(&mut decoder, &mut output_frames, &mut full_blocks);

    assert!(full_blocks > 0, "Aurora never emitted its 40-frame realtime block");
    assert_eq!(
        output_frames, EXPECTED_PINNED_OPENJOC_PCM_FRAMES,
        "Aurora must preserve the exact PCM timeline produced by the pinned direct OpenJOC renderer"
    );
    assert_eq!(decoder.detected_codec(), Some(OpenCodecKind::Eac3Joc));
    assert!(decoder.joc_render_info().is_none());
    let historical = decoder
        .last_joc_render_info()
        .expect("successful JOC render evidence must survive renderer retirement");
    assert_eq!(historical.layout_name, "7.1.4");
    assert_eq!(historical.channel_count, 12);
    assert_eq!(historical.object_count, Some(1));
    assert!(historical.last_total_time_us.is_some());
    assert!(historical.max_total_time_us.is_some());
}
