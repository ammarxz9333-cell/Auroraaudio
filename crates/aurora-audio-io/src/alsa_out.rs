//! Deterministic output-side DSP primitives for the Aurora R1 direct-ALSA path.
//!
//! The live R1 contract is fixed at 48 kHz with twelve rendered speaker
//! channels (`FL FR C LFE BL BR SL SR TFL TFR TRL TRR`) packed into a
//! sixteen-slot TDM stream. Slots 13..16 are deliberately zeroed.
//!
//! This module contains no ALSA calls. It can therefore be tested on every CI
//! platform while the Linux-specific PCM wrapper remains isolated in
//! `alsa_pcm.rs`.

use std::f64::consts::PI;

/// Number of rendered channels produced by Omniphony for Aurora R1.
pub const RENDER_CHANNELS: usize = 12;
/// Number of physical slots on the SAI3 TDM512 link.
pub const TDM_CHANNELS: usize = 16;
/// Fixed Aurora R1 output sample rate.
pub const OUTPUT_SAMPLE_RATE: u32 = 48_000;

const RESAMPLER_TAPS: usize = 32;
const RESAMPLER_PHASES: usize = 2_048;
const RESAMPLER_CENTER: usize = RESAMPLER_TAPS / 2 - 1;
const RESAMPLER_RADIUS: f64 = RESAMPLER_TAPS as f64 / 2.0;
/// Minimum source frames required to prime the 32-tap interpolation window.
pub const RESAMPLER_PRIME_FRAMES: usize = RESAMPLER_TAPS / 2 + 1;
/// Future frames retained by the resampler and therefore part of output latency.
pub const RESAMPLER_LOOKAHEAD_FRAMES: usize = RESAMPLER_TAPS - RESAMPLER_CENTER - 1;

/// One interleaved logical 7.1.4 sample frame.
pub type Frame12 = [f32; RENDER_CHANNELS];

/// Smoothed PI controller used to keep the rendered-audio queue centred while
/// the eARC media clock and DAC clock differ by a small number of ppm.
///
/// The controller deliberately filters block-level queue jitter, applies
/// anti-windup at the ppm clamp and slew-limits correction changes. The latter
/// prevents a producer block boundary or xrun recovery from turning into an
/// abrupt resampling-ratio step.
#[derive(Debug, Clone)]
pub struct AdaptiveClockController {
    target_frames: usize,
    max_ppm: f64,
    kp_ppm: f64,
    ki_ppm_per_second: f64,
    filter_tau_seconds: f64,
    max_slew_ppm_per_second: f64,
    filtered_error: f64,
    integral_error_seconds: f64,
    last_ppm: f64,
}

impl AdaptiveClockController {
    /// Creates a controller targeting `target_frames` queued source frames.
    pub fn new(target_frames: usize, max_ppm: f64) -> Self {
        Self {
            target_frames: target_frames.max(1),
            max_ppm: max_ppm.abs().max(1.0),
            kp_ppm: 900.0,
            ki_ppm_per_second: 100.0,
            filter_tau_seconds: 0.25,
            max_slew_ppm_per_second: 400.0,
            filtered_error: 0.0,
            integral_error_seconds: 0.0,
            last_ppm: 0.0,
        }
    }

