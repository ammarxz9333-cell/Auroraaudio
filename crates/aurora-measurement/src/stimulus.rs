use super::MeasurementError;
use std::f32::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogSweepConfig {
    pub sample_rate: u32,
    pub duration_seconds: f32,
    pub start_hz: f32,
    pub end_hz: f32,
    pub amplitude: f32,
    /// Raised-cosine fade duration at both ends. Clamped to half the sweep.
    pub fade_seconds: f32,
}

impl Default for LogSweepConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            duration_seconds: 5.0,
            start_hz: 20.0,
            end_hz: 20_000.0,
            amplitude: 0.25,
            fade_seconds: 0.02,
        }
    }
}

/// Generates a deterministic exponential sine sweep suitable for offline room
/// measurement. The function allocates only its returned buffer and does not
/// claim anything about a physical microphone or speaker path.
pub fn generate_log_sweep(config: LogSweepConfig) -> Result<Vec<f32>, MeasurementError> {
    if config.sample_rate == 0
        || !config.duration_seconds.is_finite()
        || config.duration_seconds <= 0.0
        || !config.start_hz.is_finite()
        || !config.end_hz.is_finite()
        || config.start_hz <= 0.0
        || config.end_hz <= config.start_hz
        || config.end_hz >= config.sample_rate as f32 * 0.49
        || !config.amplitude.is_finite()
        || config.amplitude <= 0.0
        || config.amplitude > 1.0
        || !config.fade_seconds.is_finite()
        || config.fade_seconds < 0.0
    {
        return Err(MeasurementError::InvalidMeasurement);
    }

    let frame_count = (config.duration_seconds * config.sample_rate as f32).round() as usize;
    if frame_count < 2 {
        return Err(MeasurementError::InvalidMeasurement);
    }
    let duration = frame_count as f64 / config.sample_rate as f64;
    let start = config.start_hz as f64;
    let ratio = config.end_hz as f64 / start;
    let log_ratio = ratio.ln();
    let phase_scale = 2.0 * std::f64::consts::PI * start * duration / log_ratio;
    let fade_frames = ((config.fade_seconds * config.sample_rate as f32).round() as usize)
        .min(frame_count / 2);

    let mut output = Vec::with_capacity(frame_count);
    for index in 0..frame_count {
        let t = index as f64 / config.sample_rate as f64;
        let phase = phase_scale * ((log_ratio * t / duration).exp() - 1.0);
        let mut envelope = 1.0_f32;
        if fade_frames > 0 && index < fade_frames {
            let x = index as f32 / fade_frames as f32;
            envelope = 0.5 - 0.5 * (PI * x).cos();
        } else if fade_frames > 0 && index >= frame_count - fade_frames {
            let remaining = (frame_count - 1 - index) as f32 / fade_frames as f32;
            envelope = 0.5 - 0.5 * (PI * remaining.max(0.0)).cos();
        }
        output.push((phase.sin() as f32) * config.amplitude * envelope);
    }
    Ok(output)
}

/// Deterministic excitation used when a broadband noise burst is more useful
/// than a sweep (for example a decay-tail/RT60 capture). This is intentionally
/// reproducible so unit tests and later physical captures can share fixtures.
pub fn generate_noise_burst(
    sample_rate: u32,
    duration_seconds: f32,
    amplitude: f32,
    seed: u64,
) -> Result<Vec<f32>, MeasurementError> {
    if sample_rate == 0
        || !duration_seconds.is_finite()
        || duration_seconds <= 0.0
        || !amplitude.is_finite()
        || amplitude <= 0.0
        || amplitude > 1.0
    {
        return Err(MeasurementError::InvalidMeasurement);
    }
    let frames = (duration_seconds * sample_rate as f32).round() as usize;
    if frames == 0 {
        return Err(MeasurementError::InvalidMeasurement);
    }
    let mut state = if seed == 0 { 0x9e37_79b9_7f4a_7c15 } else { seed };
    let mut output = Vec::with_capacity(frames);
    for _ in 0..frames {
        // xorshift64*: compact deterministic source; this is a measurement
        // stimulus generator, not a cryptographic random-number generator.
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let value = state.wrapping_mul(0x2545_f491_4f6c_dd1d);
        let normalized = ((value >> 40) as i32 - 8_388_608) as f32 / 8_388_608.0;
        output.push(normalized * amplitude);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_sweep_is_bounded_finite_and_exact_length() {
        let config = LogSweepConfig {
            duration_seconds: 0.25,
            ..LogSweepConfig::default()
        };
        let sweep = generate_log_sweep(config).unwrap();
        assert_eq!(sweep.len(), 12_000);
        assert!(sweep.iter().all(|sample| sample.is_finite()));
        assert!(sweep.iter().all(|sample| sample.abs() <= config.amplitude + 1.0e-6));
        assert!(sweep[0].abs() < 1.0e-7);
    }

    #[test]
    fn noise_burst_is_reproducible_and_bounded() {
        let first = generate_noise_burst(48_000, 0.1, 0.2, 1234).unwrap();
        let second = generate_noise_burst(48_000, 0.1, 0.2, 1234).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.len(), 4_800);
        assert!(first.iter().all(|sample| sample.is_finite() && sample.abs() <= 0.2));
    }
}
