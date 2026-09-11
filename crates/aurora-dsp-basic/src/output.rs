//! Canonical 48 kHz output DSP used by the S6 stream and host validation.
//! Audio is interleaved in the configured Aurora output-layout order. The
//! current storage implementation remains twelve channels while layout
//! semantics (LFE, height lanes and calibration roles) are contract-driven.
//! Construction may allocate; processing and control setters never allocate or
//! access the operating system.

use anyhow::{bail, Result};
use aurora_core::StandardLayout;
use serde::{Deserialize, Serialize};
use std::f32::consts::PI;

use crate::output_layout::OutputLayoutContract;

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: usize = 12;
const MAX_LIPSYNC_FRAMES: usize = 24_000;
const LIPSYNC_RING_FRAMES: usize = MAX_LIPSYNC_FRAMES + 1;

fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

/// Validated setup parameters for the shared cinema output chain.
#[derive(Debug, Clone, Copy)]
pub struct OutputDspConfig {
    pub bed_crossover_hz: f32,
    pub height_crossover_hz: f32,
    pub lfe_lowpass_hz: f32,
    pub sub_highpass_hz: f32,
    pub lfe_trim_db: f32,
    pub redirected_bass_db: f32,
    pub headroom_db: f32,
    pub limiter_dbfs: f32,
    pub limiter_release_ms: f32,
    pub lipsync_frames: usize,
}

impl Default for OutputDspConfig {
    fn default() -> Self {
        Self {
            bed_crossover_hz: 80.0,
            height_crossover_hz: 100.0,
            lfe_lowpass_hz: 120.0,
            sub_highpass_hz: 20.0,
            lfe_trim_db: 0.0,
            redirected_bass_db: 0.0,
            headroom_db: -3.0,
            limiter_dbfs: -1.0,
            limiter_release_ms: 50.0,
            lipsync_frames: 0,
        }
    }
}

/// One bounded setup-time parametric equalizer band.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeqBand {
    pub frequency_hz: f32,
    pub q: f32,
    pub gain_db: f32,
}

/// Measured or manually supplied speaker correction, applied after bass routing.
/// This is not automatic room measurement or a claim of acoustic calibration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelCalibration {
    pub role: aurora_core::ChannelRole,
    pub trim_db: f32,
    pub delay_frames: usize,
    pub invert_polarity: bool,
    pub peq: Vec<PeqBand>,
}

/// Versioned configuration whose channel roles must match the active output
/// layout contract exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeakerCalibration {
    pub schema_version: u32,
    pub sample_rate: u32,
    pub channels: Vec<ChannelCalibration>,
}

const MAX_PEQ_BANDS: usize = 8;
const CALIBRATION_DELAY_FRAMES: usize = 4_801;

struct PreparedCalibration {
    filters: [[Biquad; MAX_PEQ_BANDS]; CHANNELS],
    bands: [usize; CHANNELS],
    gains: [f32; CHANNELS],
    delays: [usize; CHANNELS],
    ring: Vec<f32>,
    write: usize,
}

impl PreparedCalibration {
    fn flat() -> Self {
        Self {
            filters: [[Biquad::default(); MAX_PEQ_BANDS]; CHANNELS],
            bands: [0; CHANNELS],
            gains: [1.0; CHANNELS],
            delays: [0; CHANNELS],
            ring: vec![0.0; CALIBRATION_DELAY_FRAMES * CHANNELS],
            write: 0,
        }
    }