    /// Updates the controller and returns source-consumption correction in ppm.
    /// Positive ppm means consume source frames faster.
    pub fn update(&mut self, queued_frames: usize, interval_seconds: f64) -> f64 {
        let dt = interval_seconds.clamp(0.0, 0.1);
        if dt == 0.0 {
            return self.last_ppm;
        }

        let target = self.target_frames as f64;
        let raw_error = (queued_frames as f64 - target) / target;
        let alpha = dt / (self.filter_tau_seconds + dt);
        self.filtered_error += alpha * (raw_error - self.filtered_error);

        let candidate_integral =
            (self.integral_error_seconds + self.filtered_error * dt).clamp(-2.0, 2.0);
        let candidate_unclamped =
            self.kp_ppm * self.filtered_error + self.ki_ppm_per_second * candidate_integral;

        let can_integrate = candidate_unclamped.abs() <= self.max_ppm
            || (candidate_unclamped > self.max_ppm && self.filtered_error < 0.0)
            || (candidate_unclamped < -self.max_ppm && self.filtered_error > 0.0);
        if can_integrate {
            self.integral_error_seconds = candidate_integral;
        }

        let target_ppm = (self.kp_ppm * self.filtered_error
            + self.ki_ppm_per_second * self.integral_error_seconds)
            .clamp(-self.max_ppm, self.max_ppm);
        let max_delta = self.max_slew_ppm_per_second * dt;
        self.last_ppm += (target_ppm - self.last_ppm).clamp(-max_delta, max_delta);
        self.last_ppm = self.last_ppm.clamp(-self.max_ppm, self.max_ppm);
        self.last_ppm
    }

    /// Discards timing history after an ALSA recovery discontinuity.
    pub fn reset_after_discontinuity(&mut self) {
        self.filtered_error = 0.0;
        self.integral_error_seconds = 0.0;
        self.last_ppm = 0.0;
    }

    pub fn step_from_ppm(ppm: f64) -> f64 {
        1.0 + ppm * 1.0e-6
    }

    pub fn target_frames(&self) -> usize {
        self.target_frames
    }
}

/// 32-tap, 2048-phase windowed-sinc fractional resampler for twelve-channel PCM.
///
/// Aurora only needs tiny continuous clock corrections. The precomputed
/// Lanczos-windowed sinc table keeps the steady-state hot path allocation-free
/// while avoiding the severe high-frequency interpolation error of cubic DSP.
#[derive(Debug)]
pub struct BandlimitedResampler12 {
    frames: [Frame12; RESAMPLER_TAPS],
    kernels: Box<[[f32; RESAMPLER_TAPS]]>,
    head: usize,
    phase: f64,
    primed: bool,
}

impl Default for BandlimitedResampler12 {
    fn default() -> Self {
        Self::new()
    }
}

impl BandlimitedResampler12 {
    pub fn new() -> Self {
        Self {
            frames: [[0.0; RENDER_CHANNELS]; RESAMPLER_TAPS],
            kernels: build_resampler_kernels(),
            head: 0,
            phase: 0.0,
            primed: false,
        }
    }

    pub fn prime<F, E>(&mut self, next: &mut F) -> Result<(), E>
    where
        F: FnMut() -> Result<Frame12, E>,
    {
        let first = next()?;
        for slot in &mut self.frames[..=RESAMPLER_CENTER] {
            *slot = first;
        }
        for slot in &mut self.frames[RESAMPLER_CENTER + 1..] {
            *slot = next()?;
        }
        self.head = 0;
        self.phase = 0.0;
        self.primed = true;
        Ok(())
    }

    pub fn render<F, E>(&mut self, source_step: f64, next: &mut F) -> Result<Frame12, E>
    where
        F: FnMut() -> Result<Frame12, E>,
    {
        debug_assert!(self.primed, "BandlimitedResampler12 must be primed first");
        let kernel_index =
            ((self.phase * RESAMPLER_PHASES as f64).round() as usize).min(RESAMPLER_PHASES);
        let kernel = &self.kernels[kernel_index];
        let mut out = [0.0_f32; RENDER_CHANNELS];

        for (logical_tap, coefficient) in kernel.iter().copied().enumerate() {
            let frame = &self.frames[(self.head + logical_tap) % RESAMPLER_TAPS];
            for (output_sample, input_sample) in out.iter_mut().zip(frame.iter()) {
                *output_sample += *input_sample * coefficient;
            }
        }

        self.phase += source_step.clamp(0.999, 1.001);
        while self.phase >= 1.0 {
            let recycled_slot = self.head;
            self.head = (self.head + 1) % RESAMPLER_TAPS;
            self.frames[recycled_slot] = next()?;
            self.phase -= 1.0;
        }
        Ok(out)
    }
}

