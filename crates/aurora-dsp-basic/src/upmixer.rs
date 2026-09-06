//! Aurora Smart Immersive Neural Upmixer (2.0 Stereo / 5.1 -> 11.1.4 Spatial Array).
//!
//! Transforms standard non-Atmos content (Stereo, YouTube, 5.1 broadcast) into a full
//! 16-channel 11.1.4 cinema soundstage using direct/ambient signal decomposition,
//! diffuse ceiling height extraction, and wide speaker soundstage expansion.

use crate::BasicDspError;

/// Multi-channel 11.1.4 Smart Immersive Upmixer.
#[derive(Debug, Clone)]
pub struct SmartImmersiveUpmixer {
    pub sample_rate: u32,
    height_decorrelation_samples: usize,
    surround_decorrelation_samples: usize,
    history_l: Vec<f32>,
    history_r: Vec<f32>,
    write_pos: usize,
    height_gain: f32,
    wide_gain: f32,
    surround_gain: f32,
    center_spread: f32,
}

impl SmartImmersiveUpmixer {
    /// Creates a new upmixer configured for the specified sample rate.
    pub fn new(sample_rate: u32) -> Self {
        let max_delay_ms = 25.0;
        let buf_size = ((sample_rate as f32 * max_delay_ms) / 1000.0).ceil() as usize + 64;

        Self {
            sample_rate,
            height_decorrelation_samples: ((sample_rate as f32 * 7.5) / 1000.0) as usize,
            surround_decorrelation_samples: ((sample_rate as f32 * 15.0) / 1000.0) as usize,
            history_l: vec![0.0; buf_size],
            history_r: vec![0.0; buf_size],
            write_pos: 0,
            height_gain: 0.707,
            wide_gain: 0.85,
            surround_gain: 0.8,
            center_spread: 0.707,
        }
    }

    /// Sets upmixing gains for height ceiling speakers and wide channels.
    pub fn set_gains(&mut self, height_gain: f32, wide_gain: f32, surround_gain: f32) {
        self.height_gain = height_gain.clamp(0.0, 2.0);
        self.wide_gain = wide_gain.clamp(0.0, 2.0);
        self.surround_gain = surround_gain.clamp(0.0, 2.0);
    }

    /// Upmixes a 2-channel stereo signal (Left, Right) into full 16-channel 11.1.4 array.
    ///
    /// Canonical 11.1.4 order (16 channels):
    /// 0: FrontLeft, 1: FrontRight, 2: FrontCenter, 3: LFE,
    /// 4: SurroundLeft, 5: SurroundRight, 6: SurroundBackLeft, 7: SurroundBackRight,
    /// 8: WideLeft, 9: WideRight, 10: TopFrontLeft, 11: TopFrontRight,
    /// 12: TopRearLeft, 13: TopRearRight, 14: TopSideLeft, 15: TopSideRight
    pub fn upmix_stereo_to_11_1_4(
        &mut self,
        left: &[f32],
        right: &[f32],
        output_16: &mut [Vec<f32>],
        frame_count: usize,
    ) -> Result<(), BasicDspError> {
        if output_16.len() != 16 {
            return Err(BasicDspError::ChannelCount {
                expected: 16,
                actual: output_16.len(),
            });
        }
        for (ch_idx, ch) in output_16.iter().enumerate() {
            if ch.len() < frame_count {
                return Err(BasicDspError::BufferFrames {
                    channel: ch_idx,
                    required: frame_count,
                    actual: ch.len(),
                });
            }
        }

        let buf_len = self.history_l.len();

        for i in 0..frame_count {
            let l = left[i];
            let r = right[i];

            // 1. Direct/Ambient Decomposition
            // Center direct component (in-phase sum)
            let center_direct = (l + r) * 0.5;
            // Ambient diffuse component (out-of-phase difference)
            let ambient_diff = (l - r) * 0.5;

            // Delayed ambient components for spatial decorrelation
            let d_surr = (self.write_pos + buf_len - self.surround_decorrelation_samples) % buf_len;
            let d_height = (self.write_pos + buf_len - self.height_decorrelation_samples) % buf_len;

            let l_delayed_surr = self.history_l[d_surr];
            let r_delayed_surr = self.history_r[d_surr];
            let amb_delayed_height = (self.history_l[d_height] - self.history_r[d_height]) * 0.5;

            // Update delay ring buffers
            self.history_l[self.write_pos] = l;
            self.history_r[self.write_pos] = r;
            self.write_pos = (self.write_pos + 1) % buf_len;

            // 2. Channel Synthesis
            // Ch 0: FrontLeft
            output_16[0][i] = l - (center_direct * (1.0 - self.center_spread));
            // Ch 1: FrontRight
            output_16[1][i] = r - (center_direct * (1.0 - self.center_spread));
            // Ch 2: FrontCenter
            output_16[2][i] = center_direct * 1.2;
            // Ch 3: LFE (Subwoofer bass foundation)
            output_16[3][i] = (l + r) * 0.35;

            // Ch 4: SurroundLeft
            output_16[4][i] = (ambient_diff + (l_delayed_surr * 0.4)) * self.surround_gain;
            // Ch 5: SurroundRight
            output_16[5][i] = (-ambient_diff + (r_delayed_surr * 0.4)) * self.surround_gain;

            // Ch 6: SurroundBackLeft
            output_16[6][i] = (ambient_diff * 0.7 + l_delayed_surr * 0.5) * self.surround_gain;
            // Ch 7: SurroundBackRight
            output_16[7][i] = (-ambient_diff * 0.7 + r_delayed_surr * 0.5) * self.surround_gain;

            // Ch 8: WideLeft (expands front soundstage between Front and Surround)
            output_16[8][i] = (output_16[0][i] * 0.6 + output_16[4][i] * 0.5) * self.wide_gain;
            // Ch 9: WideRight
            output_16[9][i] = (output_16[1][i] * 0.6 + output_16[5][i] * 0.5) * self.wide_gain;

            // 3. Ceiling Heights (11.1.4 Top Speakers)
            // Ch 10: TopFrontLeft
            output_16[10][i] = (ambient_diff * 0.8 + amb_delayed_height * 0.4) * self.height_gain;
            // Ch 11: TopFrontRight
            output_16[11][i] = (-ambient_diff * 0.8 - amb_delayed_height * 0.4) * self.height_gain;
            // Ch 12: TopRearLeft
            output_16[12][i] = (amb_delayed_height * 0.85) * self.height_gain;
            // Ch 13: TopRearRight
            output_16[13][i] = (-amb_delayed_height * 0.85) * self.height_gain;

            // Ch 14: TopSideLeft (Samsung Q995 side-firing height reflection)
            output_16[14][i] = (output_16[10][i] + output_16[12][i]) * 0.5;
            // Ch 15: TopSideRight
            output_16[15][i] = (output_16[11][i] + output_16[13][i]) * 0.5;
        }

        Ok(())
    }