    fn prepare(config: &SpeakerCalibration, layout: &OutputLayoutContract) -> Result<Self> {
        if config.schema_version != 1
            || config.sample_rate != SAMPLE_RATE
            || layout.channel_count() != CHANNELS
            || config.channels.len() != layout.channel_count()
        {
            bail!(
                "calibration requires schema 1, 48000 Hz and exactly the active output-layout channels"
            );
        }
        let mut prepared = Self::flat();
        for (index, (channel, expected)) in config
            .channels
            .iter()
            .zip(layout.channels().iter())
            .enumerate()
        {
            if channel.role != expected.role
                || channel.delay_frames >= CALIBRATION_DELAY_FRAMES
                || !channel.trim_db.is_finite()
                || !(-24.0..=12.0).contains(&channel.trim_db)
                || channel.peq.len() > MAX_PEQ_BANDS
            {
                bail!("invalid calibration role, delay, trim or band count at channel {index}");
            }
            prepared.gains[index] =
                db_to_linear(channel.trim_db) * if channel.invert_polarity { -1.0 } else { 1.0 };
            prepared.delays[index] = channel.delay_frames;
            prepared.bands[index] = channel.peq.len();
            for (slot, band) in channel.peq.iter().enumerate() {
                if !band.frequency_hz.is_finite()
                    || !(10.0..23_520.0).contains(&band.frequency_hz)
                    || !band.q.is_finite()
                    || !(0.1..=20.0).contains(&band.q)
                    || !band.gain_db.is_finite()
                    || !(-24.0..=12.0).contains(&band.gain_db)
                {
                    bail!("invalid PEQ band at channel {index}");
                }
                let a = 10.0_f32.powf(band.gain_db / 40.0);
                let omega = 2.0 * PI * band.frequency_hz / SAMPLE_RATE as f32;
                let alpha = omega.sin() / (2.0 * band.q);
                let denominator = 1.0 + alpha / a;
                prepared.filters[index][slot] = Biquad {
                    b0: (1.0 + alpha * a) / denominator,
                    b1: -2.0 * omega.cos() / denominator,
                    b2: (1.0 - alpha * a) / denominator,
                    a1: -2.0 * omega.cos() / denominator,
                    a2: (1.0 - alpha / a) / denominator,
                    z1: 0.0,
                    z2: 0.0,
                };
            }
        }
        Ok(prepared)
    }

    fn process_frame(&mut self, frame: &mut [f32]) {
        for (channel, sample) in frame.iter_mut().enumerate() {
            for filter in &mut self.filters[channel][..self.bands[channel]] {
                *sample = filter.process(*sample);
            }
            self.ring[self.write * CHANNELS + channel] = *sample * self.gains[channel];
            let read = (self.write + CALIBRATION_DELAY_FRAMES - self.delays[channel])
                % CALIBRATION_DELAY_FRAMES;
            *sample = self.ring[read * CHANNELS + channel];
        }
        self.write = (self.write + 1) % CALIBRATION_DELAY_FRAMES;
    }

    fn reset(&mut self) {
        for filter in self.filters.iter_mut().flatten() {
            filter.reset();
        }
        self.ring.fill(0.0);
        self.write = 0;
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn low_pass(sample_rate: f32, frequency: f32, q: f32) -> Result<Self> {
        Self::design(sample_rate, frequency, q, false)
    }

    fn high_pass(sample_rate: f32, frequency: f32, q: f32) -> Result<Self> {
        Self::design(sample_rate, frequency, q, true)
    }

    fn design(sample_rate: f32, frequency: f32, q: f32, high_pass: bool) -> Result<Self> {
        if !sample_rate.is_finite()
            || !frequency.is_finite()
            || !q.is_finite()
            || sample_rate <= 0.0
            || frequency <= 0.0
            || frequency >= sample_rate * 0.49
            || q <= 0.0
        {
            bail!("invalid biquad design");
        }
        let omega = 2.0 * PI * frequency / sample_rate;
        let cos = omega.cos();
        let sin = omega.sin();
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha;
        let (b0, b1, b2) = if high_pass {
            ((1.0 + cos) * 0.5, -(1.0 + cos), (1.0 + cos) * 0.5)
        } else {
            ((1.0 - cos) * 0.5, 1.0 - cos, (1.0 - cos) * 0.5)
        };
        Ok(Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: (-2.0 * cos) / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        })
    }

    #[inline]
    fn process(&mut self, input: f32) -> f32 {
        if !input.is_finite() {
            self.reset();
            return 0.0;
        }
        let output = self.b0 * input + self.z1;
        self.z1 = self.b1 * input - self.a1 * output + self.z2;
        self.z2 = self.b2 * input - self.a2 * output;
        if output.is_finite() {
            output
        } else {
            self.reset();
            0.0
        }
    }

    fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Crossover {
    low_1: Biquad,
    low_2: Biquad,
    high_1: Biquad,
    high_2: Biquad,
}

impl Crossover {
    fn linkwitz_riley_4(sample_rate: f32, frequency: f32) -> Result<Self> {
        let q = 1.0 / 2.0_f32.sqrt();
        let low = Biquad::low_pass(sample_rate, frequency, q)?;
        let high = Biquad::high_pass(sample_rate, frequency, q)?;
        Ok(Self {
            low_1: low,
            low_2: low,
            high_1: high,
            high_2: high,
        })
    }

