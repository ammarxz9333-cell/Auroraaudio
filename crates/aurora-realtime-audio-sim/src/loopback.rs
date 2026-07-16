use aurora_realtime_engine::{
    estimate_repeated_latency, generate_measurement_sequence, LatencyEstimateError,
};
use serde::{Deserialize, Serialize};

use crate::DeterministicRng;

/// Virtual-cable latency scenario.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LatencySimulationConfig {
    /// Exact propagation and endpoint delay in frames.
    pub loopback_delay_frames: usize,
    /// Uniform per-emission delay jitter bound.
    pub jitter_frames: usize,
    /// Added white-noise level in dBFS.
    pub noise_db: f32,
    /// Cable gain in dB.
    pub attenuation_db: f32,
    /// Whether the cable reverses polarity.
    pub polarity_inverted: bool,
    /// Optional one-pole low-pass coefficient in `[0, 1]`.
    pub low_pass_alpha: Option<f32>,
    /// Deterministic seed.
    pub seed: u64,
    /// Simulation sample rate.
    pub sample_rate: u32,
}

/// Truth-versus-estimate result from a virtual cable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimulatedLatencyReport {
    /// Explicit source label; never physical measurement.
    pub source: String,
    /// Configured truth delay.
    pub true_delay_frames: usize,
    /// Estimated median delay.
    pub estimated_median_frames: f64,
    /// Minimum accepted delay.
    pub minimum_frames: usize,
    /// Maximum accepted delay.
    pub maximum_frames: usize,
    /// Absolute median error from truth.
    pub absolute_error_frames: f64,
    /// Estimated delay jitter.
    pub jitter_frames: f64,
    /// Mean correlation confidence.
    pub confidence: f32,
    /// Accepted repeated sequences.
    pub valid_measurements: usize,
    /// Detected cable polarity.
    pub polarity_inverted: bool,
    /// Whether error is within configured jitter plus one frame.
    pub passed: bool,
}

/// Simulates a deterministic virtual loopback and validates latency truth.
pub fn simulate_latency(
    config: LatencySimulationConfig,
) -> Result<SimulatedLatencyReport, LatencyEstimateError> {
    if config.sample_rate == 0 || !config.noise_db.is_finite() || !config.attenuation_db.is_finite()
    {
        return Err(LatencyEstimateError::InvalidConfiguration);
    }
    let sequence = generate_measurement_sequence(127);
    let interval = config.sample_rate as usize / 2;
    let emissions = (0..12)
        .map(|index| interval / 2 + index * interval)
        .collect::<Vec<_>>();
    let capacity = emissions.last().copied().unwrap_or(0)
        + config.loopback_delay_frames
        + config.jitter_frames
        + sequence.len()
        + 1;
    let mut captured = vec![0.0_f32; capacity];
    let mut rng = DeterministicRng::new(config.seed);
    let gain = 10.0_f32.powf(config.attenuation_db / 20.0)
        * if config.polarity_inverted { -1.0 } else { 1.0 };
    let noise_gain = 10.0_f32.powf(config.noise_db / 20.0);
    for emission in &emissions {
        let jitter = if config.jitter_frames == 0 {
            0_i64
        } else {
            let width = config.jitter_frames * 2 + 1;
            (rng.next_u64() as usize % width) as i64 - config.jitter_frames as i64
        };
        let start = emission
            .saturating_add(config.loopback_delay_frames)
            .saturating_add_signed(jitter as isize);
        let mut filtered = 0.0_f32;
        for (target, sample) in captured[start..start + sequence.len()]
            .iter_mut()
            .zip(&sequence)
        {
            let shaped = if let Some(alpha) = config.low_pass_alpha {
                filtered += alpha.clamp(0.0, 1.0) * (*sample - filtered);
                filtered
            } else {
                *sample
            };
            *target += shaped * gain;
        }
    }
    for sample in &mut captured {
        *sample += rng.next_bipolar() * noise_gain;
    }
    let estimate = estimate_repeated_latency(
        &sequence,
        &captured,
        &emissions,
        config.loopback_delay_frames + config.jitter_frames + 256,
        0.65,
        config.sample_rate,
    )?;
    let offset = estimate.detected_sample_offset;
    let first = emissions[0] + offset;
    let dot = sequence
        .iter()
        .zip(&captured[first..first + sequence.len()])
        .map(|(left, right)| left * right)
        .sum::<f32>();
    let absolute_error = (estimate.median_samples - config.loopback_delay_frames as f64).abs();
    Ok(SimulatedLatencyReport {
        source: "simulated_virtual_loopback_truth".to_owned(),
        true_delay_frames: config.loopback_delay_frames,
        estimated_median_frames: estimate.median_samples,
        minimum_frames: estimate.minimum_samples,
        maximum_frames: estimate.maximum_samples,
        absolute_error_frames: absolute_error,
        jitter_frames: estimate.jitter_samples,
        confidence: estimate.confidence,
        valid_measurements: estimate.valid_measurements,
        polarity_inverted: dot < 0.0,
        passed: absolute_error <= config.jitter_frames as f64 + 1.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> LatencySimulationConfig {
        LatencySimulationConfig {
            loopback_delay_frames: 777,
            jitter_frames: 3,
            noise_db: -60.0,
            attenuation_db: -3.0,
            polarity_inverted: false,
            low_pass_alpha: None,
            seed: 42,
            sample_rate: 48_000,
        }
    }

    #[test]
    fn virtual_loopback_recovers_truth_with_noise_and_jitter() {
        let report = simulate_latency(config()).unwrap();
        assert!(report.passed);
        assert!(report.confidence > 0.9);
        assert_eq!(report.source, "simulated_virtual_loopback_truth");
    }

    #[test]
    fn virtual_loopback_detects_polarity() {
        let mut value = config();
        value.polarity_inverted = true;
        assert!(simulate_latency(value).unwrap().polarity_inverted);
    }

    #[test]
    fn very_noisy_loopback_is_rejected() {
        let mut value = config();
        value.noise_db = 12.0;
        assert!(matches!(
            simulate_latency(value),
            Err(LatencyEstimateError::LowConfidence { .. })
        ));
    }
}
