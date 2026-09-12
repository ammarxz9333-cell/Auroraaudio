//! Opt-in production-runtime proof for the pinned OpenJOC synthetic JOC fixture.
//!
//! The fixture is framed as canonical IEC61937 E-AC-3, embedded into the high
//! half of native two-slot S32_LE capture words, and fed only through
//! `AuroraPlaybackRuntime::push_direct_s32_words`. This covers Aurora's
//! production direct-eARC input normalizer, IEC61937 parser, JOC decoder,
//! fail-closed OpenJOC speaker renderer and canonical speaker output DSP.
//!
//! It is still a software fixture proof, not a physical HDMI/eARC capture test.

use std::{fs, path::PathBuf};

use aurora_core::{AudioFormat, SampleType};
use aurora_decoder_engine::EngineConfig;
use aurora_decoder_open::joc_access_unit::JocAccessUnitAssembler;
use aurora_dsp_basic::output::OutputDspConfig;
use aurora_encoded_input::EncodedInputConfig;
use aurora_encoded_runtime::{AuroraPlaybackRuntime, PlaybackBatch};
use aurora_iec61937::{CarrierWordHalf, DATA_TYPE_EAC3};

const FIXTURE_ENV: &str = "AURORA_OPENJOC_SYNTHETIC_FIXTURE";
const EXPECTED_BYTES: usize = 32_768;
const EXPECTED_ACCESS_UNITS: usize = 8;
const EXPECTED_PCM_FRAMES: usize = 1_568;
const EAC3_PERIOD_BYTES: usize = 24_576;
const CHANNELS: usize = 12;
const CAPTURE_SLOTS: usize = 2;
const CAPTURE_CHUNK_SAMPLES: usize = 97 * CAPTURE_SLOTS;
const SAMPLE_RATE: u32 = 48_000;

fn output_format() -> AudioFormat {
    AudioFormat {
        sample_rate: SAMPLE_RATE,
        channel_count: CHANNELS,
        sample_type: SampleType::F32,
        block_size: 40,
    }
}

fn canonical_eac3_period(payload: &[u8]) -> Vec<u8> {
    assert!(!payload.is_empty());
    assert!(payload.len() <= usize::from(u16::MAX));

    let mut period = Vec::with_capacity(EAC3_PERIOD_BYTES);
    period.extend_from_slice(&[0x72, 0xF8, 0x1F, 0x4E]);
    period.extend_from_slice(&u16::from(DATA_TYPE_EAC3).to_le_bytes());
    period.extend_from_slice(&(payload.len() as u16).to_le_bytes());

    let mut wire = payload.to_vec();
    if wire.len() % 2 != 0 {
        wire.push(0);
    }
    for word in wire.chunks_exact_mut(2) {
        word.swap(0, 1);
    }
    period.extend_from_slice(&wire);
    assert!(period.len() <= EAC3_PERIOD_BYTES);
    period.resize(EAC3_PERIOD_BYTES, 0);
    period
}

fn carrier_to_s32_high_slots(carrier: &[u8]) -> Vec<i32> {
    assert_eq!(carrier.len() % 2, 0);
    carrier
        .chunks_exact(2)
        .map(|word| {
            let carrier_word = u16::from_le_bytes([word[0], word[1]]);
            (u32::from(carrier_word) << 16) as i32
        })
        .collect()
}

fn observe_batch(batch: PlaybackBatch, pcm_frames: &mut usize, bursts: &mut usize) {
    *bursts = (*bursts).saturating_add(batch.bursts);
    for frame in batch.frames {
        assert!(frame.frame_count > 0 && frame.frame_count <= 40);
        assert_eq!(frame.interleaved_f32.len(), frame.frame_count * CHANNELS);
        assert!(frame
            .interleaved_f32
            .iter()
            .all(|sample| sample.is_finite()));
        let expected_pts = *pcm_frames as f64 / f64::from(SAMPLE_RATE);
        assert!(
            (frame.presentation_time_seconds - expected_pts).abs() < 1.0e-12,
            "production playback PTS discontinuity: expected {expected_pts:.12}, got {:.12}",
            frame.presentation_time_seconds
        );
        *pcm_frames = (*pcm_frames).saturating_add(frame.frame_count);
    }
}

#[test]
#[ignore = "requires exact OpenJOC synthetic joc.ec3 fixture via AURORA_OPENJOC_SYNTHETIC_FIXTURE"]
fn pinned_joc_survives_production_s32_playback_runtime() {
    let path =
        PathBuf::from(std::env::var(FIXTURE_ENV).unwrap_or_else(|_| {
            panic!("set {FIXTURE_ENV} to the verified OpenJOC joc.ec3 fixture")
        }));
    let fixture = fs::read(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    assert_eq!(fixture.len(), EXPECTED_BYTES);

    let mut assembler = JocAccessUnitAssembler::new();
    let mut units = assembler
        .push(&fixture)
        .expect("split pinned JOC fixture into access units");
    units.extend(
        assembler
            .finish()
            .expect("finish pinned JOC fixture access-unit split"),
    );
    assert_eq!(units.len(), EXPECTED_ACCESS_UNITS);

    let mut runtime = AuroraPlaybackRuntime::new(
        EncodedInputConfig::DirectEarc {
            slots: CAPTURE_SLOTS,
            word_half: CarrierWordHalf::High,
        },
        EngineConfig::default(),
        output_format(),
        OutputDspConfig::default(),
    )
    .expect("construct canonical production direct-eARC playback runtime");

    let mut pcm_frames = 0_usize;
    let mut bursts = 0_usize;
    for unit in &units {
        let period = canonical_eac3_period(unit);
        let capture = carrier_to_s32_high_slots(&period);
        assert_eq!(capture.len() % CAPTURE_SLOTS, 0);
        for samples in capture.chunks(CAPTURE_CHUNK_SAMPLES) {
            assert_eq!(samples.len() % CAPTURE_SLOTS, 0);
            let batch = runtime
                .push_direct_s32_words(samples)
                .expect("production runtime must accept fragmented native S32 capture");
            observe_batch(batch, &mut pcm_frames, &mut bursts);
        }
    }

    assert_eq!(bursts, EXPECTED_ACCESS_UNITS);
    let live_joc = runtime.encoded().decoder().engine().joc_status();
    assert!(live_joc.codec_classified_joc);
    assert!(live_joc.speaker_render_active);
    assert_eq!(live_joc.layout_name.as_deref(), Some("7.1.4"));
    assert_eq!(live_joc.channel_count, Some(CHANNELS));
    assert_eq!(live_joc.object_count, Some(1));
    assert!(live_joc.fallback_reason.is_none());

    let final_batch = runtime
        .finish()
        .expect("production playback runtime must drain pinned JOC cleanly");
    observe_batch(final_batch, &mut pcm_frames, &mut bursts);
    assert_eq!(bursts, EXPECTED_ACCESS_UNITS);
    assert_eq!(
        pcm_frames, EXPECTED_PCM_FRAMES,
        "production S32 -> JOC -> speaker DSP path changed the pinned OpenJOC timeline"
    );

    let final_joc = runtime.encoded().decoder().engine().joc_status();
    assert!(final_joc.codec_classified_joc);
    assert!(!final_joc.speaker_render_active);
    assert_eq!(final_joc.layout_name.as_deref(), Some("7.1.4"));
    assert_eq!(final_joc.channel_count, Some(CHANNELS));
    assert_eq!(final_joc.object_count, Some(1));
    assert!(final_joc.fallback_reason.is_none());
}
