use std::{fs, path::PathBuf};

use aurora_core::{AudioFormat, SampleType};
use aurora_decoder_api::Decoder;
use aurora_decoder_open::{
    joc_access_unit::JocAccessUnitAssembler, OpenCodecKind, OpenDecoderConfig, UniversalOpenDecoder,
};
use openjoc_api::{
    trace_access_units, OpenJocConfig, OpenJocPacket, OpenJocSession, OpenJocStatus, RenderMode,
    ValidationProfile,
};

const FIXTURE_ENV: &str = "AURORA_OPENJOC_SYNTHETIC_FIXTURE";

fn fixture() -> Vec<u8> {
    let path = PathBuf::from(
        std::env::var(FIXTURE_ENV)
            .unwrap_or_else(|_| panic!("set {FIXTURE_ENV} to the verified OpenJOC joc.ec3 fixture")),
    );
    fs::read(&path).unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

fn assemble_stream(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut assembler = JocAccessUnitAssembler::new();
    let mut units = Vec::new();
    for chunk in bytes.chunks(97) {
        units.extend(assembler.push(chunk).expect("streaming JOC AU framing"));
    }
    units.extend(assembler.finish().expect("finish streaming JOC AU framing"));
    units
}

fn direct_openjoc_samples(units: &[Vec<u8>]) -> usize {
    let config = OpenJocConfig {
        render_mode: RenderMode::Speaker,
        speaker_layout: "7.1.4".to_owned(),
        validation_profile: ValidationProfile::Auto,
        ..OpenJocConfig::default()
    };
    let mut session = OpenJocSession::new(config).expect("create direct OpenJOC 7.1.4 session");
    let mut samples = 0_usize;

    for unit in units {
        let mut retried_after_pending = false;
        loop {
            let status = session
                .push_packet(OpenJocPacket {
                    data: unit,
                    pts_samples: None,
                    discontinuity: false,
                    preroll: false,
                })
                .expect("direct OpenJOC packet");
            while let Some(frame) = session.receive_frame() {
                assert_eq!(frame.render_mode, RenderMode::Speaker);
                samples = samples.saturating_add(frame.sample_count);
            }
            if status != OpenJocStatus::OutputPending {
                break;
            }
            assert!(
                !retried_after_pending,
                "direct OpenJOC remained OutputPending after its PCM queue was drained"
            );
            retried_after_pending = true;
        }
    }

    session.drain().expect("direct OpenJOC drain");
    while let Some(frame) = session.receive_frame() {
        assert_eq!(frame.render_mode, RenderMode::Speaker);
        samples = samples.saturating_add(frame.sample_count);
    }
    samples
}

fn universal_openjoc_samples(bytes: &[u8]) -> usize {
    let mut decoder = UniversalOpenDecoder::new(OpenDecoderConfig {
        codec_hint: Some(OpenCodecKind::Eac3),
        joc_stereo_reference: false,
        ..OpenDecoderConfig::default()
    }
    .with_standard_joc_layout(aurora_core::StandardLayout::SevenOneFour)
    .expect("Aurora 7.1.4 layout hint"));
    decoder
        .configure(AudioFormat {
            sample_rate: 48_000,
            channel_count: 12,
            sample_type: SampleType::F32,
            block_size: 40,
        })
        .expect("configure universal OpenJOC output");

    let mut samples = 0_usize;
    for chunk in bytes.chunks(97) {
        if let Some(frame) = decoder.decode_chunk(chunk).expect("decode fixture chunk") {
            samples = samples.saturating_add(frame.audio.frame_count);
        }
        while let Some(frame) = decoder.decode_chunk(&[]).expect("drain ready universal PCM") {
            samples = samples.saturating_add(frame.audio.frame_count);
        }
    }

    decoder.flush_packets().expect("flush universal OpenJOC stream");
    while let Some(frame) = decoder.decode_chunk(&[]).expect("drain universal tail PCM") {
        samples = samples.saturating_add(frame.audio.frame_count);
    }
    samples
}

#[test]
#[ignore = "requires exact OpenJOC synthetic joc.ec3 fixture via AURORA_OPENJOC_SYNTHETIC_FIXTURE"]
fn streaming_wrapper_matches_direct_openjoc_timeline() {
    let bytes = fixture();
    let trace = trace_access_units(&bytes, None).expect("trace pinned synthetic JOC fixture");
    let declared_samples = trace.iter().fold(0_usize, |total, unit| {
        total.saturating_add(usize::from(unit.sample_count))
    });

    let units = assemble_stream(&bytes);
    assert_eq!(
        units.len(),
        trace.len(),
        "Aurora streaming assembler changed the OpenJOC access-unit count"
    );
    assert_eq!(
        units.concat(),
        bytes,
        "Aurora streaming assembler did not preserve the fixture byte-for-byte"
    );

    let direct_samples = direct_openjoc_samples(&units);
    let universal_samples = universal_openjoc_samples(&bytes);
    eprintln!(
        "OpenJOC parity: access_units={} declared_samples={} direct_samples={} universal_samples={}",
        units.len(), declared_samples, direct_samples, universal_samples
    );

    assert!(
        direct_samples >= declared_samples,
        "direct OpenJOC unexpectedly shortened the declared compressed-audio timeline"
    );
    assert_eq!(
        universal_samples, direct_samples,
        "Aurora UniversalOpenDecoder must preserve exactly the same PCM timeline as the pinned direct OpenJOC session"
    );
}
