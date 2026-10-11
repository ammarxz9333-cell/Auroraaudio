//! Bounded, deterministic offline audio impulse-response deconvolution.
//!
//! This is host-side DSP, never the realtime audio callback. Captured audio
//! must come from a calibrated, explicit, user-consented microphone frontend.
//! Regularized spectral division estimates transfer response; it cannot
//! identify which surface reflected a measured echo.
use std::f64::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureDspError {
    InvalidFormat,
    InvalidSignal,
    TooLarge,
    NoArrival,
}

#[derive(Debug, Clone)]
pub struct ImpulseEstimate {
    /// Truncated transfer response starting at the estimated first arrival.
    pub impulse: Vec<f32>,
    /// Earliest detected arrival in the original capture timebase.
    pub onset_frames: usize,
    /// Observed fraction of near-full-scale recorded samples.
    pub clipped_fraction: f32,
}

/// 48 kHz, mono, bounded capture from a raw microphone route.
/// The host must independently verify phone recorder mode and gain stability.
pub fn estimate_impulse_response(
    reference: &[f32],
    recorded: &[f32],
    sample_rate: u32,
) -> Result<ImpulseEstimate, CaptureDspError> {
    if sample_rate != 48_000
        || reference.len() < 256
        || reference.len() > 240_000
        || recorded.len() < reference.len() + 9_600
        || recorded.len() > 480_000
    {
        return Err(CaptureDspError::InvalidFormat);
    }
    if reference.iter().chain(recorded.iter()).any(|x| !x.is_finite()) {
        return Err(CaptureDspError::InvalidSignal);
    }
    let clipped = recorded.iter().filter(|x| x.abs() >= 0.999).count();
    let peak = reference.iter().fold(0.0_f32, |a, b| a.max(b.abs()));
    if peak < 0.001 {
        return Err(CaptureDspError::InvalidSignal);
    }
    let len = reference
        .len()
        .checked_add(recorded.len())
        .ok_or(CaptureDspError::TooLarge)?
        .next_power_of_two();
    if len > 1 << 20 {
        return Err(CaptureDspError::TooLarge);
    }

    let mut x = vec![Complex::zero(); len];
    let mut y = vec![Complex::zero(); len];
    for (target, value) in x.iter_mut().zip(reference.iter()) {
        target.re = f64::from(*value);
    }
    for (target, value) in y.iter_mut().zip(recorded.iter()) {
        target.re = f64::from(*value);
    }
    fft(&mut x, false);
    fft(&mut y, false);
    let peak_power = x
        .iter()
        .fold(0.0_f64, |acc, z| acc.max(z.power()));
    // Regularization avoids divergent estimates around sweep spectral nulls.
    // This is an experimental, bounded reference, not a calibrated RIR solver.
    let regularization = peak_power * 1.0e-9;
    if peak_power < 1.0e-14 {
        return Err(CaptureDspError::InvalidSignal);
    }
    for (frequency, reference_bin) in y.iter_mut().zip(x.iter()) {
        let numerator = *frequency * reference_bin.conjugate();
        *frequency = numerator / (reference_bin.power() + regularization);
    }
    fft(&mut y, true);
    let analysis_len = recorded.len().min(y.len());
    let peak_ir = y[..analysis_len].iter().fold(0.0_f64, |acc, z| acc.max(z.re.abs()));
    if !peak_ir.is_finite() || peak_ir < 1.0e-5 {
        return Err(CaptureDspError::NoArrival);
    }
    // First plausible arrival, not necessarily the largest later reflection.
    // Rejects a path without at least 200 ms of response after arrival.
    let threshold = (peak_ir * 0.12).max(1.0e-5);
    let max_onset = analysis_len - 9_600;
    let onset_frames = y[..=max_onset]
        .iter()
        .position(|z| z.re.abs() >= threshold)
        .ok_or(CaptureDspError::NoArrival)?;
    let impulse = y[onset_frames..onset_frames + 9_600]
        .iter()
        .map(|z| z.re as f32)
        .collect();
    Ok(ImpulseEstimate {
        impulse,
        onset_frames,
        clipped_fraction: clipped as f32 / recorded.len() as f32,
    })
}

#[derive(Debug, Clone, Copy, Default)]
struct Complex {
    re: f64,
    im: f64,
}

impl Complex {
    fn zero() -> Self {
        Self::default()
    }
    fn conjugate(self) -> Self {
        Self { re: self.re, im: -self.im }
    }
    fn power(self) -> f64 {
        self.re * self.re + self.im * self.im
    }
}

impl std::ops::Add for Complex {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self { re: self.re + rhs.re, im: self.im + rhs.im }
    }
}
impl std::ops::Sub for Complex {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self { re: self.re - rhs.re, im: self.im - rhs.im }
    }
}
impl std::ops::Mul for Complex {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self {
            re: self.re * rhs.re - self.im * rhs.im,
            im: self.re * rhs.im + self.im * rhs.re,
        }
    }
}
impl std::ops::Div<f64> for Complex {
    type Output = Self;
    fn div(self, rhs: f64) -> Self {
        Self { re: self.re / rhs, im: self.im / rhs }
    }
}

/// Iterative radix-2 FFT, inverse is normalized by n.
fn fft(values: &mut [Complex], inverse: bool) {
    let n = values.len();
    debug_assert!(n.is_power_of_two());
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            values.swap(i, j);
        }
    }
    let mut size = 2;
    while size <= n {
        let a = if inverse { 2.0 } else { -2.0 } * PI / size as f64;
        let root = Complex { re: a.cos(), im: a.sin() };
        for offset in (0..n).step_by(size) {
            let mut factor = Complex { re: 1.0, im: 0.0 };
            for k in 0..size / 2 {
                let even = values[offset + k];
                let odd = values[offset + k + size / 2] * factor;
                values[offset + k] = even + odd;
                values[offset + k + size / 2] = even - odd;
                factor = factor * root;
            }
        }
        size *= 2;
    }
    if inverse {
        for z in values.iter_mut() {
            *z = *z / n as f64;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stimulus::{generate_log_sweep, LogSweepConfig};

    #[test]
    fn synthetic_sweep_recovers_two_echoes_and_direct_first() {
        let reference = generate_log_sweep(LogSweepConfig {
            duration_seconds: 0.03,
            start_hz: 300.0,
            end_hz: 12_000.0,
            ..LogSweepConfig::default()
        }).unwrap();
        let mut captured = vec![0.0_f32; reference.len() + 12_000];
        for (index, sample) in reference.iter().enumerate() {
            captured[index + 1_000] += 0.8 * sample;
            captured[index + 1_600] += 0.5 * sample;
        }
        let estimated = estimate_impulse_response(&reference, &captured, 48_000).unwrap();
        assert!((estimated.onset_frames as i64 - 1_000).abs() <= 2, "{estimated:?}");
        assert!(estimated.impulse[0].abs() > 0.3);
        assert!(estimated.impulse[600].abs() > 0.15);
        assert_eq!(estimated.clipped_fraction, 0.0);
    }

    #[test]
    fn rejects_invalid_and_nan_recordings() {
        let reference = vec![0.1; 256];
        let mut capture = vec![0.0; 12_000];
        assert_eq!(estimate_impulse_response(&reference, &capture, 44_100).unwrap_err(), CaptureDspError::InvalidFormat);
        capture[2] = f32::NAN;
        assert_eq!(estimate_impulse_response(&reference, &capture, 48_000).unwrap_err(), CaptureDspError::InvalidSignal);
    }
}