fn build_resampler_kernels() -> Box<[[f32; RESAMPLER_TAPS]]> {
    let mut kernels = vec![[0.0_f32; RESAMPLER_TAPS]; RESAMPLER_PHASES + 1];
    for (phase_index, kernel) in kernels.iter_mut().enumerate() {
        let fraction = phase_index as f64 / RESAMPLER_PHASES as f64;
        let mut sum = 0.0_f64;
        for (tap, coefficient) in kernel.iter_mut().enumerate() {
            let offset = tap as f64 - RESAMPLER_CENTER as f64 - fraction;
            let value = if offset.abs() < RESAMPLER_RADIUS {
                sinc_pi(offset) * sinc_pi(offset / RESAMPLER_RADIUS)
            } else {
                0.0
            };
            *coefficient = value as f32;
            sum += value;
        }

        let normalization = 1.0 / sum;
        for coefficient in kernel.iter_mut() {
            *coefficient = (f64::from(*coefficient) * normalization) as f32;
        }
    }
    kernels.into_boxed_slice()
}

fn sinc_pi(x: f64) -> f64 {
    if x.abs() < 1.0e-12 {
        1.0
    } else {
        (PI * x).sin() / (PI * x)
    }
}

pub fn pack_frame_s32(frame: &Frame12, linear_gain: f32) -> [i32; TDM_CHANNELS] {
    let mut out = [0_i32; TDM_CHANNELS];
    for (dst, sample) in out[..RENDER_CHANNELS].iter_mut().zip(frame.iter()) {
        *dst = f32_to_s32(*sample * linear_gain);
    }
    out
}

pub fn f32_to_s32(sample: f32) -> i32 {
    if !sample.is_finite() {
        return 0;
    }
    if sample >= 1.0 {
        return i32::MAX;
    }
    if sample <= -1.0 {
        return i32::MIN;
    }
    (f64::from(sample) * 2_147_483_648.0).round() as i32
}

