//! Realtime-safe output DSP blocks for Aurora's loudspeaker product path.
//!
//! Configuration may allocate. Steady-state processing uses caller-owned audio
//! buffers plus state allocated during construction/configuration.

use std::f32::consts::PI;

use thiserror::Error;

const BUTTERWORTH_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// Errors returned by output DSP configuration and processing.
#[derive(Debug, Error, PartialEq)]
pub enum OutputDspError {
    /// Sample rate must be non-zero.
    #[error("sample rate must be greater than zero")]
    InvalidSampleRate,
    /// Channel count must be non-zero.
    #[error("channel count must be greater than zero")]
    InvalidChannelCount,
    /// Requested LFE channel is outside the configured channel range.
    #[error("lfe channel {lfe_index} is outside {channel_count} channels")]
    InvalidLfeIndex {
        /// Requested LFE channel index.
        lfe_index: usize,
        /// Configured channel count.
        channel_count: usize,
    },
    /// Processing block contains the wrong number of channels.
    #[error("expected {expected} channels, got {actual}")]
    ChannelCount {
        /// Configured channel count.
        expected: usize,
        /// Supplied channel count.
        actual: usize,
    },
    /// Processing block exceeds the configured maximum frame count.
    #[error("requested {actual} frames exceeds configured maximum {maximum}")]
    FrameCapacity {
        /// Configured maximum frame count.
        maximum: usize,
        /// Requested frame count.
        actual: usize,
    },
    /// One channel is shorter than the requested processing frame count.
    #[error("channel {channel} has {actual} frames, needs {required}")]
    BufferFrames {
        /// Channel index.
        channel: usize,
        /// Required frames.
        required: usize,
        /// Available frames.
        actual: usize,
    },
    /// A frequency lies outside the stable digital-filter range.
    #[error("frequency {frequency_hz} Hz is invalid for {sample_rate} Hz sample rate")]
    InvalidFrequency {
        /// Rejected frequency.
        frequency_hz: f32,
        /// Sample rate.
        sample_rate: u32,
    },
    /// Filter Q must be finite and greater than zero.
    #[error("filter Q must be finite and greater than zero, got {q}")]
    InvalidQ {
        /// Rejected Q value.
        q: f32,
    },
    /// A dB gain parameter must be finite.
    #[error("gain must be finite, got {gain_db} dB")]
    InvalidGain {
        /// Rejected gain.
        gain_db: f32,
    },
    /// Limiter threshold must be finite and in `(0, 1]`.
    #[error("limiter threshold must be finite and in (0, 1], got {threshold}")]
    InvalidLimiterThreshold {
        /// Rejected linear threshold.
        threshold: f32,
    },
    /// Limiter release time must be finite and greater than zero.
    #[error("limiter release must be finite and > 0 ms, got {release_ms}")]
    InvalidLimiterRelease {
        /// Rejected release time.
        release_ms: f32,
    },
    /// Per-channel configuration length does not match channel count.
    #[error("expected {expected} per-channel entries, got {actual}")]
    PerChannelConfig {
        /// Configured channel count.
        expected: usize,
        /// Supplied entry count.
        actual: usize,
    },
}

