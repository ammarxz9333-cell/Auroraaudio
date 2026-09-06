//! Active Center Channel Dialogue Enhancement & DRC Limiter for Aurora.
//!
//! Provides speech-frequency bandpass gain boost (300 Hz to 3400 Hz) for Center channel clarity
//! and peak dynamic range compression (DRC Limiter) for night-mode playback.

use std::f32::consts::PI;

use crate::BasicDspError;

/// Active Center Channel Dialogue Enhancer.
#[derive(Debug, Clone)]
pub struct DialogueEnhancer {
    center_channel_index: Option<usize>,
    gain_boost_db: f32,
    enable_night_mode_drc: bool,
    drc_threshold_db: f32,
    // Bandpass biquad filter state (300Hz - 3400Hz)
    bp_b0: f32,
    bp_b1: f32,
    bp_b2: f32,
    bp_a1: f32,
    bp_a2: f32,
    bp_x1: f32,
    bp_x2: f32,
    bp_y1: f32,
    bp_y2: f32,
    envelope: f32,
}

impl DialogueEnhancer {
    /// Creates a dialogue enhancer for specified center channel index and boost in dB.
    pub fn new(
        center_channel_index: Option<usize>,
        gain_boost_db: f32,
        enable_night_mode_drc: bool,
        sample_rate: u32,
    ) -> Self {
        // Calculate bandpass coefficients centered around 1000 Hz with Q ~ 1.0 (covering 300Hz - 3.4kHz)
        let f0 = 1000.0_f32;
        let q = 1.0_f32;
        let omega = 2.0 * PI * (f0 / sample_rate as f32);
        let alpha = omega.sin() / (2.0 * q);
        let a0 = 1.0 + alpha;

        let bp_b0 = alpha / a0;
        let bp_b1 = 0.0;
        let bp_b2 = -alpha / a0;
        let bp_a1 = (-2.0 * omega.cos()) / a0;
        let bp_a2 = (1.0 - alpha) / a0;

        Self {
            center_channel_index,
            gain_boost_db,
            enable_night_mode_drc,
            drc_threshold_db: -12.0, // Default Night mode threshold
            bp_b0,
            bp_b1,
            bp_b2,
            bp_a1,
            bp_a2,
            bp_x1: 0.0,
            bp_x2: 0.0,
            bp_y1: 0.0,
            bp_y2: 0.0,
            envelope: 0.0,
        }
    }

    /// Processes audio channels in-place to apply dialogue boost and peak compression.
    pub fn process_in_place(
        &mut self,
        channels: &mut [Vec<f32>],
        frame_count: usize,
    ) -> Result<(), BasicDspError> {
        let boost_factor = 10.0_f32.powf(self.gain_boost_db / 20.0) - 1.0;
        let threshold_linear = 10.0_f32.powf(self.drc_threshold_db / 20.0);

        if let Some(center_idx) = self.center_channel_index {
            if center_idx >= channels.len() {
                return Err(BasicDspError::ChannelCount {
                    expected: center_idx + 1,
                    actual: channels.len(),
                });
            }

            let center_buf = &mut channels[center_idx];
            if center_buf.len() < frame_count {
                return Err(BasicDspError::BufferFrames {
                    channel: center_idx,
                    required: frame_count,
                    actual: center_buf.len(),
                });
            }

            for sample in center_buf.iter_mut().take(frame_count) {
                let input = *sample;
                // Bandpass filtering
                let speech_band = self.bp_b0 * input + self.bp_b1 * self.bp_x1 + self.bp_b2 * self.bp_x2
                    - self.bp_a1 * self.bp_y1
                    - self.bp_a2 * self.bp_y2;

                self.bp_x2 = self.bp_x1;
                self.bp_x1 = input;
                self.bp_y2 = self.bp_y1;
                self.bp_y1 = speech_band;

                // Add speech band gain boost
                *sample = input + speech_band * boost_factor;
            }
        }

        // Apply Night Mode DRC limiter across all channels if enabled
        if self.enable_night_mode_drc {
            let attack = 0.01_f32;
            let release = 0.001_f32;

            for i in 0..frame_count {
                let mut max_peak = 0.0_f32;
                for ch in channels.iter() {
                    if i < ch.len() {
                        max_peak = max_peak.max(ch[i].abs());
                    }
                }

                // Envelope follower
                if max_peak > self.envelope {
                    self.envelope += attack * (max_peak - self.envelope);
                } else {
                    self.envelope += release * (max_peak - self.envelope);
                }

                // Compression gain
                let gain = if self.envelope > threshold_linear {
                    (threshold_linear / self.envelope.max(1e-6)).sqrt()
                } else {
                    1.0
                };

                for ch in channels.iter_mut() {
                    if i < ch.len() {
                        ch[i] *= gain;
                    }
                }
            }
        }

        Ok(())
    }

    /// Resets filter and envelope states.
    pub fn reset(&mut self) {
        self.bp_x1 = 0.0;
        self.bp_x2 = 0.0;
        self.bp_y1 = 0.0;
        self.bp_y2 = 0.0;
        self.envelope = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialogue_enhancer_processes_without_error() {
        let mut enhancer = DialogueEnhancer::new(Some(2), 3.0, true, 48000);
        let mut channels = vec![vec![0.1_f32; 128]; 6];
        enhancer.process_in_place(&mut channels, 128).unwrap();
        assert!(channels.iter().flatten().all(|s| s.is_finite()));
    }
}
