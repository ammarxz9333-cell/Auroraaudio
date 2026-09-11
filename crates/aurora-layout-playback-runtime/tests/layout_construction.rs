use aurora_core::{AudioFormat, SampleType, StandardLayout};
use aurora_decoder_engine::EngineConfig;
use aurora_dsp_basic::output::OutputDspConfig;
use aurora_encoded_input::EncodedInputConfig;
use aurora_iec61937::CarrierWordHalf;
use aurora_layout_playback_runtime::{LayoutPlaybackError, LayoutPlaybackRuntime};

fn direct_earc_input() -> EncodedInputConfig {
    EncodedInputConfig::DirectEarc {
        slots: 2,
        word_half: CarrierWordHalf::High,
    }
}

fn format(channels: usize) -> AudioFormat {
    AudioFormat {
        sample_rate: 48_000,
        channel_count: channels,
        sample_type: SampleType::F32,
        block_size: 40,
    }
}

#[test]
fn canonical_seven_one_four_constructor_remains_available() {
    let runtime = LayoutPlaybackRuntime::new_for_standard_layout(
        direct_earc_input(),
        EngineConfig::default(),
        format(12),
        StandardLayout::SevenOneFour,
        OutputDspConfig::default(),
    )
    .expect("construct canonical 7.1.4 layout runtime");

    assert_eq!(runtime.output_layout().name(), "7.1.4");
    assert_eq!(runtime.output_layout().channel_count(), 12);
}

#[test]
fn standard_layout_constructor_rejects_conflicting_decoder_layout() {
    let mut engine = EngineConfig::default();
    engine.open_decoder.joc_layout_hint = Some("5.1.2");

    let result = LayoutPlaybackRuntime::new_for_standard_layout(
        direct_earc_input(),
        engine,
        format(12),
        StandardLayout::SevenOneFour,
        OutputDspConfig::default(),
    );

    assert!(matches!(
        result,
        Err(LayoutPlaybackError::DecoderLayoutConflict {
            expected: "7.1.4",
            actual: "5.1.2",
        })
    ));
}

#[test]
fn aurora_eleven_one_four_reference_constructor_binds_sixteen_lanes() {
    let runtime = LayoutPlaybackRuntime::new_for_aurora_eleven_one_four_reference(
        direct_earc_input(),
        EngineConfig::default(),
        format(16),
        OutputDspConfig::default(),
    )
    .expect("construct Aurora 11.1.4 reference layout runtime");

    assert_eq!(runtime.output_layout().name(), "aurora-11.1.4-reference-v1");
    assert_eq!(runtime.output_layout().channel_count(), 16);
    assert_eq!(runtime.output_layout().lfe_index(), Some(3));
    assert_eq!(runtime.output_layout().height_indices(), &[12, 13, 14, 15]);
}

#[test]
fn aurora_eleven_one_four_reference_rejects_conflicting_decoder_layout() {
    let mut engine = EngineConfig::default();
    engine.open_decoder.joc_layout_hint = Some("7.1.4");

    let result = LayoutPlaybackRuntime::new_for_aurora_eleven_one_four_reference(
        direct_earc_input(),
        engine,
        format(16),
        OutputDspConfig::default(),
    );

    assert!(matches!(
        result,
        Err(LayoutPlaybackError::DecoderLayoutConflict {
            expected: "aurora-11.1.4-reference-v1",
            actual: "7.1.4",
        })
    ));
}

#[test]
fn standard_layout_constructor_rejects_width_mismatch() {
    let result = LayoutPlaybackRuntime::new_for_standard_layout(
        direct_earc_input(),
        EngineConfig::default(),
        format(16),
        StandardLayout::SevenOneFour,
        OutputDspConfig::default(),
    );

    assert!(result.is_err());
}
