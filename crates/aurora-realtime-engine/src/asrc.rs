use rubato::{
    Resampler, SincFixedOut, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use thiserror::Error;

/// Adaptive resampler setup or processing error.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum AsrcError {
    /// Rates, channels, block size, or ratio are invalid.
    #[error("invalid asynchronous resampler configuration")]
    InvalidConfiguration,
    /// Input/output slices do not match the configured channel or frame shape.
    #[error("invalid asynchronous resampler buffer shape")]
    InvalidBufferShape,
    /// The requested ratio exceeds the configured adaptive range.
    #[error("asynchronous resampling ratio is outside the configured range: {0}")]
    RatioOutOfRange(f64),
    /// The selected implementation rejected an operation.
    #[error("asynchronous resampler processing failed")]
    Processing,
}

/// Result from one allocation-free asynchronous resampling block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AsrcProcessReport {
    /// Input frames consumed.
    pub input_frames: usize,
    /// Output frames produced.
    pub output_frames: usize,
}

/// Aurora-owned asynchronous resampler contract.
///
/// Implementations own all scratch allocated by `configure`. Methods called on
/// the audio path must not allocate, block, log, or expose third-party types.
pub trait AsynchronousResampler: Send {
    /// Configures rates, channel count, and fixed maximum output block size.
    fn configure(
        &mut self,
        input_rate: u32,
        output_rate: u32,
        channels: usize,
        max_block_size: usize,
    ) -> Result<(), AsrcError>;

    /// Smoothly updates the absolute output/input sampling ratio.
    fn set_ratio(&mut self, ratio: f64) -> Result<(), AsrcError>;

    /// Processes interleaved input into one configured-size interleaved block.
    fn process(
        &mut self,
        input: &[f32],
        output: &mut [f32],
    ) -> Result<AsrcProcessReport, AsrcError>;

    /// Returns input frames required for the next configured-size output block.
    fn required_input_frames(&self) -> usize;

    /// Returns bounded algorithmic latency in output frames.
    fn latency_frames(&self) -> usize;

    /// Clears history and restores the nominal configured ratio.
    fn reset(&mut self);
}

/// Rubato-backed band-limited asynchronous resampler.
///
/// The quality profile uses a 64-tap Blackman-Harris windowed sinc, 128
/// precomputed fractional positions, and linear interpolation between positions.
/// These constants trade approximately 32 output frames of latency for bounded
/// CPU suitable for the first live duplex implementation.
pub struct RubatoAsrc {
    inner: Option<SincFixedOut<f32>>,
    input_planar: Vec<Vec<f32>>,
    output_planar: Vec<Vec<f32>>,
    channels: usize,
    output_frames: usize,
    nominal_ratio: f64,
    maximum_relative_ratio: f64,
}

impl Default for RubatoAsrc {
    fn default() -> Self {
        Self {
            inner: None,
            input_planar: Vec::new(),
            output_planar: Vec::new(),
            channels: 0,
            output_frames: 0,
            nominal_ratio: 1.0,
            maximum_relative_ratio: 1.000_5,
        }
    }
}

impl RubatoAsrc {
    /// Creates an unconfigured Rubato adapter with a relative ratio range.
    pub fn with_maximum_relative_ratio(maximum_relative_ratio: f64) -> Self {
        Self {
            maximum_relative_ratio,
            ..Self::default()
        }
    }
}

impl AsynchronousResampler for RubatoAsrc {
    fn configure(
        &mut self,
        input_rate: u32,
        output_rate: u32,
        channels: usize,
        max_block_size: usize,
    ) -> Result<(), AsrcError> {
        if input_rate == 0
            || output_rate == 0
            || channels == 0
            || max_block_size < 2
            || !self.maximum_relative_ratio.is_finite()
            || self.maximum_relative_ratio < 1.0
        {
            return Err(AsrcError::InvalidConfiguration);
        }
        let nominal_ratio = f64::from(output_rate) / f64::from(input_rate);
        let parameters = SincInterpolationParameters {
            sinc_len: 64,
            f_cutoff: 0.95,
            oversampling_factor: 128,
            interpolation: SincInterpolationType::Linear,
            window: WindowFunction::BlackmanHarris2,
        };
        let inner = SincFixedOut::<f32>::new(
            nominal_ratio,
            self.maximum_relative_ratio,
            parameters,
            max_block_size,
            channels,
        )
        .map_err(|_| AsrcError::InvalidConfiguration)?;
        let maximum_input = inner.input_frames_max();
        self.input_planar = vec![vec![0.0; maximum_input]; channels];
        self.output_planar = vec![vec![0.0; max_block_size]; channels];
        self.channels = channels;
        self.output_frames = max_block_size;
        self.nominal_ratio = nominal_ratio;
        self.inner = Some(inner);
        Ok(())
    }

