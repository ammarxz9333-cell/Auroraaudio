/// Drift correction selected for the next output callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftCorrection {
    /// Consume the requested number of input frames.
    None,
    /// Produce one additional frame while consuming one fewer input frame.
    Insert,
    /// Consume and discard one additional input frame.
    Remove,
}

/// Bounded transition applied around an inserted or removed frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrectionTransition {
    /// Abrupt reference behavior reserved for tests and comparison.
    Raw,
    /// One-frame interpolation between neighboring frames.
    LinearInterpolation,
    /// Short multichannel-coherent crossfade.
    Crossfade,
    /// Defer correction until a coherent near-zero/sign-crossing boundary.
    ZeroCrossing,
}

/// Read-only callback context supplied to an Aurora-owned compensator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DriftContext {
    /// Queue fill before serving the output callback.
    pub fill_frames: usize,
    /// Frames requested by the output callback.
    pub output_frames: usize,
    /// Desired fill after serving the callback.
    pub target_fill_frames: usize,
    /// Symmetric correction deadband in frames.
    pub threshold_frames: usize,
}

/// Aurora-owned strategy boundary for duplex clock compensation.
///
/// Implementations are created on the control thread and called only by the
/// output callback. They must not allocate, block, log, or retain borrowed PCM.
pub trait DriftCompensator: Send {
    /// Selects at most one coherent whole-frame correction for this callback.
    fn select_correction(&mut self, context: DriftContext) -> DriftCorrection;

    /// Returns the bounded transition used for selected corrections.
    fn transition(&self) -> CorrectionTransition;

    /// Returns the correction transition window in frames.
    fn correction_window_frames(&self) -> usize;

    /// Accepts a future input/output clock ratio update.
    ///
    /// Sample-slip strategies may ignore this value. Future linear, polyphase,
    /// or external ASRC implementations can update interpolation state here.
    fn update_ratio(&mut self, _input_per_output: f64) {}
}

/// Threshold-based proof-of-concept drift compensator.
#[derive(Debug, Clone)]
pub struct ThresholdDriftCompensator {
    transition: CorrectionTransition,
    correction_window_frames: usize,
}

impl ThresholdDriftCompensator {
    /// Creates a bounded threshold compensator.
    pub fn new(transition: CorrectionTransition, correction_window_frames: usize) -> Self {
        Self {
            transition,
            correction_window_frames: correction_window_frames.max(1),
        }
    }
}

impl Default for ThresholdDriftCompensator {
    fn default() -> Self {
        Self::new(CorrectionTransition::Crossfade, 16)
    }
}

impl DriftCompensator for ThresholdDriftCompensator {
    fn select_correction(&mut self, context: DriftContext) -> DriftCorrection {
        let projected = context.fill_frames.saturating_sub(context.output_frames);
        if projected
            > context
                .target_fill_frames
                .saturating_add(context.threshold_frames)
        {
            DriftCorrection::Remove
        } else if projected
            < context
                .target_fill_frames
                .saturating_sub(context.threshold_frames)
            && context.fill_frames.saturating_add(1) >= context.output_frames
        {
            DriftCorrection::Insert
        } else {
            DriftCorrection::None
        }
    }

    fn transition(&self) -> CorrectionTransition {
        self.transition
    }

    fn correction_window_frames(&self) -> usize {
        self.correction_window_frames
    }
}

/// Severity derived from numeric duplex fault thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
#[repr(u32)]
pub enum DuplexHealth {
    /// No threshold has been crossed.
    #[default]
    Normal = 0,
    /// Correction frequency deserves control-thread attention.
    Warning = 1,
    /// Correction frequency or queue excursions predict audible degradation.
    Degraded = 2,
    /// Sustained mismatch can no longer be contained safely.
    Fatal = 3,
}

/// Numeric policy observed by callbacks without logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DuplexFaultPolicy {
    /// Correction intervals below this number of output frames are warnings.
    pub warning_correction_interval_frames: u64,
    /// Correction intervals below this number are degraded audio.
    pub degraded_correction_interval_frames: u64,
    /// Maximum allowed distance from target fill before fatal status.
    pub maximum_excursion_frames: usize,
    /// Consecutive underflow callbacks allowed before fatal status.
    pub maximum_consecutive_underflows: u64,
    /// Consecutive overflow callbacks allowed before fatal status.
    pub maximum_consecutive_overflows: u64,
}

impl Default for DuplexFaultPolicy {
    fn default() -> Self {
        Self {
            warning_correction_interval_frames: 480_000,
            degraded_correction_interval_frames: 48_000,
            maximum_excursion_frames: 1_024,
            maximum_consecutive_underflows: 3,
            maximum_consecutive_overflows: 1,
        }
    }
}

/// Deterministic analytical result for independent input/output clocks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DriftSimulationReport {
    /// Signed input clock error in parts per million.
    pub input_clock_ppm: f64,
    /// Simulated duration in seconds.
    pub duration_seconds: u64,
    /// Net uncorrected fill movement over the duration.
    pub uncorrected_drift_frames: f64,
    /// Whole-frame corrections required after the deadband is crossed.
    pub corrections: u64,
    /// Average seconds between corrections, or infinity for zero drift.
    pub average_correction_interval_seconds: f64,
    /// Predicted final fill after bounded correction.
    pub final_fill_frames: f64,
    /// Whether a one-correction-per-block strategy remains bounded.
    pub bounded: bool,
    /// Predicted control-thread health state.
    pub health: DuplexHealth,
}

