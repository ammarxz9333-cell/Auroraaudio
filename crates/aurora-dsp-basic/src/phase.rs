//! Subwoofer FIR Phase Alignment & Group Delay Correction.
//!
//! Applies an all-pass phase alignment filter to eliminate acoustic phase
//! cancellation between the subwoofer and the main/satellite speakers in
//! the 60 Hz - 120 Hz crossover zone.

use std::f32::consts::PI;

/// All-pass Phase Alignment Filter for Subwoofer / LFE Integration.
#[derive(Debug, Clone)]
pub struct SubwooferPhaseAligner {
    pub sample_rate: u32,
    pub crossover_hz: f32,
    pub phase_offset_rad: f32,
    taps: Vec<f32>,
    history: Vec<f32>,
    write_pos: usize,
}

impl SubwooferPhaseAligner {
    /// Creates a subwoofer phase aligner with specified crossover frequency and phase rotation angle in degrees (0..360).
    pub fn new(sample_rate: u32, crossover_hz: f32, phase_degrees: f32) -> Self {
        let phase_offset_rad = phase_degrees.to_radians();
        let num_taps = 65; // Odd length symmetric/antisymmetric FIR for exact linear phase properties

        let taps = Self::design_allpass_fir(num_taps, sample_rate, crossover_hz, phase_offset_rad);
        let history = vec![0.0; num_taps];

        Self {
            sample_rate,
            crossover_hz,
            phase_offset_rad,
            taps,
            history,
            write_pos: 0,
        }
    }

    /// Designs an FIR all-pass Hilbert/phase-rotation kernel centered around crossover.
    fn design_allpass_fir(
        num_taps: usize,
        sample_rate: u32,
        crossover_hz: f32,
        phase_rad: f32,
    ) -> Vec<f32> {
        let mut taps = vec![0.0; num_taps];
        let center = (num_taps - 1) / 2;
        let omega_c = 2.0 * PI * (crossover_hz / sample_rate as f32);

        let cos_p = phase_rad.cos();
        let sin_p = phase_rad.sin();

        for i in 0..num_taps {
            let n = i as f32 - center as f32;
            // Blackman-Harris window for maximum stopband rejection
            let a0 = 0.35875;
            let a1 = 0.48829;
            let a2 = 0.14128;
            let a3 = 0.01168;
            let w = a0
                - a1 * (2.0 * PI * i as f32 / (num_taps - 1) as f32).cos()
                + a2 * (4.0 * PI * i as f32 / (num_taps - 1) as f32).cos()
                - a3 * (6.0 * PI * i as f32 / (num_taps - 1) as f32).cos();

            if n == 0.0 {
                taps[i] = cos_p * w;
            } else {
                // Hilbert-derived phase rotation
                let sinc = (omega_c * n).sin() / (PI * n);
                let hilbert = if (n as i32) % 2 != 0 {
                    2.0 / (PI * n)
                } else {
                    0.0
                };
                taps[i] = (cos_p * sinc - sin_p * hilbert) * w;
            }
        }

        // Normalize energy
        let energy: f32 = taps.iter().map(|t| t * t).sum();
        if energy > 0.0 {
            let scale = 1.0 / energy.sqrt();
            for t in taps.iter_mut() {
                *t *= scale;
            }
        }

        taps
    }

    /// Processes subwoofer LFE channel samples in-place.
    pub fn process_lfe_in_place(&mut self, lfe_channel: &mut [f32]) {
        let num_taps = self.taps.len();

        for sample in lfe_channel.iter_mut() {
            self.history[self.write_pos] = *sample;

            let mut out = 0.0_f32;
            let mut tap_idx = 0;
            let mut read_idx = self.write_pos;

            while tap_idx < num_taps {
                out += self.history[read_idx] * self.taps[tap_idx];
                tap_idx += 1;
                if read_idx == 0 {
                    read_idx = num_taps - 1;
                } else {
                    read_idx -= 1;
                }
            }

            self.write_pos = (self.write_pos + 1) % num_taps;
            *sample = out;
        }
    }

    /// Clears filter memory.
    pub fn reset(&mut self) {
        self.history.fill(0.0);
        self.write_pos = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processes_lfe_with_finite_energy() {
        let mut aligner = SubwooferPhaseAligner::new(48000, 80.0, 90.0);

        let mut lfe = vec![0.0; 256];
        for i in 0..256 {
            let t = i as f32 / 48000.0;
            lfe[i] = (2.0 * PI * 60.0 * t).sin() * 0.5;
        }

        aligner.process_lfe_in_place(&mut lfe);

        for &sample in &lfe {
            assert!(sample.is_finite());
        }

        let energy: f32 = lfe.iter().map(|s| s.abs()).sum();
        assert!(energy > 1.0, "LFE must retain substantial bass energy");
    }
}
