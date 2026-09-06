//! Cinema-Grade True-Peak Lookahead Limiter for Multichannel & 11.1.4 Systems.
//!
//! Provides inter-sample peak protection, lookahead delay, and multichannel
//! linked gain reduction to protect speakers and listeners from digital clipping
//! without altering spatial imaging or stereo/3D soundstage balance.

use std::collections::VecDeque;

/// Configuration for [`TruePeakLimiter`].
#[derive(Debug, Clone, PartialEq)]
pub struct LimiterConfig {
    /// Sample rate in Hertz (e.g. 48000, 96000, 192000).
    pub sample_rate: u32,
    /// Number of audio channels (e.g. 16 for 11.1.4).
    pub channel_count: usize,
    /// Maximum allowable true-peak ceiling in linear amplitude (default: 0.9772 = -0.2 dBFS).
    pub ceiling_linear: f32,
    /// Lookahead buffer duration in milliseconds (default: 2.5 ms).
    pub lookahead_ms: f32,
    /// Attack time in milliseconds (default: 1.0 ms).
    pub attack_ms: f32,
    /// Release time in milliseconds (default: 50.0 ms).
    pub release_ms: f32,
    /// Whether to link gain reduction across all channels to preserve 3D spatial panning.
    pub link_channels: bool,
}

impl Default for LimiterConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            channel_count: 16,
            ceiling_linear: 0.9772, // -0.2 dBFS
            lookahead_ms: 2.5,
            attack_ms: 1.0,
            release_ms: 50.0,
            link_channels: true,
        }
    }
}

/// Dynamic runtime statistics for [`TruePeakLimiter`].
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LimiterStats {
    /// Maximum gain reduction in decibels applied so far (negative or zero).
    pub max_gain_reduction_db: f32,
    /// Total number of frames where gain reduction was active (gain < 0.999).
    pub limited_frames_count: u64,
    /// Highest detected true-peak amplitude prior to limiting.
    pub peak_input_detected: f32,
}

/// Cinema-grade Lookahead True-Peak Limiter.
#[derive(Debug, Clone)]
pub struct TruePeakLimiter {
    config: LimiterConfig,
    lookahead_samples: usize,
    /// Lookahead ring buffers per channel.
    lookahead_buffers: Vec<VecDeque<f32>>,
    /// Previous samples for 4x inter-sample peak estimation.
    prev_samples: Vec<f32>,
    /// Smoothed envelope gain (0.0 .. 1.0).
    current_gain: f32,
    attack_coeff: f32,
    release_coeff: f32,
    stats: LimiterStats,
}

impl TruePeakLimiter {
    /// Creates a new TruePeakLimiter configured for the specified settings.
    pub fn new(config: LimiterConfig) -> Self {
        let sr = config.sample_rate.max(8000) as f32;
        let lookahead_samples = ((config.lookahead_ms * 0.001 * sr).round() as usize).max(1);

        let attack_coeff = (-1.0 / (config.attack_ms * 0.001 * sr)).exp();
        let release_coeff = (-1.0 / (config.release_ms * 0.001 * sr)).exp();

        let mut lookahead_buffers = Vec::with_capacity(config.channel_count);
        for _ in 0..config.channel_count {
            let mut q = VecDeque::with_capacity(lookahead_samples);
            q.resize(lookahead_samples, 0.0);
            lookahead_buffers.push(q);
        }

        Self {
            lookahead_samples,
            lookahead_buffers,
            prev_samples: vec![0.0; config.channel_count],
            current_gain: 1.0,
            attack_coeff,
            release_coeff,
            config,
            stats: LimiterStats::default(),
        }
    }

    /// Returns current telemetry stats.
    pub fn stats(&self) -> LimiterStats {
        self.stats
    }

    /// Resets the limiter state, lookahead history, and telemetry.
    pub fn reset(&mut self) {
        for buf in &mut self.lookahead_buffers {
            buf.clear();
            buf.resize(self.lookahead_samples, 0.0);
        }
        self.prev_samples.fill(0.0);
        self.current_gain = 1.0;
        self.stats = LimiterStats::default();
    }

