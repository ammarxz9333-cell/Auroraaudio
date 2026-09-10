//! Opt-in direct-eARC software proof using OpenJOC's published synthetic JOC
//! fixture. This wraps each complete JOC AU in a canonical IEC61937 E-AC-3
//! burst and exercises Aurora's real transport parser -> decoder-engine path.
//!
//! This remains synthetic software validation. It does not prove a TV/eARC
//! receiver, commercial streaming interoperability, DRM, TDM hardware or
//! loudspeaker output.

use std::{fs, path::PathBuf};

use aurora_core::{AudioFormat, SampleType};
use aurora_decoder_engine::EngineConfig;
use aurora_decoder_open::joc_access_unit::JocAccessUnitAssembler;
use aurora_direct_earc_decoder::DirectEarcDecoder;
use aurora_iec61937::{TransportCodec, DATA_TYPE_EAC3};

const FIXTURE_ENV: &str = "AURORA_OPENJOC_SYNTHETIC_FIXTURE";
const EXPECTED_BYTES: usize = 32_768;
const EXPECTED_ACCESS_UNITS: usize = 8;
const EAC3_PERIOD_BYTES: usize = 24_576;

fn format_7_1_4() -> AudioFormat {
    AudioFormat {
        sample_rate: 48_000,
        channel_count: 12,
        sample_type: SampleType::F32,
        block_size: 40,
    }
}

fn canonical_eac3_period(payload: &[u8]) -> Vec<u8> {
    assert!(!payload.is_empty());
    assert!(payload.len() <= usize::from(u16::MAX));

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
    assert!(
        burst.len() <= EAC3_PERIOD_BYTES,
        "synthetic AU does not fit the canonical E-AC-3 IEC61937 period"
    );
    burst.resize(EAC3_PERIOD_BYTES, 0);
    burst
}

fn observe_frames(
    frames: impl IntoIterator<Item = aurora_decoder_api::DecodedFrame>,
    pcm_frames: &mut usize,
) {
    for frame in frames {
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
        *pcm_frames = pcm_frames.saturating_add(frame.audio.frame_count);
    }
}

#[test]
#[ignore = "requires exact OpenJOC synthetic joc.ec3 fixture via AURORA_OPENJOC_SYNTHETIC_FIXTURE"]
fn synthetic_joc_survives_full_direct_earc_iec61937_chain() {
    let path = PathBuf::from(
        std::env::var(FIXTURE_ENV)
            .unwrap_or_else(|_| panic!("set {FIXTURE_ENV} to the verified OpenJOC joc.ec3 fixture")),
    );
    let fixture = fs::read(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    assert_eq!(fixture.len(), EXPECTED_BYTES, "unexpected fixture size");

    let mut assembler = JocAccessUnitAssembler::new();
    let mut units = assembler
        .push(&fixture)
        .expect("split published JOC fixture into access units");
    units.extend(
        assembler
            .finish()
            .expect("finish published JOC fixture access-unit split"),
    );
    assert_eq!(units.len(), EXPECTED_ACCESS_UNITS);

    let mut decoder = DirectEarcDecoder::new(EngineConfig::default());
    decoder
        .configure(format_7_1_4())
        .expect("configure direct-eARC decoder output");

    let mut bursts = 0_usize;
    let mut pcm_frames = 0_usize;
    for unit in &units {
        let period = canonical_eac3_period(unit);
        for chunk in period.chunks(997) {
            let batch = decoder
                .push_carrier(chunk, false)
                .expect("decode synthetic direct-eARC carrier chunk");
            bursts = bursts.saturating_add(batch.bursts);
            assert!(
                batch
                    .transport_codecs
                    .iter()
                    .all(|codec| *codec == TransportCodec::Eac3)
            );
            observe_frames(batch.frames, &mut pcm_frames);
        }
    }

    assert_eq!(bursts, EXPECTED_ACCESS_UNITS);
    let transport = decoder.transport_telemetry();
    assert!(transport.iec61937_locked);
    assert_eq!(transport.total_bursts, EXPECTED_ACCESS_UNITS as u64);
    assert_eq!(transport.last_burst_spacing_bytes, Some(EAC3_PERIOD_BYTES as u64));
    assert_eq!(transport.min_burst_spacing_bytes, Some(EAC3_PERIOD_BYTES as u64));
    assert_eq!(transport.max_burst_spacing_bytes, Some(EAC3_PERIOD_BYTES as u64));
    assert_eq!(transport.total_format_changes, 0);

    let joc = decoder.engine().joc_status();
    assert!(joc.codec_classified_joc);
    assert!(joc.speaker_render_active);
    assert_eq!(joc.layout_name.as_deref(), Some("7.1.4"));
    assert_eq!(joc.channel_count, Some(12));
    assert_eq!(joc.object_count, Some(1));
    assert!(joc.fallback_reason.is_none());
    let live_health = decoder.engine().joc_health();
    assert!(live_health.last_total_time_us.is_some());
    assert!(live_health.max_total_time_us.is_some());

    let final_batch = decoder
        .finish()
        .expect("finish synthetic direct-eARC JOC carrier");
    observe_frames(final_batch.frames, &mut pcm_frames);
    assert!(pcm_frames > 0, "full direct-eARC JOC chain produced no PCM");

    // EOF retires the live OpenJOC session, but Aurora must retain truthful
    // evidence of the last successful render in this decoder epoch.
    let final_joc = decoder.engine().joc_status();
    assert!(final_joc.codec_classified_joc);
    assert!(!final_joc.speaker_render_active);
    assert_eq!(final_joc.layout_name.as_deref(), Some("7.1.4"));
    assert_eq!(final_joc.channel_count, Some(12));
    assert_eq!(final_joc.object_count, Some(1));
    assert!(final_joc.fallback_reason.is_none());
    let final_health = decoder.engine().joc_health();
    assert!(!final_health.speaker_render_active);
    assert_eq!(final_health.channel_count, Some(12));
    assert_eq!(final_health.object_count, Some(1));
    assert!(final_health.last_total_time_us.is_some());
    assert!(final_health.max_total_time_us.is_some());
}