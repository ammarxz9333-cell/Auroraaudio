//! Linkwitz-Riley 4th-order (24 dB/octave) crossover filter processor for Aurora.
//!
//! Separates sub-crossover frequencies (< 80 Hz default) from main spatial channels
//! and routes low-frequency content into the LFE (Subwoofer) channel with phase alignment.

use std::f32::consts::PI;

use crate::BasicDspError;

/// Biquad second-order IIR filter state.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    fn new() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    /// Calculates 2nd-order Butterworth Low-Pass coefficients.
    fn set_lowpass(&mut self, sample_rate: u32, cutoff_hz: f32) {
        let omega = 2.0 * PI * (cutoff_hz / sample_rate as f32);
        let cos_w = omega.cos();
        let sin_w = omega.sin();
        let alpha = sin_w / (2.0 * (0.5_f32).sqrt()); // Q = 0.7071 for Butterworth

        let a0 = 1.0 + alpha;
        self.b0 = ((1.0 - cos_w) / 2.0) / a0;
        self.b1 = (1.0 - cos_w) / a0;
        self.b2 = ((1.0 - cos_w) / 2.0) / a0;
        self.a1 = (-2.0 * cos_w) / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    /// Calculates 2nd-order Butterworth High-Pass coefficients.
    fn set_highpass(&mut self, sample_rate: u32, cutoff_hz: f32) {
        let omega = 2.0 * PI * (cutoff_hz / sample_rate as f32);
        let cos_w = omega.cos();
        let sin_w = omega.sin();
        let alpha = sin_w / (2.0 * (0.5_f32).sqrt()); // Q = 0.7071 for Butterworth

        let a0 = 1.0 + alpha;
        self.b0 = ((1.0 + cos_w) / 2.0) / a0;
        self.b1 = (-(1.0 + cos_w)) / a0;
        self.b2 = ((1.0 + cos_w) / 2.0) / a0;
        self.a1 = (-2.0 * cos_w) / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    #[inline]
    fn process(&mut self, sample: f32) -> f32 {
        let out = self.b0 * sample + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = sample;
        self.y2 = self.y1;
        self.y1 = out;
        out
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

/// Linkwitz-Riley 4th order (24 dB/oct) filter stage composed of two cascaded 2nd-order Butterworth filters.
#[derive(Debug, Clone, Copy, PartialEq)]
struct LinkwitzRiley4 {
    stage1: Biquad,
    stage2: Biquad,
}

impl LinkwitzRiley4 {
    fn new_lowpass(sample_rate: u32, cutoff_hz: f32) -> Self {
        let mut stage1 = Biquad::new();
        let mut stage2 = Biquad::new();
        stage1.set_lowpass(sample_rate, cutoff_hz);
        stage2.set_lowpass(sample_rate, cutoff_hz);
        Self { stage1, stage2 }
    }

    fn new_highpass(sample_rate: u32, cutoff_hz: f32) -> Self {
        let mut stage1 = Biquad::new();
        let mut stage2 = Biquad::new();
        stage1.set_highpass(sample_rate, cutoff_hz);
        stage2.set_highpass(sample_rate, cutoff_hz);
        Self { stage1, stage2 }
    }

    #[inline]
    fn process(&mut self, sample: f32) -> f32 {
        self.stage2.process(self.stage1.process(sample))
    }

    fn reset(&mut self) {
        self.stage1.reset();
        self.stage2.reset();
    }
}

/// Multi-channel Crossover Processor for Satellite High-Pass and Subwoofer LFE Routing.
#[derive(Debug, Clone)]
pub struct CrossoverProcessor {
    channel_count: usize,
    lfe_channel_index: Option<usize>,
    crossover_hz: f32,
    sample_rate: u32,
    highpass_filters: Vec<LinkwitzRiley4>,
    lowpass_filters: Vec<LinkwitzRiley4>,
}

impl CrossoverProcessor {
    /// Creates a crossover processor with specified channels, LFE index, crossover frequency, and sample rate.
    pub fn new(
        channel_count: usize,
        lfe_channel_index: Option<usize>,
        crossover_hz: f32,
        sample_rate: u32,
    ) -> Self {
        let mut highpass_filters = Vec::with_capacity(channel_count);
        let mut lowpass_filters = Vec::with_capacity(channel_count);

        for _ in 0..channel_count {
            highpass_filters.push(LinkwitzRiley4::new_highpass(sample_rate, crossover_hz));
            lowpass_filters.push(LinkwitzRiley4::new_lowpass(sample_rate, crossover_hz));
        }

        Self {
            channel_count,
            lfe_channel_index,
            crossover_hz,
            sample_rate,
            highpass_filters,
            lowpass_filters,
        }
    }

    /// Processes planar audio block in-place with zero allocations.
    pub fn process_in_place(
        &mut self,
        channels: &mut [Vec<f32>],
        frame_count: usize,
    ) -> Result<(), BasicDspError> {
        if channels.len() != self.channel_count {
            return Err(BasicDspError::ChannelCount {
                expected: self.channel_count,
                actual: channels.len(),
            });
        }

        for (channel_idx, channel_buf) in channels.iter().enumerate() {
            if channel_buf.len() < frame_count {
                return Err(BasicDspError::BufferFrames {
                    channel: channel_idx,
                    required: frame_count,
                    actual: channel_buf.len(),
                });
            }
        }

        // Process satellite channels
        for i in 0..frame_count {
            let mut lfe_sum = 0.0_f32;

            for channel_idx in 0..self.channel_count {
                if Some(channel_idx) == self.lfe_channel_index {
                    continue;
                }

                let input_sample = channels[channel_idx][i];
                let hp_sample = self.highpass_filters[channel_idx].process(input_sample);
                let lp_sample = self.lowpass_filters[channel_idx].process(input_sample);

                channels[channel_idx][i] = hp_sample;
                lfe_sum += lp_sample;
            }

            // Sum low-passed content into LFE channel if present
            if let Some(lfe_idx) = self.lfe_channel_index {
                channels[lfe_idx][i] += lfe_sum;
            }
        }

        Ok(())
    }

    /// Resets all filter states.
    pub fn reset(&mut self) {
        for hp in &mut self.highpass_filters {
            hp.reset();
        }
        for lp in &mut self.lowpass_filters {
            lp.reset();
        }
    }

    /// Returns configured crossover frequency in Hz.
    pub fn crossover_hz(&self) -> f32 {
        self.crossover_hz
    }

    /// Returns configured sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossover_processor_preserves_finite_values() {
        let mut xover = CrossoverProcessor::new(6, Some(3), 80.0, 48000);
        let mut channels = vec![vec![0.1_f32; 128]; 6];
        xover.process_in_place(&mut channels, 128).unwrap();
        assert!(channels.iter().flatten().all(|s| s.is_finite()));
    }

    #[test]
    fn crossover_routes_sub_bass_to_lfe() {
        let mut xover = CrossoverProcessor::new(6, Some(3), 80.0, 48000);
        // Generate a 40Hz sine wave on channel 0 (FL)
        let mut channels = vec![vec![0.0_f32; 256]; 6];
        for i in 0..256 {
            let t = i as f32 / 48000.0;
            channels[0][i] = (2.0 * PI * 40.0 * t).sin();
        }

        xover.process_in_place(&mut channels, 256).unwrap();

        // LFE (channel 3) should have accumulated non-zero energy from low-frequency input
        let lfe_energy: f32 = channels[3].iter().map(|s| s.abs()).sum();
        assert!(lfe_energy > 1.0);
    }
}
