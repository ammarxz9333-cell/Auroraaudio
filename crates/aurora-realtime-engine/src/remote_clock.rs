//! Hardware-independent remote-clock estimation for future wireless endpoints.
//!
//! The estimator consumes NTP-style four-timestamp observations, rejects high
//! path-delay samples, and tracks offset plus rate error with a bounded alpha-
//! beta filter. It contains no networking and makes no physical-sync claim.

use thiserror::Error;

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum RemoteClockError {
    #[error("invalid remote clock estimator configuration")]
    InvalidConfiguration,
    #[error("invalid or non-monotonic remote clock observation")]
    InvalidObservation,
    #[error("remote clock observation exceeds path-delay limit")]
    ExcessivePathDelay,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RemoteClockConfig {
    /// Maximum accepted NTP-style round-trip path delay.
    pub maximum_round_trip_ns: u64,
    /// Offset residual gain in the alpha-beta filter.
    pub offset_gain: f64,
    /// Rate residual gain in the alpha-beta filter.
    pub rate_gain: f64,
    /// Maximum tracked absolute clock-rate error.
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

/// Four timestamps from one two-way clock exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteClockObservation {
    pub local_send_ns: u64,
    pub remote_receive_ns: u64,
    pub remote_send_ns: u64,
    pub local_receive_ns: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RemoteClockReport {
    /// Estimated remote-minus-local offset at the observation midpoint.
    pub offset_ns: f64,
    /// Estimated remote clock rate relative to local in ppm.
    pub rate_ppm: f64,
    /// NTP-style network round trip after subtracting remote residence time.
    pub round_trip_ns: u64,
    pub accepted_observations: u64,
    pub rejected_observations: u64,
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
}

impl RemoteClockEstimator {
    pub fn new(config: RemoteClockConfig) -> Result<Self, RemoteClockError> {
        if config.maximum_round_trip_ns == 0
            || !config.offset_gain.is_finite()
            || config.offset_gain <= 0.0
            || config.offset_gain > 1.0
            || !config.rate_gain.is_finite()
            || config.rate_gain < 0.0
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
        })
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

        if !self.initialized {
            self.initialized = true;
            self.offset_ns = measured_offset_ns;
            self.rate_ppm = 0.0;
            self.last_local_midpoint_ns = local_midpoint_ns;
        } else {
            if local_midpoint_ns <= self.last_local_midpoint_ns {
                self.rejected_observations = self.rejected_observations.saturating_add(1);
                return Err(RemoteClockError::InvalidObservation);
            }
            let elapsed_ns = (local_midpoint_ns - self.last_local_midpoint_ns) as f64;
            let predicted_offset_ns = self.offset_ns + self.rate_ppm * elapsed_ns / 1_000_000.0;
            let residual_ns = measured_offset_ns - predicted_offset_ns;
            self.offset_ns = predicted_offset_ns + self.config.offset_gain * residual_ns;
            let residual_rate_ppm = residual_ns / elapsed_ns * 1_000_000.0;
            self.rate_ppm = (self.rate_ppm + self.config.rate_gain * residual_rate_ppm).clamp(
                -self.config.maximum_rate_ppm,
                self.config.maximum_rate_ppm,
            );
            self.last_local_midpoint_ns = local_midpoint_ns;
        }

        self.accepted_observations = self.accepted_observations.saturating_add(1);
        Ok(RemoteClockReport {
            offset_ns: self.offset_ns,
            rate_ppm: self.rate_ppm,
            round_trip_ns,
            accepted_observations: self.accepted_observations,
            rejected_observations: self.rejected_observations,
        })
    }

    /// Maps a future local monotonic timestamp into estimated remote time.
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
}