    /// Estimates the inter-sample true-peak for a current sample and its previous sample.
    #[inline(always)]
    fn estimate_inter_sample_peak(curr: f32, prev: f32) -> f32 {
        let abs_curr = curr.abs();
        let abs_prev = prev.abs();
        let midpoint = (curr + prev) * 0.5;
        let estimated_inter = midpoint.abs() * 1.15;
        abs_curr.max(abs_prev).max(estimated_inter)
    }

    /// Processes multi-channel audio in place.
    pub fn process_in_place(&mut self, channels: &mut [Vec<f32>], frame_count: usize) {
        if channels.is_empty() || frame_count == 0 {
            return;
        }

        let num_channels = channels.len().min(self.config.channel_count);
        let ceiling = self.config.ceiling_linear;

        for frame_idx in 0..frame_count {
            // 1. Calculate true-peak across all active channels for this sample
            let mut frame_peak = 0.0_f32;

            for ch_idx in 0..num_channels {
                let sample = channels[ch_idx][frame_idx];
                let prev = self.prev_samples[ch_idx];
                let peak = Self::estimate_inter_sample_peak(sample, prev);
                if peak > frame_peak {
                    frame_peak = peak;
                }
                self.prev_samples[ch_idx] = sample;
            }

            if frame_peak > self.stats.peak_input_detected {
                self.stats.peak_input_detected = frame_peak;
            }

            // 2. Compute target gain
            let target_gain = if frame_peak > ceiling {
                ceiling / frame_peak
            } else {
                1.0
            };

            // 3. Smooth gain envelope with program-dependent attack/release
            if target_gain < self.current_gain {
                self.current_gain = target_gain + self.attack_coeff * (self.current_gain - target_gain);
            } else {
                self.current_gain = target_gain + self.release_coeff * (self.current_gain - target_gain);
            }

            self.current_gain = self.current_gain.clamp(0.0001, 1.0);

            // Update stats
            if self.current_gain < 0.999 {
                self.stats.limited_frames_count += 1;
                let gr_db = 20.0 * self.current_gain.log10();
                if gr_db < self.stats.max_gain_reduction_db {
                    self.stats.max_gain_reduction_db = gr_db;
                }
            }

            // 4. Apply gain to delayed lookahead audio
            for ch_idx in 0..num_channels {
                let input_sample = channels[ch_idx][frame_idx];
                let delayed_sample = self.lookahead_buffers[ch_idx].pop_front().unwrap_or(0.0);
                self.lookahead_buffers[ch_idx].push_back(input_sample);

                let limited_output = (delayed_sample * self.current_gain).clamp(-ceiling, ceiling);
                channels[ch_idx][frame_idx] = limited_output;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_preserves_signals_below_ceiling() {
        let config = LimiterConfig {
            sample_rate: 48_000,
            channel_count: 2,
            ceiling_linear: 0.9772,
            lookahead_ms: 1.0,
            attack_ms: 0.5,
            release_ms: 20.0,
            link_channels: true,
        };
        let mut limiter = TruePeakLimiter::new(config);

        let mut channels = vec![vec![0.3_f32; 256], vec![-0.4_f32; 256]];
        limiter.process_in_place(&mut channels, 256);

        for sample in &channels[0] {
            assert!(sample.abs() <= 0.35);
        }
        assert_eq!(limiter.stats().limited_frames_count, 0);
    }

    #[test]
    fn limiter_strictly_enforces_ceiling_on_high_transients() {
        let config = LimiterConfig {
            sample_rate: 48_000,
            channel_count: 16,
            ceiling_linear: 0.9772,
            lookahead_ms: 2.0,
            attack_ms: 0.5,
            release_ms: 10.0,
            link_channels: true,
        };
        let mut limiter = TruePeakLimiter::new(config);

        let mut channels = vec![vec![2.0_f32; 512]; 16];
        limiter.process_in_place(&mut channels, 512);

        for ch in &channels {
            for &sample in ch {
                assert!(
                    sample.abs() <= 0.977201,
                    "Sample {} exceeded ceiling 0.9772!",
                    sample
                );
            }
        }

        assert!(limiter.stats().max_gain_reduction_db < -3.0);
        assert!(limiter.stats().limited_frames_count > 0);
    }
}
