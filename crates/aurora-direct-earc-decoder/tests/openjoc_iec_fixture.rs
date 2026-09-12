//! Opt-in direct-eARC software proof using OpenJOC's published synthetic JOC
//! fixture. This wraps each complete JOC AU in a canonical IEC61937 E-AC-3
//! burst, embeds that carrier into native ALSA-shaped S32_LE stereo slots, then
//! exercises Aurora's real S32 normalizer -> transport parser -> decoder-engine
//! path.
//!
//! This remains synthetic software validation. It does not prove a TV/eARC
//! receiver, commercial streaming interoperability, DRM, TDM hardware or
//! loudspeaker output.

use std::{fs, path::PathBuf};

use aurora_core::{AudioFormat, SampleType};
use aurora_decoder_engine::EngineConfig;
use aurora_decoder_open::joc_access_unit::JocAccessUnitAssembler;
use aurora_direct_earc_decoder::DirectEarcDecoder;
use aurora_iec61937::{
    CarrierWordHalf, S32LeCarrierNormalizer, TransportCodec, DATA_TYPE_EAC3,
};

const FIXTURE_ENV: &str = "AURORA_OPENJOC_SYNTHETIC_FIXTURE";
const EXPECTED_BYTES: usize = 32_768;
const EXPECTED_ACCESS_UNITS: usize = 8;
const EXPECTED_PINNED_OPENJOC_PCM_FRAMES: usize = 1_568;
const EAC3_PERIOD_BYTES: usize = 24_576;
const CAPTURE_SLOTS: usize = 2;
const S32_CAPTURE_CHUNK_SAMPLES: usize = 97 * CAPTURE_SLOTS;
const OUTPUT_RATE: f64 = 48_000.0;

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

/// Models the proven Linux capture contract: two S32_LE ALSA slots carry one
/// IEC61937 S16 word each in the high half of the native 32-bit slot. Casting
/// through u32 preserves every carrier bit, including sign-bit patterns.
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

        // DirectEarcDecoder owns the presentation clock. Every emitted block,
        // including the short EOF retirement tail, must start exactly where the
        // previous real PCM block ended; decoder/backend-local clock restarts are
        // not allowed to leak into the transport-facing timeline.
        let expected_pts = *pcm_frames as f64 / OUTPUT_RATE;
        assert!(
            (frame.audio.presentation_time_seconds - expected_pts).abs() < 1.0e-12,
            "non-contiguous direct-eARC PTS: expected {expected_pts:.12}, got {:.12}",
            frame.audio.presentation_time_seconds
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

    let mut normalizer = S32LeCarrierNormalizer::new(CAPTURE_SLOTS, CarrierWordHalf::High)
        .expect("construct native ALSA S32 direct-eARC normalizer");
    let mut decoder = DirectEarcDecoder::new(EngineConfig::default());
    decoder
        .configure(format_7_1_4())
        .expect("configure direct-eARC decoder output");

    let mut bursts = 0_usize;
    let mut pcm_frames = 0_usize;
    let mut first_burst_admission_checked = false;
    for unit in &units {
        let period = canonical_eac3_period(unit);
        let s32_capture = carrier_to_s32_high_slots(&period);
        assert_eq!(s32_capture.len() % CAPTURE_SLOTS, 0);

        // 97 two-slot ALSA frames deliberately fragment IEC61937 headers,
        // payloads and idle padding across calls. The production normalizer must
        // still recover the canonical carrier bit-for-bit before decoding it.
        let mut reconstructed_period = Vec::with_capacity(period.len());
        for samples in s32_capture.chunks(S32_CAPTURE_CHUNK_SAMPLES) {
            assert_eq!(samples.len() % CAPTURE_SLOTS, 0);
            let carrier = normalizer
                .push_s32_words(samples)
                .expect("normalize native ALSA S32 capture samples");
            reconstructed_period.extend_from_slice(&carrier);

            let batch = decoder
                .push_carrier(&carrier, false)
                .expect("decode normalized synthetic direct-eARC carrier chunk");
            let prior_bursts = bursts;
            bursts = bursts.saturating_add(batch.bursts);
            assert!(
                batch
                    .transport_codecs
                    .iter()
                    .all(|codec| *codec == TransportCodec::Eac3)
            );

            if prior_bursts == 0 && batch.bursts > 0 {
                // The IEC61937 0x15 burst itself is an authenticated complete
                // E-AC-3 AU boundary. Aurora must classify/render that first AU
                // immediately rather than buffering it until the next AU arrives.
                let first_joc = decoder.engine().joc_status();
                assert!(first_joc.codec_classified_joc);
                assert!(first_joc.speaker_render_active);
                assert!(first_joc.fallback_reason.is_none());
                first_burst_admission_checked = true;
            }

            observe_frames(batch.frames, &mut pcm_frames);
        }
        assert_eq!(
            reconstructed_period, period,
            "S32 ALSA normalization changed IEC61937 carrier bits"
        );
    }

    normalizer
        .finish()
        .expect("S32 capture stream must end on a complete ALSA frame");
    let expected_carrier_words = EXPECTED_ACCESS_UNITS * EAC3_PERIOD_BYTES / 2;
    assert_eq!(normalizer.output_words(), expected_carrier_words as u64);
    assert_eq!(
        normalizer.carrier_frames(),
        (expected_carrier_words / CAPTURE_SLOTS) as u64
    );

    assert!(
        first_burst_admission_checked,
        "first IEC61937 E-AC-3 burst was never admitted before a second AU"
    );
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
    assert_eq!(
        pcm_frames, EXPECTED_PINNED_OPENJOC_PCM_FRAMES,
        "S32 direct-eARC path must preserve the exact pinned OpenJOC PCM timeline"
    );

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
