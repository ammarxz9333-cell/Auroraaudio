#[path = "estimator.rs"]
mod estimator;

use estimator::{PpmEstimator, PpmEstimatorConfig, PpmEstimatorError};
use thiserror::Error;

const CLOCK_ESTIMATOR_WINDOW_SECONDS: u64 = 10;

/// Adaptive duplex drift-controller configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DriftControllerConfig {
    /// Nominal input device sample rate.
    pub input_rate: u32,
    /// Nominal output device sample rate.
    pub output_rate: u32,
    /// Desired ring fill in frames.
    pub target_fill_frames: usize,
    /// Maximum adaptive deviation around the nominal ratio.
    pub maximum_correction_ppm: f64,
    /// Proportional correction at one target-fill error, in ppm.
    pub proportional_gain_ppm: f64,
    /// Integral correction per second at one target-fill error, in ppm.
    pub integral_gain_ppm_per_second: f64,
    /// Maximum ratio movement per update, in ppm.
    pub maximum_step_ppm: f64,
    /// Consecutive saturated updates before reporting an unsupported mismatch.
    pub fatal_saturation_updates: u64,
}

impl Default for DriftControllerConfig {
    fn default() -> Self {
        Self {
            input_rate: 48_000,
            output_rate: 48_000,
            target_fill_frames: 1_024,
            maximum_correction_ppm: 500.0,
            proportional_gain_ppm: 500.0,
            integral_gain_ppm_per_second: 0.2,
            maximum_step_ppm: 2.0,
            fatal_saturation_updates: 1_500,
        }
    }
}

/// Numeric PI-controller fault.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum DriftControllerFault {
    /// Configuration or update values are invalid.
    #[error("invalid drift controller configuration")]
    InvalidConfiguration,
    /// Required correction or measured clock mismatch exceeded the supported range.
    #[error("clock mismatch exceeds the supported adaptive range")]
    CorrectionOutOfRange,
}

/// One observable PI-controller update.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DriftControllerReport {
    /// Absolute output/input ratio sent to the ASRC.
    pub ratio: f64,
    /// Adaptive correction relative to nominal, in parts per million.
    pub correction_ppm: f64,
    /// Current trusted feed-forward correction before PI fill feedback.
    pub feedforward_correction_ppm: f64,
    /// Signed current fill error.
    pub fill_error_frames: i64,
    /// Minimum ratio produced since reset.
    pub minimum_ratio: f64,
    /// Maximum ratio produced since reset.
    pub maximum_ratio: f64,
    /// Total updates whose unconstrained result exceeded the ratio clamp.
    pub saturation_count: u64,
    /// Current consecutive saturated updates.
    pub consecutive_saturation: u64,
}

/// Conservative PI controller for an adaptive output/input ratio.
#[derive(Debug, Clone)]
pub struct DriftController {
    config: DriftControllerConfig,
    nominal_ratio: f64,
    current_correction_ppm: f64,
    feedforward_correction_ppm: f64,
    integral_ppm: f64,
    minimum_ratio: f64,
    maximum_ratio: f64,
    saturation_count: u64,
    consecutive_saturation: u64,
    estimator: PpmEstimator,
}

/// Hardware-independent adaptive drift simulation result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdaptiveDriftSimulationReport {
    /// Input clock error in parts per million.
    pub input_clock_ppm: f64,
    /// Simulated duration.
    pub duration_seconds: u64,
    /// Final ring fill.
    pub final_fill_frames: f64,
    /// Minimum simulated ring fill.
    pub minimum_fill_frames: f64,
    /// Maximum simulated ring fill.
    pub maximum_fill_frames: f64,
    /// Final adaptive correction.
    pub final_correction_ppm: f64,
    /// Controller saturation count.
    pub saturation_count: u64,
    /// Whether fill remained within fixed capacity without controller fault.
    pub bounded: bool,
}

