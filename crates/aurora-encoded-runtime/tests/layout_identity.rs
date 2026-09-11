use aurora_core::{AudioFormat, SampleType, StandardLayout};
use aurora_decoder_engine::EngineConfig;
use aurora_encoded_input::{EncodedInputConfig, EncodedInputKind};
use aurora_encoded_runtime::AuroraEncodedRuntime;
use aurora_iec61937::CarrierWordHalf;

fn format_7_1_4() -> AudioFormat {
    AudioFormat {
        sample_rate: 48_000,
        channel_count: 12,
        sample_type: SampleType::F32,
        block_size: 40,
    }
}

#[test]
fn typed_seven_one_four_identity_enters_runtime_without_count_inference() {
    let mut engine = EngineConfig::default();
    engine.open_decoder = engine
        .open_decoder
        .with_standard_joc_layout(StandardLayout::SevenOneFour)
        .expect("canonical Aurora 7.1.4 must have a fixed OpenJOC preset");
    assert_eq!(engine.open_decoder.joc_layout_hint, Some("7.1.4"));

    let runtime = AuroraEncodedRuntime::new(
        EncodedInputConfig::DirectEarc {
            slots: 2,
            word_half: CarrierWordHalf::High,
        },
        engine,
        format_7_1_4(),
    )
    .expect("typed JOC layout identity must survive runtime construction");

    assert_eq!(runtime.input_kind(), EncodedInputKind::DirectEarc);
}

#[test]
fn custom_layout_cannot_fall_back_to_channel_count_identity() {
    let error = EngineConfig::default()
        .open_decoder
        .with_standard_joc_layout(StandardLayout::Custom)
        .expect_err("custom JOC output needs explicit geometry, not channel-count inference");
    assert!(error.to_string().contains("explicit speaker geometry"));
}