pub fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_contract_is_self_consistent() {
        assert_eq!(RESAMPLER_PRIME_FRAMES, 17);
        assert_eq!(RESAMPLER_LOOKAHEAD_FRAMES, 16);
    }

    #[test]
    fn pack_contract_preserves_first_twelve_and_zeros_reserve_slots() {
        let mut frame = [0.0_f32; RENDER_CHANNELS];
        for (index, sample) in frame.iter_mut().enumerate() {
            *sample = index as f32 / 32.0;
        }
        let packed = pack_frame_s32(&frame, 1.0);
        for (index, sample) in frame.iter().enumerate() {
            assert_eq!(packed[index], f32_to_s32(*sample));
        }
        assert_eq!(&packed[RENDER_CHANNELS..], &[0, 0, 0, 0]);
    }

    #[test]
    fn s32_conversion_has_explicit_endpoints_and_nan_is_safe() {
        assert_eq!(f32_to_s32(1.0), i32::MAX);
        assert_eq!(f32_to_s32(-1.0), i32::MIN);
        assert_eq!(f32_to_s32(0.0), 0);
        assert_eq!(f32_to_s32(f32::NAN), 0);
        assert_eq!(f32_to_s32(2.0), i32::MAX);
        assert_eq!(f32_to_s32(-2.0), i32::MIN);
    }

    #[test]
    fn controller_sign_slew_and_clamp_are_correct() {
        let mut high = AdaptiveClockController::new(4_800, 300.0);
        let first_high = high.update(7_200, 0.01);
        assert!(first_high > 0.0 && first_high <= 4.01);
        for _ in 0..2_000 {
            high.update(100_000, 0.01);
        }
        assert!(high.last_ppm <= 300.0);
        assert!(high.last_ppm > 250.0);

        let mut low = AdaptiveClockController::new(4_800, 300.0);
        assert!(low.update(2_400, 0.01) < 0.0);
        for _ in 0..2_000 {
            low.update(0, 0.01);
        }
        assert!(low.last_ppm >= -300.0);
        assert!(low.last_ppm < -250.0);
    }

    #[test]
    fn controller_reset_discards_recovery_history() {
        let mut controller = AdaptiveClockController::new(4_800, 300.0);
        for _ in 0..200 {
            controller.update(7_200, 0.01);
        }
        assert!(controller.last_ppm > 0.0);
        controller.reset_after_discontinuity();
        assert_eq!(controller.last_ppm, 0.0);
        assert_eq!(controller.integral_error_seconds, 0.0);
    }

    #[test]
    fn controller_is_zero_at_target_from_clean_state() {
        let mut controller = AdaptiveClockController::new(4_800, 300.0);
        assert!(controller.update(4_800, 0.01).abs() < 1.0e-12);
    }

    #[test]
    fn bandlimited_resampler_at_unity_reproduces_source_sequence() {
        let mut index = 0usize;
        let mut next = || -> Result<Frame12, ()> {
            let mut frame = [0.0; RENDER_CHANNELS];
            frame[0] = index as f32;
            index += 1;
            Ok(frame)
        };
        let mut resampler = BandlimitedResampler12::new();
        resampler.prime(&mut next).unwrap();
        for expected in 0..64 {
            let frame = resampler.render(1.0, &mut next).unwrap();
            assert!((frame[0] - expected as f32).abs() < 1.0e-4);
        }
    }

    #[test]
    fn bandlimited_resampler_keeps_constant_signal_constant() {
        let mut next = || -> Result<Frame12, ()> { Ok([0.25; RENDER_CHANNELS]) };
        let mut resampler = BandlimitedResampler12::new();
        resampler.prime(&mut next).unwrap();
        for step in [0.9997, 1.0, 1.0003] {
            for _ in 0..2_000 {
                let frame = resampler.render(step, &mut next).unwrap();
                assert!(frame.iter().all(|sample| (*sample - 0.25).abs() < 2.0e-6));
            }
        }
    }

    fn max_sine_error(frequency_hz: f64, step: f64) -> f64 {
        let omega = 2.0 * PI * frequency_hz / f64::from(OUTPUT_SAMPLE_RATE);
        let mut source_index = 0usize;
        let mut next = || -> Result<Frame12, ()> {
            let mut frame = [0.0; RENDER_CHANNELS];
            frame[0] = (omega * source_index as f64).sin() as f32;
            source_index += 1;
            Ok(frame)
        };
        let mut resampler = BandlimitedResampler12::new();
        resampler.prime(&mut next).unwrap();

        let mut max_error = 0.0_f64;
        for output_index in 0..8_000usize {
            let frame = resampler.render(step, &mut next).unwrap();
            if output_index > 64 {
                let expected = (omega * output_index as f64 * step).sin();
                max_error = max_error.max((f64::from(frame[0]) - expected).abs());
            }
        }
        max_error
    }

    #[test]
    fn bandlimited_resampler_preserves_top_octave_at_max_clock_correction() {
        for step in [0.9997_f64, 1.0003_f64] {
            let error_18k = max_sine_error(18_000.0, step);
            assert!(
                error_18k < 0.004,
                "18 kHz step={step} max error {error_18k}"
            );

            let error_20k = max_sine_error(20_000.0, step);
            assert!(
                error_20k < 0.015,
                "20 kHz step={step} max error {error_20k}"
            );
        }
    }

    #[test]
    fn gain_conversion_matches_common_reference_points() {
        assert!((db_to_linear(0.0) - 1.0).abs() < 1.0e-6);
        assert!((db_to_linear(-6.0206) - 0.5).abs() < 0.0001);
    }
}
