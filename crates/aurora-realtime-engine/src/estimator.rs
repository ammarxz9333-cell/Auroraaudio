use thiserror::Error;

const MEDIAN_WINDOW_COUNT: usize = 3;

/// Configuration for the hardware-independent input/output clock-rate estimator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PpmEstimatorConfig {
    /// Nominal input sample rate.
    pub input_rate: u32,
    /// Nominal output sample rate.
    pub output_rate: u32,
    /// Number of output frames accumulated before one estimate is emitted.
    pub window_output_frames: u64,
    /// Number of consecutive valid windows required before an estimate is trusted.
    /// Values are limited to the fixed median history capacity of three windows.
    pub trusted_windows: u8,
    /// Absolute measurement guard. Estimates outside this bound are rejected.
    pub maximum_abs_ppm: f64,
}

impl Default for PpmEstimatorConfig {
    fn default() -> Self {
        Self {
            input_rate: 48_000,
            output_rate: 48_000,
            window_output_frames: 10 * 48_000,
            trusted_windows: MEDIAN_WINDOW_COUNT as u8,
            maximum_abs_ppm: 2_000.0,
        }
    }
}

/// Numeric estimator error suitable for control-thread diagnostics.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum PpmEstimatorError {
    /// The estimator configuration or one observation is invalid.
    #[error("invalid clock ppm estimator configuration or observation")]
    InvalidConfiguration,
    /// A completed measurement window exceeded the configured sanity bound.
    #[error("clock ppm estimate exceeded the configured sanity bound")]
    EstimateOutOfRange,
}

/// One completed rate-estimation window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PpmEstimate {
    /// Median-filtered input-clock error relative to the output clock.
    /// Positive means the input clock is producing frames faster than nominal.
    pub ppm: f64,
    /// Whether enough consecutive clean windows exist to use this value as feed-forward.
    pub trusted: bool,
    /// Number of clean windows currently represented by the fixed history.
    pub clean_windows: u8,
}

/// Allocation-free frame-count clock estimator.
///
/// The estimator deliberately knows nothing about USB, eARC, TDM, a particular host, or a
/// particular MCU. Callers feed frame counts from any two independent clock domains. A marked
/// discontinuity discards the partial window so an XRUN/reconnect cannot poison the estimate.
#[derive(Debug, Clone)]
pub struct PpmEstimator {
    config: PpmEstimatorConfig,
    input_frames: u64,
    output_frames: u64,
    history: [f64; MEDIAN_WINDOW_COUNT],
    history_len: u8,
    history_cursor: u8,
}

impl PpmEstimator {
    /// Creates a validated fixed-storage estimator.
    pub fn new(config: PpmEstimatorConfig) -> Result<Self, PpmEstimatorError> {
        if config.input_rate == 0
            || config.output_rate == 0
            || config.window_output_frames == 0
            || config.trusted_windows == 0
            || usize::from(config.trusted_windows) > MEDIAN_WINDOW_COUNT
            || !config.maximum_abs_ppm.is_finite()
            || config.maximum_abs_ppm <= 0.0
        {
            return Err(PpmEstimatorError::InvalidConfiguration);
        }
        Ok(Self {
            config,
            input_frames: 0,
            output_frames: 0,
            history: [0.0; MEDIAN_WINDOW_COUNT],
            history_len: 0,
            history_cursor: 0,
        })
    }