/// Simulates clock drift without hardware or callback wall-clock scheduling.
pub fn simulate_clock_drift(
    input_clock_ppm: f64,
    duration_seconds: u64,
    sample_rate: u32,
    block_frames: usize,
    target_fill_frames: usize,
    threshold_frames: usize,
    capacity_frames: usize,
) -> DriftSimulationReport {
    let drift_per_second = sample_rate as f64 * input_clock_ppm / 1_000_000.0;
    let total_drift = drift_per_second * duration_seconds as f64;
    let excess = (total_drift.abs() - threshold_frames as f64).max(0.0);
    let corrections = excess.floor() as u64;
    let residual = total_drift.signum() * (excess - corrections as f64);
    let final_fill = target_fill_frames as f64
        + total_drift.signum() * total_drift.abs().min(threshold_frames as f64)
        + residual;
    let drift_per_block = drift_per_second.abs() * block_frames as f64 / sample_rate as f64;
    let bounded =
        drift_per_block <= 1.0 && final_fill >= 0.0 && final_fill < capacity_frames as f64;
    let average_interval = if drift_per_second == 0.0 {
        f64::INFINITY
    } else {
        1.0 / drift_per_second.abs()
    };
    let health = if !bounded {
        DuplexHealth::Fatal
    } else if average_interval < 1.0 {
        DuplexHealth::Degraded
    } else if average_interval < 10.0 {
        DuplexHealth::Warning
    } else {
        DuplexHealth::Normal
    };
    DriftSimulationReport {
        input_clock_ppm,
        duration_seconds,
        uncorrected_drift_frames: total_drift,
        corrections,
        average_correction_interval_seconds: average_interval,
        final_fill_frames: final_fill,
        bounded,
        health,
    }
}

/// Objective correction artifact measurements against a reference signal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CorrectionArtifactMetrics {
    /// Largest adjacent-sample discontinuity.
    pub maximum_discontinuity: f64,
    /// Root-mean-square error against the reference.
    pub rms_error: f64,
    /// First-difference residual energy used as a spectral-artifact proxy.
    pub spectral_artifact_energy: f64,
}

/// Computes deterministic time-domain and high-frequency artifact metrics.
pub fn correction_artifact_metrics(
    reference: &[f32],
    corrected: &[f32],
) -> CorrectionArtifactMetrics {
    let count = reference.len().min(corrected.len());
    if count == 0 {
        return CorrectionArtifactMetrics {
            maximum_discontinuity: 0.0,
            rms_error: 0.0,
            spectral_artifact_energy: 0.0,
        };
    }
    let mut maximum_discontinuity = 0.0_f64;
    let mut squared_error = 0.0_f64;
    let mut spectral_energy = 0.0_f64;
    let mut previous_residual = 0.0_f64;
    for index in 0..count {
        let current = corrected[index] as f64;
        if index > 0 {
            maximum_discontinuity =
                maximum_discontinuity.max((current - corrected[index - 1] as f64).abs());
        }
        let residual = current - reference[index] as f64;
        squared_error += residual * residual;
        if index > 0 {
            let difference = residual - previous_residual;
            spectral_energy += difference * difference;
        }
        previous_residual = residual;
    }
    CorrectionArtifactMetrics {
        maximum_discontinuity,
        rms_error: (squared_error / count as f64).sqrt(),
        spectral_artifact_energy: spectral_energy / count as f64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn realistic_ppm_ranges_and_durations_remain_bounded() {
        for duration in [10 * 60, 60 * 60, 8 * 60 * 60] {
            for ppm in [
                -250.0, -100.0, -50.0, -25.0, -10.0, 10.0, 25.0, 50.0, 100.0, 250.0,
            ] {
                let report = simulate_clock_drift(ppm, duration, 48_000, 128, 512, 128, 2_048);
                assert!(report.bounded, "{report:?}");
                assert!(report.final_fill_frames >= 0.0);
                assert!(report.final_fill_frames < 2_048.0);
                assert!(report.uncorrected_drift_frames.is_finite());
            }
        }
    }

    #[test]
    fn abrupt_mismatch_is_fatal() {
        let report = simulate_clock_drift(20_000.0, 600, 48_000, 128, 512, 128, 2_048);
        assert!(!report.bounded);
        assert_eq!(report.health, DuplexHealth::Fatal);
    }

    #[test]
    fn artifact_metrics_are_finite_and_deterministic() {
        let reference = [0.0, 0.25, 0.5, 0.75, 1.0];
        let corrected = [0.0, 0.25, 0.375, 0.75, 1.0];
        let first = correction_artifact_metrics(&reference, &corrected);
        let second = correction_artifact_metrics(&reference, &corrected);
        assert_eq!(first, second);
        assert!(first.maximum_discontinuity.is_finite());
        assert!(first.rms_error.is_finite());
        assert!(first.spectral_artifact_energy.is_finite());
    }
}