/// One parametric equalizer band using RBJ peaking-EQ coefficients.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PeqBand {
    /// Center frequency in Hz.
    pub frequency_hz: f32,
    /// Filter Q.
    pub q: f32,
    /// Boost/cut in dB.
    pub gain_db: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct BiquadCoefficients {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct BiquadState {
    z1: f32,
    z2: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Biquad {
    coefficients: BiquadCoefficients,
    state: BiquadState,
}

impl Biquad {
    fn new(coefficients: BiquadCoefficients) -> Self {
        Self {
            coefficients,
            state: BiquadState::default(),
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let c = self.coefficients;
        let output = c.b0.mul_add(input, self.state.z1);
        self.state.z1 = c.b1.mul_add(input, self.state.z2 - c.a1 * output);
        self.state.z2 = c.b2.mul_add(input, -c.a2 * output);
        if output.is_finite() {
            output
        } else {
            self.state = BiquadState::default();
            0.0
        }
    }

    fn reset(&mut self) {
        self.state = BiquadState::default();
    }
}

/// Per-channel parametric equalizer bank.
#[derive(Debug, Clone, PartialEq)]
pub struct ParametricEq {
    channel_filters: Vec<Vec<Biquad>>,
}

impl ParametricEq {
    /// Builds one independent PEQ chain per channel.
    pub fn new(
        sample_rate: u32,
        channel_bands: &[Vec<PeqBand>],
    ) -> Result<Self, OutputDspError> {
        if sample_rate == 0 {
            return Err(OutputDspError::InvalidSampleRate);
        }
        if channel_bands.is_empty() {
            return Err(OutputDspError::InvalidChannelCount);
        }
        let mut channel_filters = Vec::with_capacity(channel_bands.len());
        for bands in channel_bands {
            let mut filters = Vec::with_capacity(bands.len());
            for band in bands {
                filters.push(Biquad::new(peaking_coefficients(sample_rate, *band)?));
            }
            channel_filters.push(filters);
        }
        Ok(Self { channel_filters })
    }

    /// Processes samples in place without allocating.
    pub fn process_in_place(
        &mut self,
        channels: &mut [Vec<f32>],
        frame_count: usize,
    ) -> Result<(), OutputDspError> {
        validate_block(channels, self.channel_filters.len(), frame_count)?;
        for (samples, filters) in channels.iter_mut().zip(self.channel_filters.iter_mut()) {
            for sample in samples.iter_mut().take(frame_count) {
                let mut value = *sample;
                for filter in filters.iter_mut() {
                    value = filter.process(value);
                }
                *sample = value;
            }
        }
        Ok(())
    }

    /// Clears all filter history.
    pub fn reset(&mut self) {
        for filter in self.channel_filters.iter_mut().flatten() {
            filter.reset();
        }
    }
}

/// Fourth-order Linkwitz-Riley bass-management stage.
///
/// Each non-LFE channel is high-passed by two cascaded second-order Butterworth
/// filters. Its complementary low-passed signal is accumulated into the LFE
/// channel with a configurable redirect trim. The existing LFE program is
/// preserved and summed with redirected bass.
#[derive(Debug, Clone, PartialEq)]
pub struct BassManager {
    channel_count: usize,
    lfe_index: usize,
    max_frames: usize,
    redirect_gain: f32,
    high_pass: Vec<[Biquad; 2]>,
    low_pass: Vec<[Biquad; 2]>,
    lfe_accumulator: Vec<f32>,
}

impl BassManager {
    /// Creates a Linkwitz-Riley 4th-order bass-management stage.
    pub fn new(
        sample_rate: u32,
        channel_count: usize,
        lfe_index: usize,
        max_frames: usize,
        crossover_hz: f32,
        redirect_gain_db: f32,
    ) -> Result<Self, OutputDspError> {
        if sample_rate == 0 {
            return Err(OutputDspError::InvalidSampleRate);
        }
        if channel_count == 0 {
            return Err(OutputDspError::InvalidChannelCount);
        }
        if lfe_index >= channel_count {
            return Err(OutputDspError::InvalidLfeIndex {
                lfe_index,
                channel_count,
            });
        }
        if !redirect_gain_db.is_finite() {
            return Err(OutputDspError::InvalidGain {
                gain_db: redirect_gain_db,
            });
        }
        let low = low_pass_coefficients(sample_rate, crossover_hz, BUTTERWORTH_Q)?;
        let high = high_pass_coefficients(sample_rate, crossover_hz, BUTTERWORTH_Q)?;
        let low_pair = [Biquad::new(low), Biquad::new(low)];
        let high_pair = [Biquad::new(high), Biquad::new(high)];
        Ok(Self {
            channel_count,
            lfe_index,
            max_frames,
            redirect_gain: db_to_gain(redirect_gain_db),
            high_pass: vec![high_pair; channel_count],
            low_pass: vec![low_pair; channel_count],
            lfe_accumulator: vec![0.0; max_frames],
        })
    }

    /// Applies crossover and low-frequency redirection in place without allocation.
    pub fn process_in_place(
        &mut self,
        channels: &mut [Vec<f32>],
        frame_count: usize,
    ) -> Result<(), OutputDspError> {
        validate_block(channels, self.channel_count, frame_count)?;
        if frame_count > self.max_frames {
            return Err(OutputDspError::FrameCapacity {
                maximum: self.max_frames,
                actual: frame_count,
            });
        }
        self.lfe_accumulator[..frame_count].fill(0.0);

        for (channel_index, samples) in channels.iter_mut().enumerate() {
            if channel_index == self.lfe_index {
                continue;
            }
            let hp = &mut self.high_pass[channel_index];
            let lp = &mut self.low_pass[channel_index];
            for (frame, sample) in samples.iter_mut().take(frame_count).enumerate() {
                let input = *sample;
                let high = hp[1].process(hp[0].process(input));
                let low = lp[1].process(lp[0].process(input));
                *sample = high;
                self.lfe_accumulator[frame] += low * self.redirect_gain;
            }
        }

        let lfe = &mut channels[self.lfe_index];
        for (sample, redirected) in lfe
            .iter_mut()
            .zip(self.lfe_accumulator.iter().copied())
            .take(frame_count)
        {
            *sample += redirected;
        }
        Ok(())
    }

    /// Clears crossover filter history and the accumulator.
    pub fn reset(&mut self) {
        for pair in self.high_pass.iter_mut().chain(self.low_pass.iter_mut()) {
            pair[0].reset();
            pair[1].reset();
        }
        self.lfe_accumulator.fill(0.0);
    }
}

/// Linked multichannel zero-lookahead peak limiter.
///
/// Gain reduction is instantaneous when a frame would exceed the threshold;
/// release is exponential. One gain value is applied to all channels in a
/// frame so spatial balance is preserved.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkedPeakLimiter {
    channel_count: usize,
    threshold: f32,
    release_coefficient: f32,
    gain: f32,
}

impl LinkedPeakLimiter {
    /// Creates a linked limiter.
    pub fn new(
        sample_rate: u32,
        channel_count: usize,
        threshold: f32,
        release_ms: f32,
    ) -> Result<Self, OutputDspError> {
        if sample_rate == 0 {
            return Err(OutputDspError::InvalidSampleRate);
        }
        if channel_count == 0 {
            return Err(OutputDspError::InvalidChannelCount);
        }
        if !threshold.is_finite() || threshold <= 0.0 || threshold > 1.0 {
            return Err(OutputDspError::InvalidLimiterThreshold { threshold });
        }
        if !release_ms.is_finite() || release_ms <= 0.0 {
            return Err(OutputDspError::InvalidLimiterRelease { release_ms });
        }
        let release_seconds = release_ms / 1_000.0;
        let release_coefficient = (-1.0 / (release_seconds * sample_rate as f32)).exp();
        Ok(Self {
            channel_count,
            threshold,
            release_coefficient,
            gain: 1.0,
        })
    }

    /// Limits one multichannel block in place without allocation.
    pub fn process_in_place(
        &mut self,
        channels: &mut [Vec<f32>],
        frame_count: usize,
    ) -> Result<(), OutputDspError> {
        validate_block(channels, self.channel_count, frame_count)?;
        for frame in 0..frame_count {
            let mut peak = 0.0_f32;
            for samples in channels.iter() {
                peak = peak.max(samples[frame].abs());
            }
            let target = if peak > self.threshold && peak.is_finite() {
                self.threshold / peak
            } else if peak.is_finite() {
                1.0
            } else {
                0.0
            };
            if target < self.gain {
                self.gain = target;
            } else {
                self.gain = self.release_coefficient.mul_add(
                    self.gain,
                    (1.0 - self.release_coefficient) * target,
                );
            }
            for samples in channels.iter_mut() {
                let output = samples[frame] * self.gain;
                samples[frame] = if output.is_finite() { output } else { 0.0 };
            }
        }
        Ok(())
    }

    /// Resets limiter gain to unity.
    pub fn reset(&mut self) {
        self.gain = 1.0;
    }

    /// Returns the current linked gain for diagnostics.
    pub fn current_gain(&self) -> f32 {
        self.gain
    }
}

/// Canonical post-render output DSP chain excluding propagation delay.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputDspChain {
    channel_count: usize,
    trims: Vec<f32>,
    bass_manager: BassManager,
    eq: ParametricEq,
    limiter: LinkedPeakLimiter,
}

impl OutputDspChain {
    /// Creates the canonical output stage.
    pub fn new(
        sample_rate: u32,
        channel_count: usize,
        lfe_index: usize,
        max_frames: usize,
        crossover_hz: f32,
        redirect_gain_db: f32,
        channel_trim_db: &[f32],
        channel_bands: &[Vec<PeqBand>],
        limiter_threshold: f32,
        limiter_release_ms: f32,
    ) -> Result<Self, OutputDspError> {
        if channel_trim_db.len() != channel_count {
            return Err(OutputDspError::PerChannelConfig {
                expected: channel_count,
                actual: channel_trim_db.len(),
            });
        }
        if channel_bands.len() != channel_count {
            return Err(OutputDspError::PerChannelConfig {
                expected: channel_count,
                actual: channel_bands.len(),
            });
        }
        let mut trims = Vec::with_capacity(channel_count);
        for gain_db in channel_trim_db {
            if !gain_db.is_finite() {
                return Err(OutputDspError::InvalidGain { gain_db: *gain_db });
            }
            trims.push(db_to_gain(*gain_db));
        }
        Ok(Self {
            channel_count,
            trims,
            bass_manager: BassManager::new(
                sample_rate,
                channel_count,
                lfe_index,
                max_frames,
                crossover_hz,
                redirect_gain_db,
            )?,
            eq: ParametricEq::new(sample_rate, channel_bands)?,
            limiter: LinkedPeakLimiter::new(
                sample_rate,
                channel_count,
                limiter_threshold,
                limiter_release_ms,
            )?,
        })
    }

    /// Applies trims, bass management, PEQ and linked limiting in place.
    pub fn process_in_place(
        &mut self,
        channels: &mut [Vec<f32>],
        frame_count: usize,
    ) -> Result<(), OutputDspError> {
        validate_block(channels, self.channel_count, frame_count)?;
        for (samples, trim) in channels.iter_mut().zip(self.trims.iter().copied()) {
            for sample in samples.iter_mut().take(frame_count) {
                *sample *= trim;
            }
        }
        self.bass_manager.process_in_place(channels, frame_count)?;
        self.eq.process_in_place(channels, frame_count)?;
        self.limiter.process_in_place(channels, frame_count)?;
        Ok(())
    }

    /// Clears all stateful processing history.
    pub fn reset(&mut self) {
        self.bass_manager.reset();
        self.eq.reset();
        self.limiter.reset();
    }

    /// Returns current linked limiter gain for diagnostics.
    pub fn limiter_gain(&self) -> f32 {
        self.limiter.current_gain()
    }
}

fn validate_block(
    channels: &[Vec<f32>],
    expected_channels: usize,
    frame_count: usize,
) -> Result<(), OutputDspError> {
    if channels.len() != expected_channels {
        return Err(OutputDspError::ChannelCount {
            expected: expected_channels,
            actual: channels.len(),
        });
    }
    for (channel, samples) in channels.iter().enumerate() {
        if samples.len() < frame_count {
            return Err(OutputDspError::BufferFrames {
                channel,
                required: frame_count,
                actual: samples.len(),
            });
        }
    }
    Ok(())
}

fn validate_filter(sample_rate: u32, frequency_hz: f32, q: f32) -> Result<(), OutputDspError> {
    if sample_rate == 0 {
        return Err(OutputDspError::InvalidSampleRate);
    }
    if !frequency_hz.is_finite()
        || frequency_hz <= 0.0
        || frequency_hz >= sample_rate as f32 * 0.5
    {
        return Err(OutputDspError::InvalidFrequency {
            frequency_hz,
            sample_rate,
        });
    }
    if !q.is_finite() || q <= 0.0 {
        return Err(OutputDspError::InvalidQ { q });
    }
    Ok(())
}

fn peaking_coefficients(
    sample_rate: u32,
    band: PeqBand,
) -> Result<BiquadCoefficients, OutputDspError> {
    validate_filter(sample_rate, band.frequency_hz, band.q)?;
    if !band.gain_db.is_finite() {
        return Err(OutputDspError::InvalidGain {
            gain_db: band.gain_db,
        });
    }
    let a = 10.0_f32.powf(band.gain_db / 40.0);
    let omega = 2.0 * PI * band.frequency_hz / sample_rate as f32;
    let alpha = omega.sin() / (2.0 * band.q);
    let cos = omega.cos();
    normalize_coefficients(
        1.0 + alpha * a,
        -2.0 * cos,
        1.0 - alpha * a,
        1.0 + alpha / a,
        -2.0 * cos,
        1.0 - alpha / a,
    )
}

fn low_pass_coefficients(
    sample_rate: u32,
    frequency_hz: f32,
    q: f32,
) -> Result<BiquadCoefficients, OutputDspError> {
    validate_filter(sample_rate, frequency_hz, q)?;
    let omega = 2.0 * PI * frequency_hz / sample_rate as f32;
    let cos = omega.cos();
    let alpha = omega.sin() / (2.0 * q);
    normalize_coefficients(
        (1.0 - cos) * 0.5,
        1.0 - cos,
        (1.0 - cos) * 0.5,
        1.0 + alpha,
        -2.0 * cos,
        1.0 - alpha,
    )
}

fn high_pass_coefficients(
    sample_rate: u32,
    frequency_hz: f32,
    q: f32,
) -> Result<BiquadCoefficients, OutputDspError> {
    validate_filter(sample_rate, frequency_hz, q)?;
    let omega = 2.0 * PI * frequency_hz / sample_rate as f32;
    let cos = omega.cos();
    let alpha = omega.sin() / (2.0 * q);
    normalize_coefficients(
        (1.0 + cos) * 0.5,
        -(1.0 + cos),
        (1.0 + cos) * 0.5,
        1.0 + alpha,
        -2.0 * cos,
        1.0 - alpha,
    )
}

fn normalize_coefficients(
    b0: f32,
    b1: f32,
    b2: f32,
    a0: f32,
    a1: f32,
    a2: f32,
) -> Result<BiquadCoefficients, OutputDspError> {
    if !a0.is_finite() || a0.abs() <= f32::EPSILON {
        return Err(OutputDspError::InvalidQ { q: 0.0 });
    }
    let inverse = 1.0 / a0;
    Ok(BiquadCoefficients {
        b0: b0 * inverse,
        b1: b1 * inverse,
        b2: b2 * inverse,
        a1: a1 * inverse,
        a2: a2 * inverse,
    })
}

fn db_to_gain(gain_db: f32) -> f32 {
    10.0_f32.powf(gain_db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bass_manager_removes_dc_from_main_and_redirects_it_to_lfe() {
        let mut manager = BassManager::new(48_000, 2, 1, 8_192, 120.0, 0.0).unwrap();
        let mut audio = vec![vec![1.0; 8_192], vec![0.0; 8_192]];
        manager.process_in_place(&mut audio, 8_192).unwrap();

        let main_tail = audio[0][8_000..].iter().map(|v| v.abs()).sum::<f32>() / 192.0;
        let lfe_tail = audio[1][8_000..].iter().map(|v| v.abs()).sum::<f32>() / 192.0;
        assert!(main_tail < 0.01, "main dc tail={main_tail}");
        assert!(lfe_tail > 0.9, "lfe dc tail={lfe_tail}");
    }

    #[test]
    fn linked_limiter_caps_every_channel_and_preserves_ratio() {
        let mut limiter = LinkedPeakLimiter::new(48_000, 2, 0.8, 100.0).unwrap();
        let mut audio = vec![vec![2.0, 0.5], vec![1.0, 0.25]];
        limiter.process_in_place(&mut audio, 2).unwrap();

        assert!(audio.iter().flatten().all(|sample| sample.abs() <= 0.800_001));
        assert!((audio[0][0] / audio[1][0] - 2.0).abs() < 1.0e-6);
    }

    #[test]
    fn peq_is_deterministic_and_finite() {
        let bands = vec![
            vec![PeqBand {
                frequency_hz: 1_000.0,
                q: 1.0,
                gain_db: 3.0,
            }],
            vec![],
        ];
        let mut first = ParametricEq::new(48_000, &bands).unwrap();
        let mut second = ParametricEq::new(48_000, &bands).unwrap();
        let mut first_audio = vec![vec![0.1; 256], vec![0.2; 256]];
        let mut second_audio = first_audio.clone();
        first.process_in_place(&mut first_audio, 256).unwrap();
        second.process_in_place(&mut second_audio, 256).unwrap();
        assert_eq!(first_audio, second_audio);
        assert!(first_audio.iter().flatten().all(|sample| sample.is_finite()));
    }

    #[test]
    fn processing_does_not_change_channel_capacities() {
        let bands = vec![vec![], vec![]];
        let mut chain = OutputDspChain::new(
            48_000,
            2,
            1,
            256,
            120.0,
            -6.0,
            &[0.0, 0.0],
            &bands,
            0.95,
            100.0,
        )
        .unwrap();
        let mut audio = vec![vec![0.1; 256], vec![0.0; 256]];
        let capacities = audio.iter().map(Vec::capacity).collect::<Vec<_>>();
        for _ in 0..128 {
            chain.process_in_place(&mut audio, 256).unwrap();
        }
        assert_eq!(
            capacities,
            audio.iter().map(Vec::capacity).collect::<Vec<_>>()
        );
    }

    #[test]
    fn invalid_filter_and_limiter_configuration_fails_closed() {
        assert!(matches!(
            BassManager::new(48_000, 2, 1, 256, 24_000.0, 0.0),
            Err(OutputDspError::InvalidFrequency { .. })
        ));
        assert!(matches!(
            LinkedPeakLimiter::new(48_000, 2, 1.1, 100.0),
            Err(OutputDspError::InvalidLimiterThreshold { .. })
        ));
    }
}
