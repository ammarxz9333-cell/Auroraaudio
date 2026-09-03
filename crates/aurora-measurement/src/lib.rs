//! Hardware-independent measurement primitives for Aurora calibration.
//!
//! These routines are deterministic offline analysis. Results from synthetic
//! inputs are `unit_test`/`deterministic_simulation` truth only; they are not
//! physical measurements until fed by an accepted real capture path.

pub mod remote_clock;
pub mod stimulus;

use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeasurementError {
    EmptySignal,
    InvalidSampleRate,
    InvalidLag,
    NoCorrelation,
    InsufficientDecayRange,
    InvalidMeasurement,
    UnsafeCalibration,
}

impl Display for MeasurementError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::EmptySignal => "measurement signal is empty",
            Self::InvalidSampleRate => "measurement sample rate is invalid",
            Self::InvalidLag => "measurement lag range is invalid",
            Self::NoCorrelation => "measurement correlation is undefined",
            Self::InsufficientDecayRange => "insufficient decay range for RT60 estimation",
            Self::InvalidMeasurement => "measurement contains invalid numeric values",
            Self::UnsafeCalibration => "measurement-derived calibration exceeds safety limits",
        };
        formatter.write_str(message)
    }
}

impl Error for MeasurementError {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChannelMeasurement {
    pub delay_frames: f32,
    pub level_dbfs: f32,
    pub rt60_seconds: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CalibrationAdjustment {
    /// Delay to add so all channels align to the latest measured arrival.
    pub added_delay_frames: f32,
    /// Gain adjustment in dB. The safe derivation never boosts a channel.
    pub gain_db: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CalibrationSafetyLimits {
    /// Maximum delay that an automatically staged preset may add.
    pub maximum_added_delay_frames: f32,
    /// Largest automatic attenuation allowed for one channel.
    pub maximum_cut_db: f32,
    /// Plausible RT60 interval used as a measurement sanity gate.
    pub minimum_rt60_seconds: f32,
    pub maximum_rt60_seconds: f32,
}

impl Default for CalibrationSafetyLimits {
    fn default() -> Self {
        Self {
            maximum_added_delay_frames: 4_800.0, // 100 ms at 48 kHz.
            maximum_cut_db: 12.0,
            minimum_rt60_seconds: 0.05,
            maximum_rt60_seconds: 3.0,
        }
    }
}

/// Returns the non-negative frame delay with the strongest normalized
/// correlation between a reference and a captured response.
pub fn estimate_delay_frames(
    reference: &[f32],
    captured: &[f32],
    max_lag_frames: usize,
) -> Result<usize, MeasurementError> {
    if reference.is_empty() || captured.is_empty() {
        return Err(MeasurementError::EmptySignal);
    }
    if max_lag_frames >= captured.len() {
        return Err(MeasurementError::InvalidLag);
    }

    let mut best_lag = 0usize;
    let mut best_score = f64::NEG_INFINITY;
    let mut found = false;

    for lag in 0..=max_lag_frames {
        let overlap = reference.len().min(captured.len() - lag);
        if overlap == 0 {
            continue;
        }
        let mut dot = 0.0_f64;
        let mut ref_energy = 0.0_f64;
        let mut cap_energy = 0.0_f64;
        for index in 0..overlap {
            let a = reference[index] as f64;
            let b = captured[index + lag] as f64;
            if !a.is_finite() || !b.is_finite() {
                return Err(MeasurementError::InvalidMeasurement);
            }
            dot += a * b;
            ref_energy += a * a;
            cap_energy += b * b;
        }
        let denominator = (ref_energy * cap_energy).sqrt();
        if denominator <= f64::EPSILON {
            continue;
        }
        let score = dot / denominator;
        if score > best_score {
            best_score = score;
            best_lag = lag;
            found = true;
        }
    }

    if found {
        Ok(best_lag)
    } else {
        Err(MeasurementError::NoCorrelation)
    }
}

/// RMS level relative to full scale. Digital silence returns negative infinity.
pub fn rms_dbfs(samples: &[f32]) -> Result<f32, MeasurementError> {
    if samples.is_empty() {
        return Err(MeasurementError::EmptySignal);
    }
    let mut energy = 0.0_f64;
    for sample in samples {
        if !sample.is_finite() {
            return Err(MeasurementError::InvalidMeasurement);
        }
        let value = *sample as f64;
        energy += value * value;
    }
    let rms = (energy / samples.len() as f64).sqrt();
    if rms <= f64::EPSILON {
        Ok(f32::NEG_INFINITY)
    } else {
        Ok((20.0 * rms.log10()) as f32)
    }
}

/// Estimates RT60 with Schroeder backward integration and a least-squares fit
/// over the -5 dB to -35 dB decay interval.
pub fn estimate_rt60_seconds(
    impulse_response: &[f32],
    sample_rate: u32,
) -> Result<f32, MeasurementError> {
    if impulse_response.is_empty() {
        return Err(MeasurementError::EmptySignal);
    }
    if sample_rate == 0 {
        return Err(MeasurementError::InvalidSampleRate);
    }

    let mut integrated = vec![0.0_f64; impulse_response.len()];
    let mut sum = 0.0_f64;
    for (index, sample) in impulse_response.iter().enumerate().rev() {
        if !sample.is_finite() {
            return Err(MeasurementError::InvalidMeasurement);
        }
        let value = *sample as f64;
        sum += value * value;
        integrated[index] = sum;
    }
    if sum <= f64::EPSILON {
        return Err(MeasurementError::InsufficientDecayRange);
    }

    let total = integrated[0];
    let mut count = 0_u64;
    let mut sum_t = 0.0_f64;
    let mut sum_db = 0.0_f64;
    let mut sum_tt = 0.0_f64;
    let mut sum_tdb = 0.0_f64;

    for (index, energy) in integrated.iter().copied().enumerate() {
        if energy <= f64::EPSILON {
            continue;
        }
        let db = 10.0 * (energy / total).log10();
        if !(-35.0..=-5.0).contains(&db) {
            continue;
        }
        let time = index as f64 / sample_rate as f64;
        count += 1;
        sum_t += time;
        sum_db += db;
        sum_tt += time * time;
        sum_tdb += time * db;
    }

    if count < 8 {
        return Err(MeasurementError::InsufficientDecayRange);
    }
    let n = count as f64;
    let denominator = n * sum_tt - sum_t * sum_t;
    if denominator.abs() <= f64::EPSILON {
        return Err(MeasurementError::InsufficientDecayRange);
    }
    let slope_db_per_second = (n * sum_tdb - sum_t * sum_db) / denominator;
    if !slope_db_per_second.is_finite() || slope_db_per_second >= -f64::EPSILON {
        return Err(MeasurementError::InsufficientDecayRange);
    }
    let rt60 = -60.0 / slope_db_per_second;
    if !rt60.is_finite() || rt60 <= 0.0 {
        return Err(MeasurementError::InsufficientDecayRange);
    }
    Ok(rt60 as f32)
}

/// Derives delay/level alignment without any positive gain. This is deliberately
/// conservative: the latest-arriving channel defines time zero and the quietest
/// measured channel defines the level target.
pub fn derive_safe_time_level_alignment(
    measurements: &[ChannelMeasurement],
) -> Result<Vec<CalibrationAdjustment>, MeasurementError> {
    if measurements.is_empty() {
        return Err(MeasurementError::EmptySignal);
    }
    let mut latest_delay = f32::NEG_INFINITY;
    let mut quietest_level = f32::INFINITY;
    for measurement in measurements {
        if !measurement.delay_frames.is_finite()
            || measurement.delay_frames < 0.0
            || !measurement.level_dbfs.is_finite()
            || !measurement.rt60_seconds.is_finite()
            || measurement.rt60_seconds < 0.0
        {
            return Err(MeasurementError::InvalidMeasurement);
        }
        latest_delay = latest_delay.max(measurement.delay_frames);
        quietest_level = quietest_level.min(measurement.level_dbfs);
    }

    Ok(measurements
        .iter()
        .map(|measurement| CalibrationAdjustment {
            added_delay_frames: (latest_delay - measurement.delay_frames).max(0.0),
            gain_db: (quietest_level - measurement.level_dbfs).min(0.0),
        })
        .collect())
}

/// Produces an automatically stageable time/level preset only when every
/// measurement and derived adjustment stays inside conservative safety limits.
/// The caller is still responsible for transactional apply/rollback; this
/// function deliberately never touches live DSP state itself.
pub fn derive_guarded_time_level_alignment(
    measurements: &[ChannelMeasurement],
    limits: CalibrationSafetyLimits,
) -> Result<Vec<CalibrationAdjustment>, MeasurementError> {
    if !limits.maximum_added_delay_frames.is_finite()
        || limits.maximum_added_delay_frames < 0.0
        || !limits.maximum_cut_db.is_finite()
        || limits.maximum_cut_db < 0.0
        || !limits.minimum_rt60_seconds.is_finite()
        || !limits.maximum_rt60_seconds.is_finite()
        || limits.minimum_rt60_seconds < 0.0
        || limits.maximum_rt60_seconds <= limits.minimum_rt60_seconds
    {
        return Err(MeasurementError::InvalidMeasurement);
    }
    for measurement in measurements {
        if !measurement.rt60_seconds.is_finite()
            || measurement.rt60_seconds < limits.minimum_rt60_seconds
            || measurement.rt60_seconds > limits.maximum_rt60_seconds
        {
            return Err(MeasurementError::UnsafeCalibration);
        }
    }
    let adjustments = derive_safe_time_level_alignment(measurements)?;
    if adjustments.iter().any(|adjustment| {
        adjustment.added_delay_frames > limits.maximum_added_delay_frames
            || adjustment.gain_db < -limits.maximum_cut_db
    }) {
        return Err(MeasurementError::UnsafeCalibration);
    }
    Ok(adjustments)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correlation_recovers_known_positive_delay() {
        let mut reference = vec![0.0_f32; 256];
        reference[20] = 1.0;
        reference[55] = -0.4;
        reference[80] = 0.25;
        let delay = 37usize;
        let mut captured = vec![0.0_f32; 512];
        for (index, sample) in reference.iter().copied().enumerate() {
            captured[index + delay] = sample;
        }
        assert_eq!(
            estimate_delay_frames(&reference, &captured, 100).unwrap(),
            delay
        );
    }

    #[test]
    fn rms_reports_known_half_scale_level() {
        let samples = vec![0.5_f32; 1_000];
        let level = rms_dbfs(&samples).unwrap();
        assert!((level + 6.0206).abs() < 0.001);
    }

    #[test]
    fn schroeder_fit_recovers_synthetic_rt60() {
        let sample_rate = 48_000_u32;
        let target_rt60 = 0.60_f32;
        let frames = sample_rate as usize * 2;
        let impulse = (0..frames)
            .map(|index| {
                let t = index as f32 / sample_rate as f32;
                10.0_f32.powf(-3.0 * t / target_rt60)
            })
            .collect::<Vec<_>>();
        let estimated = estimate_rt60_seconds(&impulse, sample_rate).unwrap();
        assert!((estimated - target_rt60).abs() < 0.02, "{estimated}");
    }

    #[test]
    fn alignment_adds_delay_and_only_cuts_level() {
        let measurements = [
            ChannelMeasurement {
                delay_frames: 10.0,
                level_dbfs: -20.0,
                rt60_seconds: 0.4,
            },
            ChannelMeasurement {
                delay_frames: 20.0,
                level_dbfs: -26.0,
                rt60_seconds: 0.4,
            },
        ];
        let adjustments = derive_safe_time_level_alignment(&measurements).unwrap();
        assert_eq!(adjustments[0].added_delay_frames, 10.0);
        assert_eq!(adjustments[1].added_delay_frames, 0.0);
        assert_eq!(adjustments[0].gain_db, -6.0);
        assert_eq!(adjustments[1].gain_db, 0.0);
        assert!(adjustments
            .iter()
            .all(|adjustment| adjustment.gain_db <= 0.0));
    }

    #[test]
    fn guarded_alignment_rejects_implausible_or_excessive_adjustments() {
        let safe = [
            ChannelMeasurement {
                delay_frames: 10.0,
                level_dbfs: -20.0,
                rt60_seconds: 0.4,
            },
            ChannelMeasurement {
                delay_frames: 100.0,
                level_dbfs: -24.0,
                rt60_seconds: 0.5,
            },
        ];
        assert!(
            derive_guarded_time_level_alignment(&safe, CalibrationSafetyLimits::default()).is_ok()
        );

        let unsafe_level = [
            ChannelMeasurement {
                delay_frames: 0.0,
                level_dbfs: -5.0,
                rt60_seconds: 0.4,
            },
            ChannelMeasurement {
                delay_frames: 0.0,
                level_dbfs: -30.0,
                rt60_seconds: 0.4,
            },
        ];
        assert_eq!(
            derive_guarded_time_level_alignment(&unsafe_level, CalibrationSafetyLimits::default()),
            Err(MeasurementError::UnsafeCalibration)
        );

        let unsafe_rt60 = [ChannelMeasurement {
            delay_frames: 0.0,
            level_dbfs: -20.0,
            rt60_seconds: 10.0,
        }];
        assert_eq!(
            derive_guarded_time_level_alignment(&unsafe_rt60, CalibrationSafetyLimits::default()),
            Err(MeasurementError::UnsafeCalibration)
        );
    }
}
