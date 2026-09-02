//! Deterministic output-side primitives for the Aurora R1 direct-ALSA path.
//!
//! The live R1 contract is fixed at 48 kHz with twelve rendered speaker
//! channels (`FL FR C LFE BL BR SL SR TFL TFR TRL TRR`) packed into a
//! sixteen-slot TDM stream. Slots 13..16 are deliberately zeroed.
//!
//! This module is platform-independent so the channel contract, sample
//! conversion, adaptive clock controller and fractional resampler can be
//! validated in the normal workspace CI. The Linux ALSA device wrapper lives
//! in the `aurora-alsa-out` binary.

/// Number of rendered channels produced by Omniphony for Aurora R1.
pub const RENDER_CHANNELS: usize = 12;
/// Number of physical slots on the SAI3 TDM512 link.
pub const TDM_CHANNELS: usize = 16;
/// Fixed Aurora R1 output sample rate.
pub const OUTPUT_SAMPLE_RATE: u32 = 48_000;

/// One interleaved logical 7.1.4 sample frame.
pub type Frame12 = [f32; RENDER_CHANNELS];

/// PI controller used to keep the software queue centred while the upstream
/// media clock and the DAC clock differ by a small number of ppm.
#[derive(Debug, Clone)]
pub struct AdaptiveClockController {
    target_frames: usize,
    max_ppm: f64,
    kp_ppm: f64,
    ki_ppm_per_second: f64,
    integral_error_seconds: f64,
}

impl AdaptiveClockController {
    /// Creates a controller targeting `target_frames` queued source frames.
    ///
    /// `max_ppm` is a hard safety clamp on the resampling correction. The
    /// defaults used by the binary are intentionally conservative because the
    /// eARC source and DAC clocks should differ only by oscillator tolerance.
    pub fn new(target_frames: usize, max_ppm: f64) -> Self {
        Self {
            target_frames: target_frames.max(1),
            max_ppm: max_ppm.abs().max(1.0),
            kp_ppm: 1_200.0,
            ki_ppm_per_second: 120.0,
            integral_error_seconds: 0.0,
        }
    }

    /// Updates the controller and returns the source-consumption correction in
    /// parts per million. Positive ppm means consume source frames faster.
    pub fn update(&mut self, queued_frames: usize, interval_seconds: f64) -> f64 {
        let target = self.target_frames as f64;
        let normalized_error = (queued_frames as f64 - target) / target;
        let dt = interval_seconds.max(0.0);
        self.integral_error_seconds =
            (self.integral_error_seconds + normalized_error * dt).clamp(-2.0, 2.0);
        let ppm =
            self.kp_ppm * normalized_error + self.ki_ppm_per_second * self.integral_error_seconds;
        ppm.clamp(-self.max_ppm, self.max_ppm)
    }

    /// Converts a ppm correction to the source-frame step used by the
    /// fractional resampler.
    pub fn step_from_ppm(ppm: f64) -> f64 {
        1.0 + ppm * 1.0e-6
    }

    /// Returns the configured queue target.
    pub fn target_frames(&self) -> usize {
        self.target_frames
    }
}

/// Streaming four-point cubic interpolator for twelve-channel PCM.
///
/// At the tiny correction ratios used for clock matching this avoids the
/// obvious high-frequency droop of linear interpolation without requiring a
/// large FFT/sinc state or another runtime dependency. It is not intended for
/// large sample-rate conversions.
#[derive(Debug, Clone)]
pub struct CubicResampler12 {
    frames: [Frame12; 4],
    phase: f64,
    primed: bool,
}

impl Default for CubicResampler12 {
    fn default() -> Self {
        Self {
            frames: [[0.0; RENDER_CHANNELS]; 4],
            phase: 0.0,
            primed: false,
        }
    }
}

impl CubicResampler12 {
    /// Primes the interpolator from a source callback.
    ///
    /// The first source frame is duplicated as the pre-roll sample so the
    /// first rendered frame corresponds to the first real source frame.
    pub fn prime<F, E>(&mut self, next: &mut F) -> Result<(), E>
    where
        F: FnMut() -> Result<Frame12, E>,
    {
        let first = next()?;
        self.frames[0] = first;
        self.frames[1] = first;
        self.frames[2] = next()?;
        self.frames[3] = next()?;
        self.phase = 0.0;
        self.primed = true;
        Ok(())
    }

