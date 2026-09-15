//! Public entrypoint for Aurora's offline CamillaDSP adapter.
//!
//! The legacy adapter API remains available and interprets numeric channel IDs as
//! physical file-order indices. Role-aware callers should use the layout-aware
//! helpers below whenever the input WAV was serialized from Aurora's canonical
//! logical channel order. WAVE_FORMAT_EXTENSIBLE orders channels by speaker-mask
//! bit, which differs from Aurora's canonical 7.1/7.1.4 order for side vs back
//! surrounds.

#[path = "lib.rs"]
mod legacy;

pub use legacy::*;

use aurora_core::StandardLayout;
use std::path::Path;
use std::time::Duration;

/// Resolves one Aurora logical channel index to the physical channel index used
/// by a WAVE_FORMAT_EXTENSIBLE file for a standard layout.
///
/// This mapping is intentionally explicit rather than inferred from channel
/// count. For example, Aurora 7.1.4 uses logical order
/// `FL, FR, FC, LFE, SL, SR, SBL, SBR, ...`, while the WAVE speaker-mask order
/// is `FL, FR, FC, LFE, SBL, SBR, SL, SR, ...`.
pub fn wav_file_channel_index_for_layout(
    layout: StandardLayout,
    channel_count: usize,
    logical_channel: usize,
) -> Result<usize, CamillaDspError> {
    if !layout.is_standard() {
        return Err(CamillaDspError::InvalidConfig(
            "role-aware CamillaDSP mapping requires a standard Aurora layout".to_owned(),
        ));
    }

    let roles = layout.canonical_roles();
    if roles.len() != channel_count {
        return Err(CamillaDspError::InvalidConfig(format!(
            "layout {:?} has {} channels but config channel_count is {}",
            layout,
            roles.len(),
            channel_count
        )));
    }
    if logical_channel >= roles.len() {
        return Err(CamillaDspError::InvalidConfig(format!(
            "logical channel {} is outside layout channel count {}",
            logical_channel,
            roles.len()
        )));
    }

    let target_bit = roles[logical_channel].wav_channel_mask_bit().ok_or_else(|| {
        CamillaDspError::InvalidConfig(format!(
            "layout {:?} contains a channel role without a standard WAVE speaker-mask bit",
            layout
        ))
    })?;

    let mut file_index = 0usize;
    for role in roles {
        let bit = role.wav_channel_mask_bit().ok_or_else(|| {
            CamillaDspError::InvalidConfig(format!(
                "layout {:?} contains a channel role without a standard WAVE speaker-mask bit",
                layout
            ))
        })?;
        if bit < target_bit {
            file_index += 1;
        }
    }
    Ok(file_index)
}

/// Returns a copy of an Aurora DSP config whose per-channel indices have been
/// translated from Aurora logical layout order to physical WAV file order.
///
/// The original config is not mutated. Duplicate logical channel entries fail
/// closed because applying two independent chains to the same physical channel
/// would make the intended semantics ambiguous.
pub fn remap_config_for_wav_layout(
    config: &AuroraDspConfig,
    layout: StandardLayout,
) -> Result<AuroraDspConfig, CamillaDspError> {
    if !layout.is_standard() {
        return Err(CamillaDspError::InvalidConfig(
            "role-aware CamillaDSP mapping requires a standard Aurora layout".to_owned(),
        ));
    }
    if layout.canonical_roles().len() != config.channel_count {
        return Err(CamillaDspError::InvalidConfig(format!(
            "layout {:?} has {} channels but config channel_count is {}",
            layout,
            layout.canonical_roles().len(),
            config.channel_count
        )));
    }

    let mut seen = vec![false; config.channel_count];
    let mut mapped = config.clone();
    for channel in &mut mapped.channels {
        if channel.channel >= config.channel_count {
            return Err(CamillaDspError::InvalidConfig(format!(
                "logical channel {} is outside channel_count {}",
                channel.channel, config.channel_count
            )));
        }
        if seen[channel.channel] {
            return Err(CamillaDspError::InvalidConfig(format!(
                "duplicate logical channel {} in role-aware CamillaDSP config",
                channel.channel
            )));
        }
        seen[channel.channel] = true;
        channel.channel =
            wav_file_channel_index_for_layout(layout, config.channel_count, channel.channel)?;
    }
    Ok(mapped)
}