    fn set_ratio(&mut self, ratio: f64) -> Result<(), AsrcError> {
        if !ratio.is_finite() || ratio <= 0.0 {
            return Err(AsrcError::RatioOutOfRange(ratio));
        }
        self.inner
            .as_mut()
            .ok_or(AsrcError::InvalidConfiguration)?
            .set_resample_ratio(ratio, true)
            .map_err(|_| AsrcError::RatioOutOfRange(ratio))
    }

    fn process(
        &mut self,
        input: &[f32],
        output: &mut [f32],
    ) -> Result<AsrcProcessReport, AsrcError> {
        let required = self.required_input_frames();
        if self.channels == 0
            || input.len() != required.saturating_mul(self.channels)
            || output.len() != self.output_frames.saturating_mul(self.channels)
        {
            return Err(AsrcError::InvalidBufferShape);
        }
        for (frame_index, frame) in input.chunks_exact(self.channels).enumerate() {
            for (channel, sample) in frame.iter().enumerate() {
                self.input_planar[channel][frame_index] = *sample;
            }
        }
        let (consumed, produced) = self
            .inner
            .as_mut()
            .ok_or(AsrcError::InvalidConfiguration)?
            .process_into_buffer(&self.input_planar, &mut self.output_planar, None)
            .map_err(|_| AsrcError::Processing)?;
        if produced != self.output_frames {
            return Err(AsrcError::Processing);
        }
        for (frame_index, frame) in output.chunks_exact_mut(self.channels).enumerate() {
            for (channel, sample) in frame.iter_mut().enumerate() {
                *sample = self.output_planar[channel][frame_index];
            }
        }
        Ok(AsrcProcessReport {
            input_frames: consumed,
            output_frames: produced,
        })
    }

    fn required_input_frames(&self) -> usize {
        self.inner.as_ref().map_or(0, Resampler::input_frames_next)
    }

    fn latency_frames(&self) -> usize {
        self.inner.as_ref().map_or(0, Resampler::output_delay)
    }

    fn reset(&mut self) {
        if let Some(inner) = self.inner.as_mut() {
            inner.reset();
        }
        for channel in &mut self.input_planar {
            channel.fill(0.0);
        }
        for channel in &mut self.output_planar {
            channel.fill(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_ratio_is_finite_coherent_and_deterministic() {
        let first = run_identity();
        let second = run_identity();
        assert_eq!(first, second);
        assert!(first.iter().all(|sample| sample.is_finite()));
        for frame in first.chunks_exact(2) {
            assert!((frame[1] + frame[0]).abs() < 1.0e-6);
        }
    }

    #[test]
    fn silence_remains_silence_with_rate_mismatch() {
        let mut resampler = RubatoAsrc::default();
        resampler.configure(44_100, 48_000, 6, 256).unwrap();
        for _ in 0..8 {
            let input = vec![0.0; resampler.required_input_frames() * 6];
            let mut output = vec![1.0; 256 * 6];
            resampler.process(&input, &mut output).unwrap();
            assert!(output.iter().all(|sample| *sample == 0.0));
        }
    }

    fn run_identity() -> Vec<f32> {
        let mut resampler = RubatoAsrc::default();
        resampler.configure(48_000, 48_000, 2, 128).unwrap();
        let mut phase = 0.0_f32;
        let mut collected = Vec::new();
        for _ in 0..8 {
            let required = resampler.required_input_frames();
            let mut input = Vec::with_capacity(required * 2);
            for _ in 0..required {
                let sample = phase.sin() * 0.25;
                input.extend([sample, -sample]);
                phase += 0.1;
            }
            let mut output = vec![0.0; 128 * 2];
            resampler.process(&input, &mut output).unwrap();
            collected.extend(output);
        }
        collected
    }
}
