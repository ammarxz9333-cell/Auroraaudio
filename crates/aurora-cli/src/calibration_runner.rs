//! Automated Room Calibration Engine (SpaceFit Sound Pro equivalent).
//!
//! Generates 16-channel acoustic test stimuli and analyzes microphone recordings
//! to compute millisecond-accurate speaker arrival delays, acoustic SPL trims,
//! and RT60 reverberation times for 11.1.4 cinema setups.

use anyhow::{bail, Context, Result};
use aurora_audio_io::{read_wav, write_wav_f32_with_channel_roles};
use aurora_core::StandardLayout;
use aurora_measurement::stimulus::{generate_log_sweep, LogSweepConfig};
use aurora_measurement::{
    derive_safe_time_level_alignment, estimate_delay_frames, estimate_rt60_seconds, rms_dbfs,
    ChannelMeasurement,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Final room calibration profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomCalibrationProfile {
    pub layout: String,
    pub sample_rate: u32,
    pub total_speakers: usize,
    pub channel_calibrations: Vec<SpeakerCalibrationResult>,
    pub max_added_delay_ms: f32,
    pub average_rt60_seconds: f32,
    pub acoustic_environment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeakerCalibrationResult {
    pub channel_index: usize,
    pub role: String,
    pub measured_delay_ms: f32,
    pub added_delay_ms: f32,
    pub added_delay_samples: f32,
    pub measured_level_dbfs: f32,
    pub gain_trim_db: f32,
    pub rt60_decay_seconds: f32,
}

/// Generates a calibrated 16-channel test stimulus WAV file with sequential channel sweeps.
pub fn generate_calibration_stimulus_wav(
    output_path: &Path,
    sample_rate: u32,
    sweep_duration_sec: f32,
) -> Result<()> {
    let sweep_cfg = LogSweepConfig {
        sample_rate,
        duration_seconds: sweep_duration_sec,
        start_hz: 20.0,
        end_hz: 20_000.0,
        amplitude: 0.25,
        fade_seconds: 0.02,
    };

    let single_sweep = generate_log_sweep(sweep_cfg)
        .map_err(|e| anyhow::anyhow!("Failed to generate log sweep: {e}"))?;

    let roles = StandardLayout::ElevenOneFour.canonical_roles();
    let num_speakers = roles.len(); // 16 speakers
    let silence_frames = (sample_rate as f32 * 0.5) as usize; // 0.5s pause between sweeps
    let channel_segment_frames = single_sweep.len() + silence_frames;
    let total_frames = channel_segment_frames * num_speakers;

    let mut channels: Vec<Vec<f32>> = vec![vec![0.0; total_frames]; num_speakers];

    for (spk_idx, ch) in channels.iter_mut().enumerate() {
        let start_frame = spk_idx * channel_segment_frames;
        ch[start_frame..start_frame + single_sweep.len()].copy_from_slice(&single_sweep);
    }

    write_wav_f32_with_channel_roles(output_path, sample_rate, &channels, &roles)?;
    println!(
        "Generated 16-channel SpaceFit calibration stimulus: {} ({} frames, 11.1.4 layout)",
        output_path.display(),
        total_frames
    );

    Ok(())
}

/// Analyzes a recorded microphone sweep against the reference sweep and produces a room calibration profile.
pub fn analyze_room_calibration(
    recorded_wav_path: &Path,
    output_profile_json: &Path,
    sample_rate: u32,
) -> Result<RoomCalibrationProfile> {
    let rec_wav = read_wav(recorded_wav_path)
        .with_context(|| format!("Failed to read recorded WAV: {}", recorded_wav_path.display()))?;

    if rec_wav.channels.is_empty() || rec_wav.frame_count < 1000 {
        bail!("Recorded audio is empty or too short for calibration.");
    }

    let mic_data = &rec_wav.channels[0]; // Primary measurement microphone
    let roles = StandardLayout::ElevenOneFour.canonical_roles();
    let num_speakers = roles.len();

    // Generate local reference sweep for cross-correlation delay estimation
    let sweep_cfg = LogSweepConfig {
        sample_rate,
        duration_seconds: 2.0,
        start_hz: 20.0,
        end_hz: 20_000.0,
        amplitude: 0.25,
        fade_seconds: 0.02,
    };
    let ref_sweep = generate_log_sweep(sweep_cfg)
        .map_err(|e| anyhow::anyhow!("Failed to generate reference sweep: {e}"))?;

    let segment_len = mic_data.len() / num_speakers;
    let mut measurements = Vec::with_capacity(num_speakers);
    let mut channel_rt60s = Vec::with_capacity(num_speakers);

    for spk_idx in 0..num_speakers {
        let ch_data = if rec_wav.channels.len() > spk_idx {
            &rec_wav.channels[spk_idx]
        } else {
            mic_data
        };

        let start = spk_idx * segment_len;
        let end = (start + segment_len).min(ch_data.len());
        let segment = if rec_wav.channels.len() > spk_idx {
            &ch_data[start..end]
        } else {
            &mic_data[start..end]
        };

        let delay_frames = if segment.len() > ref_sweep.len() {
            estimate_delay_frames(&ref_sweep, segment, segment.len() - ref_sweep.len())
                .unwrap_or(0) as f32
        } else {
            0.0
        };

        let raw_level = rms_dbfs(segment).unwrap_or(-60.0);
        let level_dbfs = if raw_level.is_finite() {
            raw_level.clamp(-90.0, 0.0)
        } else {
            -90.0
        };

        let raw_rt60 = estimate_rt60_seconds(segment, sample_rate).unwrap_or(0.35);
        let rt60 = if raw_rt60.is_finite() && raw_rt60 > 0.0 {
            raw_rt60.clamp(0.05, 3.0)
        } else {
            0.35
        };

        measurements.push(ChannelMeasurement {
            delay_frames,
            level_dbfs,
            rt60_seconds: rt60,
        });
        channel_rt60s.push(rt60);
    }

    let alignments = derive_safe_time_level_alignment(&measurements)
        .map_err(|e| anyhow::anyhow!("Safe alignment derivation failed: {e}"))?;

    let mut channel_calibrations = Vec::with_capacity(num_speakers);
    let mut max_added_delay_ms = 0.0_f32;

    for (spk_idx, (meas, align)) in measurements.iter().zip(alignments.iter()).enumerate() {
        let measured_delay_ms = (meas.delay_frames / sample_rate as f32) * 1000.0;
        let added_delay_ms = (align.added_delay_frames / sample_rate as f32) * 1000.0;
        if added_delay_ms > max_added_delay_ms {
            max_added_delay_ms = added_delay_ms;
        }

        channel_calibrations.push(SpeakerCalibrationResult {
            channel_index: spk_idx,
            role: format!("{:?}", roles[spk_idx]),
            measured_delay_ms,
            added_delay_ms,
            added_delay_samples: align.added_delay_frames,
            measured_level_dbfs: meas.level_dbfs,
            gain_trim_db: align.gain_db,
            rt60_decay_seconds: meas.rt60_seconds,
        });
    }

    let avg_rt60: f32 = channel_rt60s.iter().sum::<f32>() / num_speakers as f32;
    let acoustic_environment = if avg_rt60 < 0.25 {
        "Heavily Damped / Cinema Treated Room".to_string()
    } else if avg_rt60 < 0.55 {
        "Balanced Living Room / Typical Cinema".to_string()
    } else {
        "Highly Reflective / Hard Surfaces (Acoustic Absorption Recommended)".to_string()
    };

    let profile = RoomCalibrationProfile {
        layout: "11.1.4 (16 discrete channels)".to_string(),
        sample_rate,
        total_speakers: num_speakers,
        channel_calibrations,
        max_added_delay_ms,
        average_rt60_seconds: avg_rt60,
        acoustic_environment,
    };

    let json_str = serde_json::to_string_pretty(&profile)?;
    std::fs::write(output_profile_json, json_str)?;

    println!("Room Calibration Complete:");
    println!("  - Target Layout: 11.1.4 (16 channels)");
    println!("  - Max Alignment Delay: {:.2} ms", max_added_delay_ms);
    println!("  - Average RT60: {:.3} s", avg_rt60);
    println!("  - Environment: {}", profile.acoustic_environment);
    println!("  - Profile saved: {}", output_profile_json.display());

    Ok(profile)
}