/// Generates CamillaDSP YAML while interpreting `ChannelDspConfig.channel` as
/// an Aurora logical channel index for the supplied standard layout.
pub fn generate_camilladsp_yaml_for_layout(
    config: &AuroraDspConfig,
    layout: StandardLayout,
    input_path: &Path,
    output_path: &Path,
) -> Result<String, CamillaDspError> {
    let mapped = remap_config_for_wav_layout(config, layout)?;
    legacy::generate_camilladsp_yaml(&mapped, input_path, output_path)
}

/// Processes a WAV through CamillaDSP after mapping Aurora logical channel IDs
/// to the physical WAVE channel order for the supplied standard layout.
pub fn process_offline_wav_for_layout(
    executable: &Path,
    input_path: &Path,
    output_path: &Path,
    config: &AuroraDspConfig,
    layout: StandardLayout,
    keep_temp: bool,
    timeout: Duration,
) -> Result<CamillaDspRunReport, CamillaDspError> {
    let mapped = remap_config_for_wav_layout(config, layout)?;
    legacy::process_offline_wav(
        executable,
        input_path,
        output_path,
        &mapped,
        keep_temp,
        timeout,
    )
}

impl CamillaDspOfflineAdapter {
    /// Role-aware counterpart of [`CamillaDspOfflineAdapter::process_wav`].
    ///
    /// Use this for standard Aurora layouts when the DSP channel IDs are in
    /// Aurora logical order and the input is a role-tagged WAV serialized by
    /// Aurora's WAVE_FORMAT_EXTENSIBLE writer.
    pub fn process_wav_for_layout(
        &self,
        input_path: &Path,
        output_path: &Path,
        config: &AuroraDspConfig,
        layout: StandardLayout,
        keep_temp: bool,
        timeout: Duration,
    ) -> Result<CamillaDspRunReport, CamillaDspError> {
        process_offline_wav_for_layout(
            &self.executable().path,
            input_path,
            output_path,
            config,
            layout,
            keep_temp,
            timeout,
        )
    }
}

#[cfg(test)]
mod role_aware_tests {
    use super::*;
    use aurora_audio_io::{read_wav, write_wav_f32_with_channel_roles};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn channel(channel: usize, gain_db: f32) -> ChannelDspConfig {
        ChannelDspConfig {
            channel,
            gain_db,
            mute: false,
            polarity_invert: false,
            delay_ms: 0.0,
            high_pass: None,
            low_pass: None,
            parametric_eq: vec![],
        }
    }

    fn seven_one_four_config() -> AuroraDspConfig {
        AuroraDspConfig {
            channel_count: 12,
            sample_rate: 48_000,
            chunk_size: 1024,
            channels: (0..12)
                .map(|index| channel(index, -(index as f32)))
                .collect(),
        }
    }

    #[test]
    fn seven_one_four_maps_logical_surrounds_to_wave_mask_order() {
        let expected = [0usize, 1, 2, 3, 6, 7, 4, 5, 8, 9, 10, 11];
        for (logical, physical) in expected.into_iter().enumerate() {
            assert_eq!(
                wav_file_channel_index_for_layout(StandardLayout::SevenOneFour, 12, logical)
                    .unwrap(),
                physical,
                "logical channel {logical} mapped incorrectly"
            );
        }
    }

    #[test]
    fn role_aware_remap_preserves_dsp_semantics_while_moving_indices() {
        let mapped =
            remap_config_for_wav_layout(&seven_one_four_config(), StandardLayout::SevenOneFour)
                .unwrap();
        let physical_indices: Vec<_> = mapped.channels.iter().map(|ch| ch.channel).collect();
        assert_eq!(
            physical_indices,
            vec![0, 1, 2, 3, 6, 7, 4, 5, 8, 9, 10, 11]
        );

        // Front center/dialogue remains physical channel 2.
        assert_eq!(mapped.channels[2].channel, 2);
        assert_eq!(mapped.channels[2].gain_db, -2.0);

        // Aurora SL (logical 4) moves to WAVE physical 6, while SBL (logical 6)
        // moves to physical 4; their distinct DSP gains remain attached to the
        // intended semantic channels.
        assert_eq!(mapped.channels[4].channel, 6);
        assert_eq!(mapped.channels[4].gain_db, -4.0);
        assert_eq!(mapped.channels[6].channel, 4);
        assert_eq!(mapped.channels[6].gain_db, -6.0);
    }