impl DriftController {
    /// Creates a validated controller at its nominal ratio.
    pub fn new(config: DriftControllerConfig) -> Result<Self, DriftControllerFault> {
        if config.input_rate == 0
            || config.output_rate == 0
            || config.target_fill_frames == 0
            || !config.maximum_correction_ppm.is_finite()
            || config.maximum_correction_ppm <= 0.0
            || !config.proportional_gain_ppm.is_finite()
            || config.proportional_gain_ppm < 0.0
            || !config.integral_gain_ppm_per_second.is_finite()
            || config.integral_gain_ppm_per_second < 0.0
            || !config.maximum_step_ppm.is_finite()
            || config.maximum_step_ppm <= 0.0
            || config.fatal_saturation_updates == 0
        {
            return Err(DriftControllerFault::InvalidConfiguration);
        }
        let nominal_ratio = f64::from(config.output_rate) / f64::from(config.input_rate);
        let estimator = PpmEstimator::new(PpmEstimatorConfig {
            input_rate: config.input_rate,
            output_rate: config.output_rate,
            window_output_frames: u64::from(config.output_rate)
                .checked_mul(CLOCK_ESTIMATOR_WINDOW_SECONDS)
                .ok_or(DriftControllerFault::InvalidConfiguration)?,
            trusted_windows: 3,
            maximum_abs_ppm: 2_000.0,
        })
        .map_err(|_| DriftControllerFault::InvalidConfiguration)?;
        Ok(Self {
            config,
            nominal_ratio,
            current_correction_ppm: 0.0,
            feedforward_correction_ppm: 0.0,
            integral_ppm: 0.0,
            minimum_ratio: nominal_ratio,
            maximum_ratio: nominal_ratio,
            saturation_count: 0,
            consecutive_saturation: 0,
            estimator,
        })
    }

    /// Feeds independent clock-domain frame accounting into the fixed-storage estimator.
    ///
    /// The estimator emits one measurement every ten seconds of output time and requires three
    /// consecutive clean windows before it may drive feed-forward. `discontinuity` must be set
    /// for XRUNs, reconnects, format/epoch changes, callback gaps, or any event that makes the
    /// two frame counters incomparable. Such an event starts a fresh adaptive clock epoch by
    /// clearing estimator history, feed-forward, PI integral/current correction, saturation
    /// history, and ratio extrema before new measurements are trusted.
    pub fn observe_clock_frames(
        &mut self,
        input_frames: u64,
        output_frames: u64,
        discontinuity: bool,
    ) -> Result<Option<f64>, DriftControllerFault> {
        let estimate = self
            .estimator
            .observe(input_frames, output_frames, discontinuity);
        match estimate {
            Ok(Some(estimate)) if estimate.trusted => {
                self.set_feedforward_clock_ppm(estimate.ppm)?;
                Ok(Some(estimate.ppm))
            }
            Ok(_) => {
                if discontinuity {
                    self.reset();
                }
                Ok(None)
            }
            Err(PpmEstimatorError::EstimateOutOfRange) => {
                self.clear_feedforward();
                Err(DriftControllerFault::CorrectionOutOfRange)
            }
            Err(PpmEstimatorError::InvalidConfiguration) => {
                self.clear_feedforward();
                Err(DriftControllerFault::InvalidConfiguration)
            }
        }
    }

    /// Installs a trusted estimate of the input clock error.
    ///
    /// Positive input-clock ppm requires a negative ASRC correction, so the sign inversion is
    /// performed here once. The resulting feed-forward term is clamped to the same configured
    /// correction envelope as the PI controller. Ratio movement remains limited by
    /// `maximum_step_ppm` in `update`, preventing a measurement update from creating a pitch step.
    pub fn set_feedforward_clock_ppm(
        &mut self,
        estimated_input_clock_ppm: f64,
    ) -> Result<(), DriftControllerFault> {
        if !estimated_input_clock_ppm.is_finite() {
            return Err(DriftControllerFault::InvalidConfiguration);
        }
        self.feedforward_correction_ppm = (-estimated_input_clock_ppm).clamp(
            -self.config.maximum_correction_ppm,
            self.config.maximum_correction_ppm,
        );
        Ok(())
    }

