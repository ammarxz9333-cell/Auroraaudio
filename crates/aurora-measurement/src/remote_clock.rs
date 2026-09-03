use std::error::Error;
use std::fmt::{Display, Formatter};

const RATE_WINDOW: usize = 128;
const MIN_RATE_POINTS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteClockError {
    InvalidConfiguration,
    InvalidObservation,
    ExcessivePathDelay,
}

impl Display for RemoteClockError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::InvalidConfiguration => "invalid remote clock estimator configuration",
            Self::InvalidObservation => "invalid or non-monotonic remote clock observation",
            Self::ExcessivePathDelay => "remote clock observation exceeds path-delay limit",
        };
        formatter.write_str(message)
    }
}

impl Error for RemoteClockError {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RemoteClockConfig {
    pub maximum_round_trip_ns: u64,
    /// Correction applied to the predicted clock offset for each accepted observation.
    pub offset_gain: f64,
    /// Smoothing gain applied to the sliding-regression clock-rate estimate.
    pub rate_gain: f64,
    pub maximum_rate_ppm: f64,
}

impl Default for RemoteClockConfig {
    fn default() -> Self {
        Self {
            maximum_round_trip_ns: 20_000_000,
            offset_gain: 0.15,
            rate_gain: 0.01,
            maximum_rate_ppm: 1_000.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteClockObservation {
    pub local_send_ns: u64,
    pub remote_receive_ns: u64,
    pub remote_send_ns: u64,
    pub local_receive_ns: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RemoteClockReport {
    pub offset_ns: f64,
    pub rate_ppm: f64,
    pub round_trip_ns: u64,
    pub accepted_observations: u64,
    pub rejected_observations: u64,
}

#[derive(Debug, Clone, Copy, Default)]
struct RatePoint {
    local_midpoint_ns: u64,
    measured_offset_ns: f64,
}

#[derive(Debug, Clone)]
pub struct RemoteClockEstimator {
    config: RemoteClockConfig,
    initialized: bool,
    offset_ns: f64,
    rate_ppm: f64,
    last_local_midpoint_ns: u64,
    accepted_observations: u64,
    rejected_observations: u64,
    rate_points: [RatePoint; RATE_WINDOW],
    rate_head: usize,
    rate_len: usize,
}

impl RemoteClockEstimator {
    pub fn new(config: RemoteClockConfig) -> Result<Self, RemoteClockError> {
        if config.maximum_round_trip_ns == 0
            || !config.offset_gain.is_finite()
            || config.offset_gain <= 0.0
            || config.offset_gain > 1.0
            || !config.rate_gain.is_finite()
            || config.rate_gain <= 0.0
            || config.rate_gain > 1.0
            || !config.maximum_rate_ppm.is_finite()
            || config.maximum_rate_ppm <= 0.0
        {
            return Err(RemoteClockError::InvalidConfiguration);
        }
        Ok(Self {
            config,
            initialized: false,
            offset_ns: 0.0,
            rate_ppm: 0.0,
            last_local_midpoint_ns: 0,
            accepted_observations: 0,
            rejected_observations: 0,
            rate_points: [RatePoint::default(); RATE_WINDOW],
            rate_head: 0,
            rate_len: 0,
        })
    }

    fn push_rate_point(&mut self, point: RatePoint) {
        let write = (self.rate_head + self.rate_len) % RATE_WINDOW;
        self.rate_points[write] = point;
        if self.rate_len < RATE_WINDOW {
            self.rate_len += 1;
        } else {
            self.rate_head = (self.rate_head + 1) % RATE_WINDOW;
        }
    }

    fn regression_rate_ppm(&self) -> Option<f64> {
        if self.rate_len < MIN_RATE_POINTS {
            return None;
        }
        let base = self.rate_points[self.rate_head].local_midpoint_ns;
        let n = self.rate_len as f64;
        let mut sum_x = 0.0_f64;
        let mut sum_y = 0.0_f64;
        let mut sum_xx = 0.0_f64;
        let mut sum_xy = 0.0_f64;

        for index in 0..self.rate_len {
            let point = self.rate_points[(self.rate_head + index) % RATE_WINDOW];
            // Center time at the oldest sample to preserve f64 precision even
            // after a long device uptime. x is seconds; y is nanoseconds.
            let x = (point.local_midpoint_ns - base) as f64 / 1_000_000_000.0;
            let y = point.measured_offset_ns;
            sum_x += x;
            sum_y += y;
            sum_xx += x * x;
            sum_xy += x * y;
        }
        let denominator = n * sum_xx - sum_x * sum_x;
        if !denominator.is_finite() || denominator.abs() <= f64::EPSILON {
            return None;
        }
        let slope_ns_per_second = (n * sum_xy - sum_x * sum_y) / denominator;
        if !slope_ns_per_second.is_finite() {
            return None;
        }
        // One ppm produces 1,000 ns of offset change per second.
        Some((slope_ns_per_second / 1_000.0).clamp(
            -self.config.maximum_rate_ppm,
            self.config.maximum_rate_ppm,
        ))
    }

    pub fn observe(
        &mut self,
        observation: RemoteClockObservation,
    ) -> Result<RemoteClockReport, RemoteClockError> {
        if observation.local_receive_ns < observation.local_send_ns
            || observation.remote_send_ns < observation.remote_receive_ns
        {
            self.rejected_observations = self.rejected_observations.saturating_add(1);
            return Err(RemoteClockError::InvalidObservation);
        }

        let local_span = observation.local_receive_ns - observation.local_send_ns;
        let remote_residence = observation.remote_send_ns - observation.remote_receive_ns;
        if local_span < remote_residence {
            self.rejected_observations = self.rejected_observations.saturating_add(1);
            return Err(RemoteClockError::InvalidObservation);
        }
        let round_trip_ns = local_span - remote_residence;
        if round_trip_ns > self.config.maximum_round_trip_ns {
            self.rejected_observations = self.rejected_observations.saturating_add(1);
            return Err(RemoteClockError::ExcessivePathDelay);
        }

        let local_midpoint_ns = observation.local_send_ns
            + (observation.local_receive_ns - observation.local_send_ns) / 2;
        let remote_midpoint_ns = observation.remote_receive_ns
            + (observation.remote_send_ns - observation.remote_receive_ns) / 2;
        let measured_offset_ns = remote_midpoint_ns as f64 - local_midpoint_ns as f64;

        if self.initialized && local_midpoint_ns <= self.last_local_midpoint_ns {
            self.rejected_observations = self.rejected_observations.saturating_add(1);
            return Err(RemoteClockError::InvalidObservation);
        }

        self.push_rate_point(RatePoint {
            local_midpoint_ns,
            measured_offset_ns,
        });

        if !self.initialized {
            self.initialized = true;
            self.offset_ns = measured_offset_ns;
            self.rate_ppm = 0.0;
        } else {
            if let Some(estimated_rate_ppm) = self.regression_rate_ppm() {
                self.rate_ppm +=
                    self.config.rate_gain * (estimated_rate_ppm - self.rate_ppm);
                self.rate_ppm = self.rate_ppm.clamp(
                    -self.config.maximum_rate_ppm,
                    self.config.maximum_rate_ppm,
                );
            }
            let elapsed_ns = (local_midpoint_ns - self.last_local_midpoint_ns) as f64;
            let predicted_offset_ns =
                self.offset_ns + self.rate_ppm * elapsed_ns / 1_000_000.0;
            let residual_ns = measured_offset_ns - predicted_offset_ns;
            self.offset_ns = predicted_offset_ns + self.config.offset_gain * residual_ns;
        }
        self.last_local_midpoint_ns = local_midpoint_ns;

        self.accepted_observations = self.accepted_observations.saturating_add(1);
        Ok(RemoteClockReport {
            offset_ns: self.offset_ns,
            rate_ppm: self.rate_ppm,
            round_trip_ns,
            accepted_observations: self.accepted_observations,
            rejected_observations: self.rejected_observations,
        })
    }

    pub fn remote_time_for_local_ns(&self, local_ns: u64) -> Option<f64> {
        if !self.initialized {
            return None;
        }
        let delta_ns = local_ns as f64 - self.last_local_midpoint_ns as f64;
        Some(local_ns as f64 + self.offset_ns + self.rate_ppm * delta_ns / 1_000_000.0)
    }

    pub fn reset(&mut self) {
        self.initialized = false;
        self.offset_ns = 0.0;
        self.rate_ppm = 0.0;
        self.last_local_midpoint_ns = 0;
        self.accepted_observations = 0;
        self.rejected_observations = 0;
        self.rate_points.fill(RatePoint::default());
        self.rate_head = 0;
        self.rate_len = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote_time(local_ns: u64, offset_ns: f64, rate_ppm: f64) -> u64 {
        (local_ns as f64 * (1.0 + rate_ppm / 1_000_000.0) + offset_ns).round() as u64
    }

    #[test]
    fn rejects_excessive_path_delay() {
        let mut estimator = RemoteClockEstimator::new(RemoteClockConfig {
            maximum_round_trip_ns: 1_000_000,
            ..RemoteClockConfig::default()
        })
        .unwrap();
        let result = estimator.observe(RemoteClockObservation {
            local_send_ns: 0,
            remote_receive_ns: 1_000_000,
            remote_send_ns: 1_000_100,
            local_receive_ns: 10_000_000,
        });
        assert_eq!(result, Err(RemoteClockError::ExcessivePathDelay));
    }

    #[test]
    fn converges_on_deterministic_offset_and_rate() {
        let offset_ns = 3_000_000.0;
        let rate_ppm = 80.0;
        let mut estimator = RemoteClockEstimator::new(RemoteClockConfig {
            maximum_round_trip_ns: 5_000_000,
            offset_gain: 0.20,
            rate_gain: 0.05,
            ..RemoteClockConfig::default()
        })
        .unwrap();
        let mut last = None;
        for index in 0..2_000_u64 {
            let local_send = index * 20_000_000;
            let forward_delay = 400_000 + (index % 7) * 20_000;
            let reverse_delay = 420_000 + (index % 5) * 15_000;
            let remote_receive = remote_time(local_send + forward_delay, offset_ns, rate_ppm);
            let remote_send = remote_receive + 100_000;
            let local_receive = local_send + forward_delay + 100_000 + reverse_delay;
            last = Some(
                estimator
                    .observe(RemoteClockObservation {
                        local_send_ns: local_send,
                        remote_receive_ns: remote_receive,
                        remote_send_ns: remote_send,
                        local_receive_ns: local_receive,
                    })
                    .unwrap(),
            );
        }
        let report = last.unwrap();
        assert!((report.rate_ppm - rate_ppm).abs() < 5.0, "{report:?}");
        let future_local = 45_000_000_000_u64;
        let expected = remote_time(future_local, offset_ns, rate_ppm) as f64;
        let estimated = estimator.remote_time_for_local_ns(future_local).unwrap();
        assert!((estimated - expected).abs() < 500_000.0, "estimated={estimated} expected={expected}");
    }

    #[test]
    fn reset_discards_rate_history() {
        let mut estimator = RemoteClockEstimator::new(RemoteClockConfig::default()).unwrap();
        estimator
            .observe(RemoteClockObservation {
                local_send_ns: 1_000_000,
                remote_receive_ns: 2_000_000,
                remote_send_ns: 2_100_000,
                local_receive_ns: 3_100_000,
            })
            .unwrap();
        estimator.reset();
        assert!(estimator.remote_time_for_local_ns(4_000_000).is_none());
        assert_eq!(estimator.rate_len, 0);
    }
}