    /// Generic upmix accepting 2.0 or 5.1/7.1 input and expanding to 16-channel 11.1.4.
    pub fn upmix_to_11_1_4(
        &mut self,
        input: &[Vec<f32>],
        output_16: &mut [Vec<f32>],
        frame_count: usize,
    ) -> Result<(), BasicDspError> {
        if input.is_empty() {
            return Ok(());
        }
        if input.len() >= 2 {
            self.upmix_stereo_to_11_1_4(&input[0], &input[1], output_16, frame_count)?;

            // If 5.1 or 7.1 content is present, blend native discrete channels
            if input.len() >= 6 {
                // Blend discrete Center (ch 2) and LFE (ch 3)
                for i in 0..frame_count {
                    output_16[2][i] = (output_16[2][i] * 0.3) + (input[2][i] * 0.8);
                    output_16[3][i] = (output_16[3][i] * 0.3) + (input[3][i] * 0.8);
                    output_16[4][i] = (output_16[4][i] * 0.4) + (input[4][i] * 0.7);
                    output_16[5][i] = (output_16[5][i] * 0.4) + (input[5][i] * 0.7);
                }
            }
            if input.len() >= 8 {
                for i in 0..frame_count {
                    output_16[6][i] = (output_16[6][i] * 0.4) + (input[6][i] * 0.7);
                    output_16[7][i] = (output_16[7][i] * 0.4) + (input[7][i] * 0.7);
                }
            }
        }
        Ok(())
    }

    /// Clears delay line history.
    pub fn reset(&mut self) {
        self.history_l.fill(0.0);
        self.history_r.fill(0.0);
        self.write_pos = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    #[test]
    fn stereo_upmixes_to_all_sixteen_channels() {
        let mut upmixer = SmartImmersiveUpmixer::new(48000);
        let frames = 256;

        // Create stereo test input (Left channel tone, Right channel tone)
        let mut left = vec![0.0; frames];
        let mut right = vec![0.0; frames];
        for i in 0..frames {
            let t = i as f32 / 48000.0;
            left[i] = (2.0 * PI * 440.0 * t).sin() * 0.5;
            right[i] = (2.0 * PI * 440.0 * t + 0.5).sin() * 0.5;
        }

        let mut output_16 = vec![vec![0.0; frames]; 16];
        upmixer
            .upmix_stereo_to_11_1_4(&left, &right, &mut output_16, frames)
            .expect("stereo upmix succeeded");

        // Verify all 16 channels have active non-zero energy
        for (ch_idx, ch) in output_16.iter().enumerate() {
            let energy: f32 = ch.iter().map(|s| s.abs()).sum();
            assert!(
                energy > 0.1,
                "Channel {ch_idx} must have active upmixed energy, got {energy}"
            );
        }

        // Verify Height channels (10, 11, 12, 13, 14, 15) contain diffuse ambient height sound
        let top_front_left_energy: f32 = output_16[10].iter().map(|s| s.abs()).sum();
        let top_front_right_energy: f32 = output_16[11].iter().map(|s| s.abs()).sum();
        assert!(top_front_left_energy > 0.5);
        assert!(top_front_right_energy > 0.5);
    }
}