    /// Clears feed-forward when a stream epoch, device, or clock relationship changes.
    pub fn clear_feedforward(&mut self) {
        self.feedforward_correction_ppm = 0.0;
    }

    /// Returns the currently installed feed-forward correction.
    pub fn feedforward_correction_ppm(&self) -> f64 {
        self.feedforward_correction_ppm
    }

    /// Updates the ratio from current fill and elapsed output frames.
    ///
    /// Positive fill error lowers output/input ratio so each output block
    /// consumes more input frames. One shared result applies to all channels.
    pub fn update(
        &mut self,
        current_fill_frames: usize,
        _fill_trend_frames: i64,
        elapsed_output_frames: usize,
    ) -> Result<DriftControllerReport, DriftControllerFault> {
        if elapsed_output_frames == 0 {
            return Err(DriftControllerFault::InvalidConfiguration);
        }
        let error = current_fill_frames as i128 - self.config.target_fill_frames as i128;
        let fill_error_frames = error.clamp(i64::MIN as i128, i64::MAX as i128) as i64;
        let normalized_error = fill_error_frames as f64 / self.config.target_fill_frames as f64;
        let elapsed_seconds = elapsed_output_frames as f64 / f64::from(self.config.output_rate);
        let integral_candidate = self.integral_ppm
            + normalized_error * self.config.integral_gain_ppm_per_second * elapsed_seconds;
        let proportional = normalized_error * self.config.proportional_gain_ppm;
        let unconstrained = self.feedforward_correction_ppm - proportional - integral_candidate;
        let constrained = unconstrained.clamp(
            -self.config.maximum_correction_ppm,
            self.config.maximum_correction_ppm,
        );
        let saturated = constrained != unconstrained;

        // Integrate only when unsaturated or when the error moves the controller
        // back toward its supported range.
        if !saturated
            || (unconstrained > 0.0 && normalized_error > 0.0)
            || (unconstrained < 0.0 && normalized_error < 0.0)
        {
            self.integral_ppm = integral_candidate;
        }
        if saturated {
            self.saturation_count = self.saturation_count.saturating_add(1);
            self.consecutive_saturation = self.consecutive_saturation.saturating_add(1);
        } else {
            self.consecutive_saturation = 0;
        }
        let delta = (constrained - self.current_correction_ppm)
            .clamp(-self.config.maximum_step_ppm, self.config.maximum_step_ppm);
        self.current_correction_ppm += delta;
        let ratio = self.nominal_ratio * (1.0 + self.current_correction_ppm / 1_000_000.0);
        self.minimum_ratio = self.minimum_ratio.min(ratio);
        self.maximum_ratio = self.maximum_ratio.max(ratio);
        let report = DriftControllerReport {
            ratio,
            correction_ppm: self.current_correction_ppm,
            feedforward_correction_ppm: self.feedforward_correction_ppm,
            fill_error_frames,
            minimum_ratio: self.minimum_ratio,
            maximum_ratio: self.maximum_ratio,
            saturation_count: self.saturation_count,
            consecutive_saturation: self.consecutive_saturation,
        };
        if self.consecutive_saturation >= self.config.fatal_saturation_updates {
            return Err(DriftControllerFault::CorrectionOutOfRange);
        }
        Ok(report)
    }

    /// Returns the nominal output/input ratio.
    pub fn nominal_ratio(&self) -> f64 {
        self.nominal_ratio
    }

    /// Clears estimator, integral, feed-forward, and extrema state for a new clock epoch.
    pub fn reset(&mut self) {
        self.current_correction_ppm = 0.0;
        self.feedforward_correction_ppm = 0.0;
        self.integral_ppm = 0.0;
        self.minimum_ratio = self.nominal_ratio;
        self.maximum_ratio = self.nominal_ratio;
        self.saturation_count = 0;
        self.consecutive_saturation = 0;
        self.estimator.reset();
    }
}