    #[inline]
    fn split(&mut self, input: f32) -> (f32, f32) {
        let low = self.low_2.process(self.low_1.process(input));
        let high = self.high_2.process(self.high_1.process(input));
        (high, low)
    }

    fn reset(&mut self) {
        self.low_1.reset();
        self.low_2.reset();
        self.high_1.reset();
        self.high_2.reset();
    }
}

#[derive(Debug, Clone)]
struct LinkedLimiter {
    ceiling: f32,
    gain: f32,
    release_alpha: f32,
}

impl LinkedLimiter {
    fn new(ceiling_dbfs: f32, release_ms: f32) -> Result<Self> {
        let ceiling = db_to_linear(ceiling_dbfs);
        if !ceiling.is_finite()
            || ceiling <= 0.0
            || ceiling > 1.0
            || !release_ms.is_finite()
            || release_ms <= 0.0
        {
            bail!("invalid limiter configuration");
        }
        let release_samples = release_ms * SAMPLE_RATE as f32 / 1_000.0;
        let release_alpha = 1.0 - (-1.0 / release_samples.max(1.0)).exp();
        Ok(Self {
            ceiling,
            gain: 1.0,
            release_alpha,
        })
    }

    #[inline]
    fn process_frame(&mut self, frame: &mut [f32]) {
        let peak = frame.iter().copied().map(f32::abs).fold(0.0_f32, f32::max);
        let requested = if peak > self.ceiling && peak > 0.0 {
            self.ceiling / peak
        } else {
            1.0
        };
        if requested < self.gain {
            self.gain = requested;
        } else {
            self.gain = (self.gain + (1.0 - self.gain) * self.release_alpha).min(requested);
        }
        for sample in frame {
            *sample = (*sample * self.gain).clamp(-self.ceiling, self.ceiling);
        }
    }

    fn reset(&mut self) {
        self.gain = 1.0;
    }
}

#[derive(Debug, Clone)]
struct LipDelay {
    ring: Vec<f32>,
    write_frame: usize,
    delay_frames: usize,
    previous_delay: usize,
    requested_delay: usize,
    fade_remaining: usize,
}

impl LipDelay {
    fn new(delay_frames: usize) -> Result<Self> {
        if delay_frames > MAX_LIPSYNC_FRAMES {
            bail!("lip-sync delay exceeds 500 ms");
        }
        Ok(Self {
            ring: vec![0.0; LIPSYNC_RING_FRAMES * CHANNELS],
            write_frame: 0,
            delay_frames,
            previous_delay: delay_frames,
            requested_delay: delay_frames,
            fade_remaining: 0,
        })
    }

    fn set_delay_frames(&mut self, delay_frames: usize) {
        self.requested_delay = delay_frames.min(MAX_LIPSYNC_FRAMES);
    }

    #[inline]
    fn process_frame(&mut self, frame: &mut [f32]) {
        const FADE_FRAMES: usize = 240;
        if self.fade_remaining == 0 && self.requested_delay != self.delay_frames {
            self.previous_delay = self.delay_frames;
            self.delay_frames = self.requested_delay;
            self.fade_remaining = FADE_FRAMES;
        }
        let write_base = self.write_frame * CHANNELS;
        self.ring[write_base..write_base + CHANNELS].copy_from_slice(frame);
        let read_frame =
            (self.write_frame + LIPSYNC_RING_FRAMES - self.delay_frames) % LIPSYNC_RING_FRAMES;
        let read_base = read_frame * CHANNELS;
        frame.copy_from_slice(&self.ring[read_base..read_base + CHANNELS]);
        if self.fade_remaining > 0 {
            let old_base = ((self.write_frame + LIPSYNC_RING_FRAMES - self.previous_delay)
                % LIPSYNC_RING_FRAMES)
                * CHANNELS;
            let alpha = 1.0 - self.fade_remaining as f32 / FADE_FRAMES as f32;
            for (channel, sample) in frame.iter_mut().enumerate() {
                *sample = self.ring[old_base + channel] * (1.0 - alpha) + *sample * alpha;
            }
            self.fade_remaining -= 1;
        }
        self.write_frame += 1;
        if self.write_frame == LIPSYNC_RING_FRAMES {
            self.write_frame = 0;
        }
    }