    /// Adds one frame-accounting observation.
    ///
    /// Set `discontinuity` for an XRUN, callback gap, reconnect, rate change, epoch change, or
    /// any event for which the two frame counters are no longer comparable. The partial window
    /// is discarded and the trust history is cleared fail-closed.
    pub fn observe(
        &mut self,
        input_frames: u64,
        output_frames: u64,
        discontinuity: bool,
    ) -> Result<Option<PpmEstimate>, PpmEstimatorError> {
        if discontinuity {
            self.reset();
            return Ok(None);
        }
        if input_frames == 0 || output_frames == 0 {
            return Err(PpmEstimatorError::InvalidConfiguration);
        }
        self.input_frames = self
            .input_frames
            .checked_add(input_frames)
            .ok_or(PpmEstimatorError::InvalidConfiguration)?;
        self.output_frames = self
            .output_frames
            .checked_add(output_frames)
            .ok_or(PpmEstimatorError::InvalidConfiguration)?;
        if self.output_frames < self.config.window_output_frames {
            return Ok(None);
        }

        let expected_input = self.output_frames as f64 * f64::from(self.config.input_rate)
            / f64::from(self.config.output_rate);
        let ppm = (self.input_frames as f64 / expected_input - 1.0) * 1_000_000.0;
        self.input_frames = 0;
        self.output_frames = 0;

        if !ppm.is_finite() || ppm.abs() > self.config.maximum_abs_ppm {
            self.clear_history();
            return Err(PpmEstimatorError::EstimateOutOfRange);
        }

        let index = usize::from(self.history_cursor);
        self.history[index] = ppm;
        self.history_cursor = (self.history_cursor + 1) % MEDIAN_WINDOW_COUNT as u8;
        self.history_len = self.history_len.saturating_add(1).min(MEDIAN_WINDOW_COUNT as u8);

        let filtered = self.filtered_ppm();
        Ok(Some(PpmEstimate {
            ppm: filtered,
            trusted: self.history_len >= self.config.trusted_windows,
            clean_windows: self.history_len,
        }))
    }

    /// Clears the partial measurement and trust history.
    pub fn reset(&mut self) {
        self.input_frames = 0;
        self.output_frames = 0;
        self.clear_history();
    }

    fn clear_history(&mut self) {
        self.history = [0.0; MEDIAN_WINDOW_COUNT];
        self.history_len = 0;
        self.history_cursor = 0;
    }

    fn filtered_ppm(&self) -> f64 {
        match self.history_len {
            0 => 0.0,
            1 => self.history[0],
            2 => (self.history[0] + self.history[1]) * 0.5,
            _ => median3(self.history[0], self.history[1], self.history[2]),
        }
    }
}

fn median3(a: f64, b: f64, c: f64) -> f64 {
    if a > b {
        if b > c {
            b
        } else if a > c {
            c
        } else {
            a
        }
    } else if a > c {
        a
    } else if b > c {
        c
    } else {
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimates_plus_and_minus_250_ppm_within_five_ppm_in_thirty_seconds() {
        for (input_per_window, expected_ppm) in [(480_120_u64, 250.0), (479_880, -250.0)] {
            let mut estimator = PpmEstimator::new(PpmEstimatorConfig::default()).unwrap();
            let mut final_estimate = None;
            for _ in 0..3 {
                final_estimate = estimator
                    .observe(input_per_window, 480_000, false)
                    .unwrap();
            }
            let report = final_estimate.unwrap();
            assert!(report.trusted);
            assert_eq!(report.clean_windows, 3);
            assert!((report.ppm - expected_ppm).abs() <= 5.0, "{report:?}");
        }
    }

    #[test]
    fn discontinuity_rejects_partial_window_and_restarts_trust() {
        let mut estimator = PpmEstimator::new(PpmEstimatorConfig::default()).unwrap();
        assert!(estimator.observe(240_060, 240_000, false).unwrap().is_none());
        assert!(estimator.observe(128, 128, true).unwrap().is_none());
        let first_clean = estimator.observe(480_120, 480_000, false).unwrap().unwrap();
        assert!(!first_clean.trusted);
        assert_eq!(first_clean.clean_windows, 1);
    }

    #[test]
    fn median_of_three_rejects_single_window_outlier() {
        let config = PpmEstimatorConfig {
            maximum_abs_ppm: 5_000.0,
            ..PpmEstimatorConfig::default()
        };
        let mut estimator = PpmEstimator::new(config).unwrap();
        estimator.observe(480_120, 480_000, false).unwrap();
        estimator.observe(481_200, 480_000, false).unwrap();
        let estimate = estimator.observe(480_120, 480_000, false).unwrap().unwrap();
        assert!(estimate.trusted);
        assert!((estimate.ppm - 250.0).abs() <= 5.0, "{estimate:?}");
    }
}