    /// Produces one output frame and advances through source frames according
    /// to `source_step` (normally very close to `1.0`).
    pub fn render<F, E>(&mut self, source_step: f64, next: &mut F) -> Result<Frame12, E>
    where
        F: FnMut() -> Result<Frame12, E>,
    {
        debug_assert!(self.primed, "CubicResampler12 must be primed first");
        let t = self.phase as f32;
        let t2 = t * t;
        let t3 = t2 * t;
        let mut out = [0.0_f32; RENDER_CHANNELS];
        for channel in 0..RENDER_CHANNELS {
            let p0 = self.frames[0][channel];
            let p1 = self.frames[1][channel];
            let p2 = self.frames[2][channel];
            let p3 = self.frames[3][channel];
            out[channel] = 0.5
                * ((2.0 * p1)
                    + (-p0 + p2) * t
                    + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
                    + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3);
        }

        self.phase += source_step.clamp(0.999, 1.001);
        while self.phase >= 1.0 {
            self.frames[0] = self.frames[1];
            self.frames[1] = self.frames[2];
            self.frames[2] = self.frames[3];
            self.frames[3] = next()?;
            self.phase -= 1.0;
        }
        Ok(out)
    }
}

/// Packs one logical 12-channel frame into the canonical sixteen TDM slots and
/// converts it to signed 32-bit PCM. The final four slots are always zero.
pub fn pack_frame_s32(frame: &Frame12, linear_gain: f32) -> [i32; TDM_CHANNELS] {
    let mut out = [0_i32; TDM_CHANNELS];
    for (dst, sample) in out[..RENDER_CHANNELS].iter_mut().zip(frame.iter()) {
        *dst = f32_to_s32(*sample * linear_gain);
    }
    out
}

/// Converts a floating-point PCM sample to full-scale signed 32-bit PCM with
/// explicit saturation and well-defined endpoint handling.
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

/// Converts decibels to a linear gain multiplier.
pub fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_contract_preserves_first_twelve_and_zeros_reserve_slots() {
        let mut frame = [0.0_f32; RENDER_CHANNELS];
        for (index, sample) in frame.iter_mut().enumerate() {
            *sample = index as f32 / 32.0;
        }
        let packed = pack_frame_s32(&frame, 1.0);
        for index in 0..RENDER_CHANNELS {
            assert_eq!(packed[index], f32_to_s32(frame[index]));
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
    fn controller_sign_and_clamp_are_correct() {
        let mut controller = AdaptiveClockController::new(4_800, 300.0);
        assert!(controller.update(7_200, 0.01) > 0.0);
        assert!(controller.update(2_400, 0.01) < 0.0);
        assert!(controller.update(100_000, 0.01) <= 300.0);
        assert!(controller.update(0, 0.01) >= -300.0);
    }

    #[test]
    fn controller_is_near_zero_at_target() {
        let mut controller = AdaptiveClockController::new(4_800, 300.0);
        assert!(controller.update(4_800, 0.01).abs() < 1.0e-9);
    }

    #[test]
    fn cubic_resampler_at_unity_reproduces_source_sequence() {
        let mut index = 0usize;
        let mut next = || -> Result<Frame12, ()> {
            let mut frame = [0.0; RENDER_CHANNELS];
            frame[0] = index as f32;
            index += 1;
            Ok(frame)
        };
        let mut resampler = CubicResampler12::default();
        resampler.prime(&mut next).unwrap();
        for expected in 0..16 {
            let frame = resampler.render(1.0, &mut next).unwrap();
            assert!((frame[0] - expected as f32).abs() < 1.0e-5);
        }
    }

    #[test]
    fn cubic_resampler_keeps_constant_signal_constant_during_ppm_correction() {
        let mut next = || -> Result<Frame12, ()> { Ok([0.25; RENDER_CHANNELS]) };
        let mut resampler = CubicResampler12::default();
        resampler.prime(&mut next).unwrap();
        for step in [0.9997, 1.0, 1.0003] {
            for _ in 0..2_000 {
                let frame = resampler.render(step, &mut next).unwrap();
                assert!(frame.iter().all(|sample| (*sample - 0.25).abs() < 1.0e-6));
            }
        }
    }

    #[test]
    fn gain_conversion_matches_common_reference_points() {
        assert!((db_to_linear(0.0) - 1.0).abs() < 1.0e-6);
        assert!((db_to_linear(-6.0206) - 0.5).abs() < 0.0001);
    }
}