    fn reset(&mut self) {
        self.ring.fill(0.0);
        self.write_frame = 0;
        self.delay_frames = self.requested_delay;
        self.previous_delay = self.requested_delay;
        self.fade_remaining = 0;
    }
}

/// Returned without allocation when an interleaved block is incomplete.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("output block must contain complete twelve-channel frames")]
pub struct OutputShapeError;

/// One prepared cinema DSP chain shared by offline and appliance processing.
pub struct SpeakerPostProcessor {
    layout: OutputLayoutContract,
    calibration: PreparedCalibration,
    crossovers: [Crossover; CHANNELS],
    lfe_index: usize,
    lfe_low_1: Biquad,
    lfe_low_2: Biquad,
    sub_high_1: Biquad,
    sub_high_2: Biquad,
    lfe_gain: f32,
    redirected_bass_gain: f32,
    headroom_gain: f32,
    user_gain: f32,
    smoothed_master_gain: f32,
    gain_alpha: f32,
    muted: bool,
    standby: bool,
    limiter: LinkedLimiter,
    lip_delay: LipDelay,
}

impl SpeakerPostProcessor {
    /// Preserves the established product constructor by selecting Aurora's
    /// canonical 7.1.4 contract explicitly.
    pub fn new(config: OutputDspConfig) -> Result<Self> {
        let layout = OutputLayoutContract::for_standard(StandardLayout::SevenOneFour)?;
        Self::new_for_layout(config, layout)
    }

    /// Prepares all filter and delay state for an explicit output layout. The
    /// current storage implementation is intentionally still limited to twelve
    /// channels; callers get a deterministic setup error for wider layouts
    /// until the dynamic-buffer phase lands.
    pub fn new_for_layout(config: OutputDspConfig, layout: OutputLayoutContract) -> Result<Self> {
        if layout.channel_count() != CHANNELS {
            bail!(
                "current speaker postprocessor storage requires {CHANNELS} channels; layout {} has {}",
                layout.name(),
                layout.channel_count()
            );
        }
        let Some(lfe_index) = layout.lfe_index() else {
            bail!("current bass-managed speaker postprocessor requires exactly one LFE channel");
        };
        let OutputDspConfig {
            bed_crossover_hz,
            height_crossover_hz,
            lfe_lowpass_hz,
            sub_highpass_hz,
            lfe_trim_db,
            redirected_bass_db,
            headroom_db,
            limiter_dbfs,
            limiter_release_ms,
            lipsync_frames,
        } = config;
        for gain in [lfe_trim_db, redirected_bass_db, headroom_db] {
            if !gain.is_finite() || !(-80.0..=12.0).contains(&gain) {
                bail!("DSP gain must be finite and between -80 and +12 dB");
            }
        }
        let bed = Crossover::linkwitz_riley_4(SAMPLE_RATE as f32, bed_crossover_hz)?;
        let height = Crossover::linkwitz_riley_4(SAMPLE_RATE as f32, height_crossover_hz)?;
        let mut crossovers = [bed; CHANNELS];
        for &index in layout.height_indices() {
            crossovers[index] = height;
        }
        crossovers[lfe_index] = Crossover::default();
        let q = 1.0 / 2.0_f32.sqrt();
        let lfe_low = Biquad::low_pass(SAMPLE_RATE as f32, lfe_lowpass_hz, q)?;
        let sub_high = Biquad::high_pass(SAMPLE_RATE as f32, sub_highpass_hz, q)?;
        let gain_samples = 5.0 * SAMPLE_RATE as f32 / 1_000.0;
        Ok(Self {
            layout,
            calibration: PreparedCalibration::flat(),
            crossovers,
            lfe_index,
            lfe_low_1: lfe_low,
            lfe_low_2: lfe_low,
            sub_high_1: sub_high,
            sub_high_2: sub_high,
            lfe_gain: db_to_linear(lfe_trim_db),
            redirected_bass_gain: db_to_linear(redirected_bass_db),
            headroom_gain: db_to_linear(headroom_db),
            user_gain: 1.0,
            smoothed_master_gain: 0.0,
            gain_alpha: 1.0 - (-1.0 / gain_samples.max(1.0)).exp(),
            muted: false,
            standby: false,
            limiter: LinkedLimiter::new(limiter_dbfs, limiter_release_ms)?,
            lip_delay: LipDelay::new(lipsync_frames)?,
        })
    }

    pub fn layout(&self) -> &OutputLayoutContract {
        &self.layout
    }