    #[test]
    fn role_aware_yaml_targets_mapped_physical_channels() {
        let config = AuroraDspConfig {
            channel_count: 12,
            sample_rate: 48_000,
            chunk_size: 1024,
            channels: vec![channel(2, -2.0), channel(4, -4.0), channel(6, -6.0)],
        };
        let yaml = generate_camilladsp_yaml_for_layout(
            &config,
            StandardLayout::SevenOneFour,
            Path::new("input.wav"),
            Path::new("output.wav"),
        )
        .unwrap();

        assert!(yaml.contains("ch2_gain"));
        assert!(yaml.contains("channels: [2]"));
        assert!(yaml.contains("ch6_gain"));
        assert!(yaml.contains("channels: [6]"));
        assert!(yaml.contains("ch4_gain"));
        assert!(yaml.contains("channels: [4]"));
    }

    #[test]
    fn role_aware_mapping_rejects_layout_count_mismatch() {
        let mut config = seven_one_four_config();
        config.channel_count = 8;
        assert!(matches!(
            remap_config_for_wav_layout(&config, StandardLayout::SevenOneFour),
            Err(CamillaDspError::InvalidConfig(_))
        ));
    }

    #[test]
    fn role_aware_mapping_rejects_custom_layout() {
        assert!(matches!(
            wav_file_channel_index_for_layout(StandardLayout::Custom, 12, 0),
            Err(CamillaDspError::InvalidConfig(_))
        ));
    }

    #[test]
    fn role_aware_mapping_rejects_duplicate_logical_channels() {
        let config = AuroraDspConfig {
            channel_count: 6,
            sample_rate: 48_000,
            chunk_size: 1024,
            channels: vec![channel(4, -1.0), channel(4, -2.0)],
        };
        assert!(matches!(
            remap_config_for_wav_layout(&config, StandardLayout::FiveOne),
            Err(CamillaDspError::InvalidConfig(_))
        ));
    }

    #[test]
    fn role_aware_real_camilladsp_preserves_center_and_surround_mapping() {
        let Ok(executable) = std::env::var("AURORA_CAMILLADSP_PATH") else {
            return;
        };

        let input = temp_wav_path("role_aware_714_input");
        let output = temp_wav_path("role_aware_714_output");
        let frame_count = 4096usize;
        let logical_channels: Vec<Vec<f32>> = (0..12)
            .map(|logical| vec![0.01 * (logical as f32 + 1.0); frame_count])
            .collect();
        write_wav_f32_with_channel_roles(
            &input,
            48_000,
            &logical_channels,
            StandardLayout::SevenOneFour.canonical_roles(),
        )
        .unwrap();

        let config = AuroraDspConfig {
            channel_count: 12,
            sample_rate: 48_000,
            chunk_size: 1024,
            channels: vec![channel(2, -6.0), channel(4, -12.0), channel(6, -18.0)],
        };
        process_offline_wav_for_layout(
            Path::new(&executable),
            &input,
            &output,
            &config,
            StandardLayout::SevenOneFour,
            false,
            Duration::from_secs(30),
        )
        .unwrap();

        let processed = read_wav(&output).unwrap();
        assert_eq!(processed.format.channel_count, 12);
        assert_eq!(processed.format.sample_rate, 48_000);

        for (logical, gain_db) in [(2usize, -6.0f32), (4, -12.0), (6, -18.0)] {
            let physical = wav_file_channel_index_for_layout(
                StandardLayout::SevenOneFour,
                12,
                logical,
            )
            .unwrap();
            let input_level = 0.01 * (logical as f32 + 1.0);
            let expected = input_level * 10.0f32.powf(gain_db / 20.0);
            let actual = peak_abs(&processed.channels[physical]);
            assert!(
                (actual - expected).abs() < 0.002,
                "logical {logical} -> physical {physical}: expected peak {expected}, got {actual}"
            );
        }

        // An unfiltered semantic neighbour must remain on its own physical
        // channel and retain its level. Logical SR is physical channel 7.
        let sr_physical =
            wav_file_channel_index_for_layout(StandardLayout::SevenOneFour, 12, 5).unwrap();
        assert_eq!(sr_physical, 7);
        let sr_peak = peak_abs(&processed.channels[sr_physical]);
        assert!((sr_peak - 0.06).abs() < 0.002, "SR peak drifted to {sr_peak}");

        let _ = fs::remove_file(input);
        let _ = fs::remove_file(output);
    }

    fn peak_abs(samples: &[f32]) -> f32 {
        samples
            .iter()
            .map(|sample| sample.abs())
            .fold(0.0f32, f32::max)
    }

    fn temp_wav_path(name: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("aurora_camilladsp_{name}_{nonce}.wav"))
    }
}
