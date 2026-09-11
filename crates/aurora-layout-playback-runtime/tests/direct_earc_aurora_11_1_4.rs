use std::{fs, path::PathBuf};

use aurora_core::{AudioFormat, SampleType};
use aurora_decoder_engine::EngineConfig;
use aurora_decoder_open::joc_access_unit::JocAccessUnitAssembler;
use aurora_dsp_basic::output::OutputDspConfig;
use aurora_encoded_input::EncodedInputConfig;
use aurora_iec61937::{CarrierWordHalf, DATA_TYPE_EAC3};
use aurora_layout_playback_runtime::LayoutPlaybackRuntime;

const FIXTURE_ENV: &str = "AURORA_OPENJOC_SYNTHETIC_FIXTURE";
const EAC3_PERIOD_BYTES: usize = 24_576;

fn output_format() -> AudioFormat {
    AudioFormat {
        sample_rate: 48_000,
        channel_count: 16,
        sample_type: SampleType::F32,
        block_size: 40,
    }
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

fn high_half_s32(carrier: &[u8]) -> Vec<i32> {
    assert_eq!(carrier.len() % 2, 0);
    carrier
        .chunks_exact(2)
        .map(|word| {
            let word = u16::from_le_bytes([word[0], word[1]]);
            (u32::from(word) << 16) as i32
        })
        .collect()
}

#[test]
#[ignore = "requires exact OpenJOC synthetic joc.ec3 fixture via AURORA_OPENJOC_SYNTHETIC_FIXTURE"]
fn direct_earc_s32_reaches_layout_runtime_aurora_11_1_4() {
    let path = PathBuf::from(
        std::env::var(FIXTURE_ENV)
            .unwrap_or_else(|_| panic!("set {FIXTURE_ENV} to the verified OpenJOC joc.ec3 fixture")),
    );
    let fixture = fs::read(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));

    let mut assembler = JocAccessUnitAssembler::new();
    let mut units = assembler
        .push(&fixture)
        .expect("split published JOC fixture into access units");
    units.extend(
        assembler
            .finish()
            .expect("finish published JOC fixture access-unit split"),
    );
    assert!(!units.is_empty());

    let input = EncodedInputConfig::DirectEarc {
        slots: 2,
        word_half: CarrierWordHalf::High,
    };
    let mut runtime = LayoutPlaybackRuntime::new_for_aurora_eleven_one_four_reference(
        input,
        EngineConfig::default(),
        output_format(),
        OutputDspConfig::default(),
    )
    .expect("construct Aurora 11.1.4 reference direct-eARC runtime");

    let mut bursts = 0_usize;
    let mut output_frames = 0_usize;
    let mut output_samples = 0_usize;

    for unit in &units {
        let carrier = canonical_eac3_period(unit);
        let s32 = high_half_s32(&carrier);
        for chunk in s32.chunks(514) {
            let mut batch = runtime
                .push_direct_s32_words(chunk)
                .expect("decode direct-eARC S32 chunk through layout runtime");
            bursts = bursts.saturating_add(batch.bursts);
            for frame in batch.frames.drain(..) {
                assert_eq!(frame.channel_count, 16);
                assert!(frame.frame_count > 0);
                assert_eq!(frame.interleaved_f32.len(), frame.frame_count * 16);
                assert!(frame.interleaved_f32.iter().all(|sample| sample.is_finite()));
                output_samples = output_samples.saturating_add(frame.frame_count);
                output_frames = output_frames.saturating_add(1);
                runtime.recycle_output_frame(frame);
            }
        }
    }

    let mut final_batch = runtime.finish().expect("finish Aurora 11.1.4 runtime");
    bursts = bursts.saturating_add(final_batch.bursts);
    for frame in final_batch.frames.drain(..) {
        assert_eq!(frame.channel_count, 16);
        assert!(frame.frame_count > 0);
        assert_eq!(frame.interleaved_f32.len(), frame.frame_count * 16);
        assert!(frame.interleaved_f32.iter().all(|sample| sample.is_finite()));
        output_samples = output_samples.saturating_add(frame.frame_count);
        output_frames = output_frames.saturating_add(1);
        runtime.recycle_output_frame(frame);
    }

    assert_eq!(bursts, units.len());
    assert!(output_frames > 0, "layout runtime emitted no speaker frames");
    assert!(output_samples > 0, "layout runtime emitted no speaker samples");

    let joc = runtime.encoded().decoder().engine().joc_status();
    assert!(joc.codec_classified_joc);
    assert_eq!(
        joc.layout_name.as_deref(),
        Some("aurora-11.1.4-reference-v1")
    );
    assert_eq!(joc.channel_count, Some(16));
    assert_eq!(joc.object_count, Some(1));
    assert!(joc.fallback_reason.is_none());
}