    /// Replaces calibration transactionally at setup, never from an audio callback.
    pub fn configure_calibration(&mut self, config: &SpeakerCalibration) -> Result<()> {
        let prepared = PreparedCalibration::prepare(config, &self.layout)?;
        self.calibration = prepared;
        self.reset();
        Ok(())
    }

    pub fn set_master_gain_mdb(&mut self, milli_db: i64) {
        let db = (milli_db as f32 / 1_000.0).clamp(-80.0, 12.0);
        self.user_gain = db_to_linear(db);
    }

    pub fn set_lipsync_frames(&mut self, frames: usize) {
        self.lip_delay.set_delay_frames(frames);
    }

    pub fn set_mute(&mut self, muted: bool) {
        self.muted = muted;
    }

    pub fn set_standby(&mut self, standby: bool) {
        self.standby = standby;
    }

    pub fn process_block(
        &mut self,
        block: &mut [f32],
    ) -> std::result::Result<(), OutputShapeError> {
        if block.len() % CHANNELS != 0 {
            block.fill(0.0);
            self.reset();
            return Err(OutputShapeError);
        }
        for frame in block.chunks_exact_mut(CHANNELS) {
            for sample in frame.iter_mut() {
                if !sample.is_finite() {
                    *sample = 0.0;
                }
            }
            let original_lfe = frame[self.lfe_index];
            let mut redirected_bass = 0.0_f32;
            for (channel, sample) in frame.iter_mut().enumerate() {
                if channel == self.lfe_index {
                    continue;
                }
                let (high, low) = self.crossovers[channel].split(*sample);
                *sample = high;
                redirected_bass += low;
            }

            let lfe_band =
                self.lfe_low_2.process(self.lfe_low_1.process(original_lfe)) * self.lfe_gain;
            let summed_sub = lfe_band + redirected_bass * self.redirected_bass_gain;
            frame[self.lfe_index] = self.sub_high_2.process(self.sub_high_1.process(summed_sub));

            self.calibration.process_frame(frame);
            // Apply mute/gain after delay so a 500 ms lip-sync buffer cannot
            // postpone user mute or emit stored full-volume audio on standby.
            self.lip_delay.process_frame(frame);

            let target = if self.muted || self.standby {
                0.0
            } else {
                self.headroom_gain * self.user_gain
            };
            self.smoothed_master_gain += (target - self.smoothed_master_gain) * self.gain_alpha;
            for sample in frame.iter_mut() {
                *sample *= self.smoothed_master_gain;
            }
            self.limiter.process_frame(frame);
        }
        Ok(())
    }

