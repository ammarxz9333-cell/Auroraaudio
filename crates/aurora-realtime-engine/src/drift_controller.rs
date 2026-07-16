use thiserror::Error;

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
    /// Required correction remained outside the supported adaptive range.
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
    integral_ppm: f64,
    minimum_ratio: f64,
    maximum_ratio: f64,
    saturation_count: u64,
    consecutive_saturation: u64,
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
        Ok(Self {
            config,
            nominal_ratio,
            current_correction_ppm: 0.0,
            integral_ppm: 0.0,
            minimum_ratio: nominal_ratio,
            maximum_ratio: nominal_ratio,
            saturation_count: 0,
            consecutive_saturation: 0,
        })
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
        let unconstrained = -(proportional + integral_candidate);
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

    /// Clears integral and extrema state.
    pub fn reset(&mut self) {
        self.current_correction_ppm = 0.0;
        self.integral_ppm = 0.0;
        self.minimum_ratio = self.nominal_ratio;
        self.maximum_ratio = self.nominal_ratio;
        self.saturation_count = 0;
        self.consecutive_saturation = 0;
    }
}

/// Simulates independent clocks and PI updates at one-second control intervals.
pub fn simulate_adaptive_drift(
    input_clock_ppm: f64,
    duration_seconds: u64,
    config: DriftControllerConfig,
    capacity_frames: usize,
) -> Result<AdaptiveDriftSimulationReport, DriftControllerFault> {
    let mut controller = DriftController::new(config)?;
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
}