/// Simulates independent clocks and PI updates at one-second control intervals.
pub fn simulate_adaptive_drift(
    input_clock_ppm: f64,
    duration_seconds: u64,
    config: DriftControllerConfig,
    capacity_frames: usize,
) -> Result<AdaptiveDriftSimulationReport, DriftControllerFault> {
    simulate_adaptive_drift_inner(
        input_clock_ppm,
        duration_seconds,
        config,
        capacity_frames,
        None,
    )
}

fn simulate_adaptive_drift_inner(
    input_clock_ppm: f64,
    duration_seconds: u64,
    config: DriftControllerConfig,
    capacity_frames: usize,
    feedforward_clock_ppm: Option<f64>,
) -> Result<AdaptiveDriftSimulationReport, DriftControllerFault> {
    let mut controller = DriftController::new(config)?;
    if let Some(ppm) = feedforward_clock_ppm {
        controller.set_feedforward_clock_ppm(ppm)?;
    }
    let mut fill = config.target_fill_frames as f64;
    let mut minimum = fill;
    let mut maximum = fill;
    let mut final_report =
        controller.update(config.target_fill_frames, 0, config.output_rate as usize)?;
    let mut bounded = true;
    for _ in 0..duration_seconds {
        final_report = controller.update(
            fill.round().clamp(0.0, usize::MAX as f64) as usize,
            (fill - config.target_fill_frames as f64)
                .round()
                .clamp(i64::MIN as f64, i64::MAX as f64) as i64,
            config.output_rate as usize,
        )?;
        fill += f64::from(config.input_rate) * (input_clock_ppm + final_report.correction_ppm)
            / 1_000_000.0;
        minimum = minimum.min(fill);
        maximum = maximum.max(fill);
        if fill < 0.0 || fill >= capacity_frames as f64 {
            bounded = false;
            break;
        }
    }
    Ok(AdaptiveDriftSimulationReport {
        input_clock_ppm,
        duration_seconds,
        final_fill_frames: fill,
        minimum_fill_frames: minimum,
        maximum_fill_frames: maximum,
        final_correction_ppm: final_report.correction_ppm,
        saturation_count: final_report.saturation_count,
        bounded,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_moves_slowly_and_in_the_correct_direction() {
        let mut controller = DriftController::new(DriftControllerConfig::default()).unwrap();
        let high = controller.update(1_200, 10, 256).unwrap();
        assert!(high.ratio < controller.nominal_ratio());
        assert!(high.correction_ppm >= -2.0);
        let low = controller.update(800, -10, 256).unwrap();
        assert!(low.correction_ppm > high.correction_ppm);
    }

    #[test]
    fn estimator_trust_drives_feedforward_and_discontinuity_clears_it() {
        let config = DriftControllerConfig::default();
        let mut controller = DriftController::new(config).unwrap();
        for clean_window in 0..3 {
            let estimate = controller
                .observe_clock_frames(480_120, 480_000, false)
                .unwrap();
            if clean_window < 2 {
                assert!(estimate.is_none());
            } else {
                assert!((estimate.unwrap() - 250.0).abs() <= 5.0);
            }
        }
        assert!((controller.feedforward_correction_ppm() + 250.0).abs() <= 5.0);
        assert!(controller
            .observe_clock_frames(128, 128, true)
            .unwrap()
            .is_none());
        assert_eq!(controller.feedforward_correction_ppm(), 0.0);
        let reset = controller
            .update(config.target_fill_frames, 0, config.output_rate as usize)
            .unwrap();
        assert_eq!(reset.correction_ppm, 0.0);
        assert_eq!(reset.minimum_ratio, controller.nominal_ratio());
        assert_eq!(reset.maximum_ratio, controller.nominal_ratio());
        assert_eq!(reset.saturation_count, 0);
    }

    #[test]
    fn feedforward_is_slew_limited_and_uses_clock_error_sign() {
        let config = DriftControllerConfig::default();
        let mut controller = DriftController::new(config).unwrap();
        controller.set_feedforward_clock_ppm(250.0).unwrap();
        assert_eq!(controller.feedforward_correction_ppm(), -250.0);
        let first = controller
            .update(config.target_fill_frames, 0, config.output_rate as usize)
            .unwrap();
        assert!((first.correction_ppm + config.maximum_step_ppm).abs() < 1.0e-12);
        let second = controller
            .update(config.target_fill_frames, 0, config.output_rate as usize)
            .unwrap();
        assert!(
            (second.correction_ppm - first.correction_ppm).abs()
                <= config.maximum_step_ppm + f64::EPSILON
        );
    }

    #[test]
    fn anti_windup_bounds_integral_and_reports_saturation() {
        let config = DriftControllerConfig {
            fatal_saturation_updates: 10_000,
            ..DriftControllerConfig::default()
        };
        let mut controller = DriftController::new(config).unwrap();
        let mut report = controller.update(20_000, 1_000, 256).unwrap();
        for _ in 0..2_000 {
            report = controller.update(20_000, 1_000, 256).unwrap();
        }
        assert!(report.correction_ppm >= -config.maximum_correction_ppm);
        assert!(report.saturation_count > 0);
        for _ in 0..500 {
            report = controller
                .update(config.target_fill_frames, 0, 256)
                .unwrap();
        }
        assert!(report.correction_ppm.abs() < config.maximum_correction_ppm);
    }

    #[test]
    fn prolonged_unsupported_mismatch_faults() {
        let config = DriftControllerConfig {
            fatal_saturation_updates: 4,
            ..DriftControllerConfig::default()
        };
        let mut controller = DriftController::new(config).unwrap();
        let mut result = Ok(controller.update(10_000, 1_000, 256).unwrap());
        for _ in 0..8 {
            result = controller.update(10_000, 1_000, 256);
            if result.is_err() {
                break;
            }
        }
        assert_eq!(result, Err(DriftControllerFault::CorrectionOutOfRange));
    }

    #[test]
    fn differing_nominal_rates_produce_expected_base_ratio() {
        let config = DriftControllerConfig {
            input_rate: 44_100,
            output_rate: 48_000,
            ..DriftControllerConfig::default()
        };
        let controller = DriftController::new(config).unwrap();
        assert!((controller.nominal_ratio() - 48_000.0 / 44_100.0).abs() < 1.0e-12);
    }

    #[test]
    fn realistic_ppm_simulations_remain_bounded_for_eight_hours() {
        let config = DriftControllerConfig {
            target_fill_frames: 2_048,
            ..DriftControllerConfig::default()
        };
        for duration in [600, 3_600, 8 * 3_600] {
            for ppm in [
                -250.0, -100.0, -50.0, -25.0, -10.0, 10.0, 25.0, 50.0, 100.0, 250.0,
            ] {
                let report = simulate_adaptive_drift(ppm, duration, config, 8_192).unwrap();
                assert!(report.bounded, "{report:?}");
                assert!(
                    (report.final_correction_ppm + ppm).abs() < 20.0,
                    "{report:?}"
                );
            }
        }
    }

    #[test]
    fn trusted_feedforward_keeps_plus_minus_250_ppm_bounded_for_twenty_four_hours() {
        let config = DriftControllerConfig {
            target_fill_frames: 2_048,
            ..DriftControllerConfig::default()
        };
        for ppm in [-250.0, 250.0] {
            let report =
                simulate_adaptive_drift_inner(ppm, 24 * 3_600, config, 4_096, Some(ppm)).unwrap();
            assert!(report.bounded, "{report:?}");
            let maximum_excursion = (report.maximum_fill_frames - config.target_fill_frames as f64)
                .abs()
                .max((report.minimum_fill_frames - config.target_fill_frames as f64).abs());
            assert!(maximum_excursion <= 3_072.0, "{report:?}");
            assert!(
                (report.final_correction_ppm + ppm).abs() < 5.0,
                "{report:?}"
            );
        }
    }
}