    pub fn reset(&mut self) {
        self.calibration.reset();
        for crossover in &mut self.crossovers {
            crossover.reset();
        }
        self.lfe_low_1.reset();
        self.lfe_low_2.reset();
        self.sub_high_1.reset();
        self.sub_high_2.reset();
        self.limiter.reset();
        self.lip_delay.reset();
        self.smoothed_master_gain = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output_layout::{OutputChannelClass, OutputChannelSpec};
    use aurora_core::ChannelRole;

    const BLOCK_SAMPLES: usize = 40 * CHANNELS;

    fn processor() -> SpeakerPostProcessor {
        SpeakerPostProcessor::new(OutputDspConfig::default()).unwrap()
    }

    fn canonical_layout() -> OutputLayoutContract {
        OutputLayoutContract::for_standard(StandardLayout::SevenOneFour).unwrap()
    }

    #[test]
    fn prepared_calibration_applies_exact_delay_and_peq_center_gain() {
        let layout = canonical_layout();
        let mut config = SpeakerCalibration {
            schema_version: 1,
            sample_rate: SAMPLE_RATE,
            channels: layout
                .channels()
                .iter()
                .map(|channel| ChannelCalibration {
                    role: channel.role.clone(),
                    trim_db: 0.0,
                    delay_frames: 0,
                    invert_polarity: false,
                    peq: vec![],
                })
                .collect(),
        };
        config.channels[0].delay_frames = 48;
        config.channels[10].peq.push(PeqBand {
            frequency_hz: 1000.0,
            q: 1.0,
            gain_db: 6.0,
        });
        let mut prepared = PreparedCalibration::prepare(&config, &layout).unwrap();
        let mut power_in = 0.0_f64;
        let mut power_out = 0.0_f64;
        for index in 0..4800 {
            let input = 0.1 * (2.0 * PI * 1000.0 * index as f32 / SAMPLE_RATE as f32).sin();
            let mut frame = [0.0; CHANNELS];
            frame[0] = if index == 0 { 1.0 } else { 0.0 };
            frame[10] = input;
            prepared.process_frame(&mut frame);
            assert_eq!(frame[0], if index == 48 { 1.0 } else { 0.0 });
            if index >= 2400 {
                power_in += f64::from(input).powi(2);
                power_out += f64::from(frame[10]).powi(2);
            }
        }
        assert!(((power_out / power_in).sqrt() - f64::from(db_to_linear(6.0))).abs() < 0.002);
    }

    #[test]
    fn explicit_layout_drives_lfe_and_height_indices() {
        let mut channels = vec![
            OutputChannelSpec {
                role: ChannelRole::FrontLeft,
                class: OutputChannelClass::Bed,
            },
            OutputChannelSpec {
                role: ChannelRole::FrontRight,
                class: OutputChannelClass::Bed,
            },
        ];
        for index in 2..12 {
            let (role, class) = if index == 5 {
                (
                    ChannelRole::Custom("sub".to_owned()),
                    OutputChannelClass::Lfe,
                )
            } else if index >= 10 {
                (
                    ChannelRole::Custom(format!("height-{index}")),
                    OutputChannelClass::Height,
                )
            } else {
                (
                    ChannelRole::Custom(format!("bed-{index}")),
                    OutputChannelClass::Bed,
                )
            };
            channels.push(OutputChannelSpec { role, class });
        }
        let layout = OutputLayoutContract::custom("test-12", channels).unwrap();
        let post = SpeakerPostProcessor::new_for_layout(OutputDspConfig::default(), layout).unwrap();
        assert_eq!(post.lfe_index, 5);
        assert_eq!(post.layout().height_indices(), &[10, 11]);
    }

    #[test]
    fn wider_layout_is_rejected_until_dynamic_storage_lands() {
        let mut channels = Vec::new();
        for index in 0..16 {
            let class = if index == 3 {
                OutputChannelClass::Lfe
            } else if index >= 12 {
                OutputChannelClass::Height
            } else {
                OutputChannelClass::Bed
            };
            channels.push(OutputChannelSpec {
                role: ChannelRole::Custom(format!("lane-{index}")),
                class,
            });
        }
        let layout = OutputLayoutContract::custom("future-16", channels).unwrap();
        assert!(SpeakerPostProcessor::new_for_layout(OutputDspConfig::default(), layout).is_err());
    }

    #[test]
    fn linked_limiter_never_exceeds_ceiling_and_preserves_channel_ratio() {
        let mut limiter = LinkedLimiter::new(-1.0, 50.0).unwrap();
        let mut frame = [0.0_f32; CHANNELS];
        frame[0] = 2.0;
        frame[1] = 1.0;
        limiter.process_frame(&mut frame);
        let ceiling = db_to_linear(-1.0);
        assert!(frame.iter().all(|sample| sample.abs() <= ceiling + 1.0e-6));
        assert!((frame[0] / frame[1] - 2.0).abs() < 1.0e-5);
    }

    #[test]
    fn lip_delay_delays_all_channels_by_exact_frames() {
        let mut delay = LipDelay::new(2).unwrap();
        let mut first = [0.0_f32; CHANNELS];
        first[0] = 1.0;
        delay.process_frame(&mut first);
        assert_eq!(first[0], 0.0);
        let mut second = [0.0_f32; CHANNELS];
        delay.process_frame(&mut second);
        assert_eq!(second[0], 0.0);
        let mut third = [0.0_f32; CHANNELS];
        delay.process_frame(&mut third);
        assert_eq!(third[0], 1.0);
    }

    #[test]
    fn speaker_postprocessor_is_finite_and_peak_bounded() {
        let mut post = processor();
        let mut block = vec![0.0_f32; BLOCK_SAMPLES];
        for (index, sample) in block.iter_mut().enumerate() {
            *sample = ((index as f32 * 0.071).sin() * 1.8).clamp(-1.8, 1.8);
        }
        for _ in 0..200 {
            post.process_block(&mut block).unwrap();
            assert!(block.iter().all(|sample| sample.is_finite()));
            assert!(block
                .iter()
                .all(|sample| sample.abs() <= db_to_linear(-1.0) + 1.0e-6));
        }
    }
}
