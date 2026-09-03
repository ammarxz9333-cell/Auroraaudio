//! Low-latency post-render processing for the Galaxy S6 Aurora appliance.
//!
//! stdin:  raw interleaved f32, 48 kHz, 12-channel 7.1.4 from Omniphony
//! stdout: raw interleaved f32, 48 kHz, 12-channel 7.1.4 in 40-frame quanta
//!
//! An optional inherited control pipe (fd 3 by default) carries fixed-size
//! CLOCK_REPORT and local-control messages from `aurora-live-ingest`. Control is
//! consumed on a dedicated thread; the audio loop observes atomics only.

use anyhow::{bail, Context, Result};
use aurora_realtime_engine::{
    AsynchronousResampler, DriftController, DriftControllerConfig, RubatoAsrc,
};
use std::f32::consts::PI;
use std::fs::File;
use std::io::{self, ErrorKind, Read, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: usize = 12;
const BLOCK_FRAMES: usize = 40;
const BLOCK_SAMPLES: usize = CHANNELS * BLOCK_FRAMES;
const READ_FRAMES: usize = 8;
const READ_SAMPLES: usize = READ_FRAMES * CHANNELS;
const READ_BYTES: usize = READ_SAMPLES * 4;
const LFE: usize = 3;
const MAX_ASRC_INPUT_FRAMES: usize = 128;
const INPUT_QUEUE_FRAMES: usize = 512;
const MAX_LIPSYNC_FRAMES: usize = 24_000; // 500 ms at 48 kHz.
const LIPSYNC_RING_FRAMES: usize = MAX_LIPSYNC_FRAMES + 1;
const CONTROL_MESSAGE_BYTES: usize = 32;
const CONTROL_MAGIC: [u8; 4] = *b"APC0";
const CONTROL_VERSION: u16 = 1;

const CTRL_RESET: u16 = 1;
const CTRL_CLOCK_REPORT: u16 = 2;
const CTRL_LIPSYNC_FRAMES: u16 = 3;
const CTRL_MASTER_GAIN_MDB: u16 = 4;
const CTRL_MUTE: u16 = 5;
const CTRL_STANDBY: u16 = 6;
const CTRL_SOURCE_FORMAT: u16 = 7;

fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

fn env_f32(name: &str, default: f32) -> Result<f32> {
    match std::env::var(name) {
        Ok(value) => value
            .parse::<f32>()
            .with_context(|| format!("invalid {name}={value}")),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error.into()),
    }
}

fn env_usize(name: &str, default: usize) -> Result<usize> {
    match std::env::var(name) {
        Ok(value) => value
            .parse::<usize>()
            .with_context(|| format!("invalid {name}={value}")),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error.into()),
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
        let output = self.b0 * input + self.z1;
        self.z1 = self.b1 * input - self.a1 * output + self.z2;
        self.z2 = self.b2 * input - self.a2 * output;
        if output.is_finite() {
            output
        } else {
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
        let peak = frame
            .iter()
            .copied()
            .map(f32::abs)
            .fold(0.0_f32, f32::max);
        let requested = if peak > self.ceiling && peak > 0.0 {
            self.ceiling / peak
        } else {
            1.0
        };
        if requested < self.gain {
            self.gain = requested;
        } else {
            self.gain += (1.0 - self.gain) * self.release_alpha;
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
        })
    }

    fn set_delay_frames(&mut self, delay_frames: usize) {
        self.delay_frames = delay_frames.min(MAX_LIPSYNC_FRAMES);
    }

    #[inline]
    fn process_frame(&mut self, frame: &mut [f32]) {
        let write_base = self.write_frame * CHANNELS;
        self.ring[write_base..write_base + CHANNELS].copy_from_slice(frame);
        let read_frame =
            (self.write_frame + LIPSYNC_RING_FRAMES - self.delay_frames) % LIPSYNC_RING_FRAMES;
        let read_base = read_frame * CHANNELS;
        frame.copy_from_slice(&self.ring[read_base..read_base + CHANNELS]);
        self.write_frame += 1;
        if self.write_frame == LIPSYNC_RING_FRAMES {
            self.write_frame = 0;
        }
    }

    fn reset(&mut self) {
        self.ring.fill(0.0);
        self.write_frame = 0;
    }
}

struct SpeakerPostProcessor {
    crossovers: [Crossover; CHANNELS],
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
    fn from_environment() -> Result<Self> {
        let bed_crossover_hz = env_f32("AURORA_BED_CROSSOVER_HZ", 80.0)?;
        let height_crossover_hz = env_f32("AURORA_HEIGHT_CROSSOVER_HZ", 100.0)?;
        let lfe_lowpass_hz = env_f32("AURORA_LFE_LOWPASS_HZ", 120.0)?;
        let sub_highpass_hz = env_f32("AURORA_SUB_HIGHPASS_HZ", 20.0)?;
        // 0 dB is intentionally conservative until the physical decoder/render
        // chain is level-calibrated. This avoids accidentally applying the LFE
        // playback convention twice.
        let lfe_trim_db = env_f32("AURORA_LFE_TRIM_DB", 0.0)?;
        let redirected_bass_db = env_f32("AURORA_REDIRECTED_BASS_DB", 0.0)?;
        let headroom_db = env_f32("AURORA_HEADROOM_DB", -3.0)?;
        let limiter_dbfs = env_f32("AURORA_LIMITER_DBFS", -1.0)?;
        let limiter_release_ms = env_f32("AURORA_LIMITER_RELEASE_MS", 50.0)?;
        let lipsync_ms = env_f32("AURORA_LIPSYNC_MS", 0.0)?.clamp(0.0, 500.0);
        let lipsync_frames =
            (lipsync_ms * SAMPLE_RATE as f32 / 1_000.0).round() as usize;
        Self::new(
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
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        bed_crossover_hz: f32,
        height_crossover_hz: f32,
        lfe_lowpass_hz: f32,
        sub_highpass_hz: f32,
        lfe_trim_db: f32,
        redirected_bass_db: f32,
        headroom_db: f32,
        limiter_dbfs: f32,
        limiter_release_ms: f32,
        lipsync_frames: usize,
    ) -> Result<Self> {
        let bed = Crossover::linkwitz_riley_4(SAMPLE_RATE as f32, bed_crossover_hz)?;
        let height = Crossover::linkwitz_riley_4(SAMPLE_RATE as f32, height_crossover_hz)?;
        let mut crossovers = [bed; CHANNELS];
        for crossover in &mut crossovers[8..12] {
            *crossover = height;
        }
        crossovers[LFE] = Crossover::default();
        let q = 1.0 / 2.0_f32.sqrt();
        let lfe_low = Biquad::low_pass(SAMPLE_RATE as f32, lfe_lowpass_hz, q)?;
        let sub_high = Biquad::high_pass(SAMPLE_RATE as f32, sub_highpass_hz, q)?;
        let gain_samples = 5.0 * SAMPLE_RATE as f32 / 1_000.0;
        Ok(Self {
            crossovers,
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

    fn set_master_gain_mdb(&mut self, milli_db: i64) {
        let db = (milli_db as f32 / 1_000.0).clamp(-80.0, 12.0);
        self.user_gain = db_to_linear(db);
    }

    fn set_lipsync_frames(&mut self, frames: usize) {
        self.lip_delay.set_delay_frames(frames);
    }

    fn set_mute(&mut self, muted: bool) {
        self.muted = muted;
    }

    fn set_standby(&mut self, standby: bool) {
        self.standby = standby;
    }

    fn process_block(&mut self, block: &mut [f32]) {
        for frame in block.chunks_exact_mut(CHANNELS) {
            let original_lfe = frame[LFE];
            let mut redirected_bass = 0.0_f32;
            for (channel, sample) in frame.iter_mut().enumerate() {
                if channel == LFE {
                    continue;
                }
                let (high, low) = self.crossovers[channel].split(*sample);
                *sample = high;
                redirected_bass += low;
            }

            let lfe_band = self
                .lfe_low_2
                .process(self.lfe_low_1.process(original_lfe))
                * self.lfe_gain;
            let summed_sub = lfe_band + redirected_bass * self.redirected_bass_gain;
            frame[LFE] = self
                .sub_high_2
                .process(self.sub_high_1.process(summed_sub));

            let target = if self.muted || self.standby {
                0.0
            } else {
                self.headroom_gain * self.user_gain
            };
            self.smoothed_master_gain +=
                (target - self.smoothed_master_gain) * self.gain_alpha;
            for sample in frame.iter_mut() {
                *sample *= self.smoothed_master_gain;
            }
            self.limiter.process_frame(frame);
            self.lip_delay.process_frame(frame);
        }
    }

    fn reset(&mut self) {
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

struct FixedFrameQueue {
    storage: Vec<f32>,
    capacity_frames: usize,
    read_frame: usize,
    len_frames: usize,
}

impl FixedFrameQueue {
    fn new(capacity_frames: usize) -> Self {
        Self {
            storage: vec![0.0; capacity_frames * CHANNELS],
            capacity_frames,
            read_frame: 0,
            len_frames: 0,
        }
    }

    fn clear(&mut self) {
        self.read_frame = 0;
        self.len_frames = 0;
    }

    fn push_interleaved(&mut self, input: &[f32]) -> Result<()> {
        if input.len() % CHANNELS != 0 {
            bail!("unaligned interleaved input");
        }
        let frames = input.len() / CHANNELS;
        if frames > self.capacity_frames.saturating_sub(self.len_frames) {
            bail!("postprocessor input queue overflow");
        }
        for frame_index in 0..frames {
            let destination_frame =
                (self.read_frame + self.len_frames + frame_index) % self.capacity_frames;
            let destination = destination_frame * CHANNELS;
            let source = frame_index * CHANNELS;
            self.storage[destination..destination + CHANNELS]
                .copy_from_slice(&input[source..source + CHANNELS]);
        }
        self.len_frames += frames;
        Ok(())
    }

    fn pop_frames(&mut self, output: &mut [f32], frames: usize) -> Result<()> {
        if frames > self.len_frames || output.len() != frames * CHANNELS {
            bail!("postprocessor input queue underflow");
        }
        for frame_index in 0..frames {
            let source_frame = (self.read_frame + frame_index) % self.capacity_frames;
            let source = source_frame * CHANNELS;
            let destination = frame_index * CHANNELS;
            output[destination..destination + CHANNELS]
                .copy_from_slice(&self.storage[source..source + CHANNELS]);
        }
        self.read_frame = (self.read_frame + frames) % self.capacity_frames;
        self.len_frames -= frames;
        Ok(())
    }
}

#[derive(Default)]
struct ControlState {
    reset_epoch: AtomicU64,
    clock_epoch: AtomicU64,
    sink_counter: AtomicU64,
    source_counter: AtomicU64,
    queued_playback_frames: AtomicUsize,
    capture_flags: AtomicU32,
    lipsync_frames: AtomicUsize,
    master_gain_mdb: AtomicI64,
    muted: AtomicBool,
    standby: AtomicBool,
}

fn read_le16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

fn read_le32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn read_le64(bytes: &[u8]) -> u64 {
    u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

fn apply_control_message(state: &ControlState, message: &[u8; CONTROL_MESSAGE_BYTES]) {
    if message[0..4] != CONTROL_MAGIC || read_le16(&message[4..6]) != CONTROL_VERSION {
        return;
    }
    let kind = read_le16(&message[6..8]);
    let data0 = read_le64(&message[8..16]);
    let data1 = read_le64(&message[16..24]);
    let data2 = read_le32(&message[24..28]);
    let flags = read_le32(&message[28..32]);
    match kind {
        CTRL_RESET | CTRL_SOURCE_FORMAT => {
            state.reset_epoch.fetch_add(1, Ordering::Release);
        }
        CTRL_CLOCK_REPORT => {
            state.sink_counter.store(data0, Ordering::Relaxed);
            state.source_counter.store(data1, Ordering::Relaxed);
            state
                .queued_playback_frames
                .store(data2 as usize, Ordering::Relaxed);
            state.capture_flags.store(flags, Ordering::Relaxed);
            state.clock_epoch.fetch_add(1, Ordering::Release);
        }
        CTRL_LIPSYNC_FRAMES => {
            state
                .lipsync_frames
                .store((data0 as usize).min(MAX_LIPSYNC_FRAMES), Ordering::Release);
        }
        CTRL_MASTER_GAIN_MDB => {
            state
                .master_gain_mdb
                .store(data0 as i64, Ordering::Release);
        }
        CTRL_MUTE => state.muted.store(data0 != 0, Ordering::Release),
        CTRL_STANDBY => state.standby.store(data0 != 0, Ordering::Release),
        _ => {}
    }
}

fn spawn_control_reader(fd: RawFd, state: Arc<ControlState>) {
    std::thread::Builder::new()
        .name("aurora-post-control".to_owned())
        .spawn(move || {
            // SAFETY: the descriptor is intentionally transferred to this
            // thread by the execing parent and is owned here until thread exit.
            let mut file = unsafe { File::from_raw_fd(fd) };
            let mut message = [0_u8; CONTROL_MESSAGE_BYTES];
            loop {
                match file.read_exact(&mut message) {
                    Ok(()) => apply_control_message(&state, &message),
                    Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        })
        .expect("control thread creation must succeed before streaming");
}

fn read_exact_or_clean_eof<R: Read>(reader: &mut R, buffer: &mut [u8]) -> io::Result<bool> {
    let mut read_total = 0;
    while read_total < buffer.len() {
        match reader.read(&mut buffer[read_total..]) {
            Ok(0) if read_total == 0 => return Ok(false),
            Ok(0) => return Err(io::Error::new(ErrorKind::UnexpectedEof, "truncated raw-f32 block")),
            Ok(count) => read_total += count,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(true)
}

fn floats_to_bytes(samples: &[f32], output: &mut [u8]) {
    debug_assert_eq!(output.len(), samples.len() * 4);
    for (sample, bytes) in samples.iter().zip(output.chunks_exact_mut(4)) {
        bytes.copy_from_slice(&sample.to_le_bytes());
    }
}

fn run() -> Result<()> {
    let control_fd = env_usize("AURORA_CONTROL_FD", 3)? as RawFd;
    let drift_target_frames = env_usize("AURORA_DRIFT_TARGET_FRAMES", 120)?;
    if drift_target_frames < BLOCK_FRAMES || drift_target_frames > 4_096 {
        bail!("AURORA_DRIFT_TARGET_FRAMES must be between 40 and 4096");
    }

    let state = Arc::new(ControlState::default());
    state.lipsync_frames.store(
        (env_f32("AURORA_LIPSYNC_MS", 0.0)?.clamp(0.0, 500.0)
            * SAMPLE_RATE as f32
            / 1_000.0)
            .round() as usize,
        Ordering::Relaxed,
    );
    spawn_control_reader(control_fd, Arc::clone(&state));

    let drift_config = DriftControllerConfig {
        target_fill_frames: drift_target_frames,
        ..DriftControllerConfig::default()
    };
    let mut drift = DriftController::new(drift_config)?;
    let mut asrc = RubatoAsrc::default();
    asrc.configure(SAMPLE_RATE, SAMPLE_RATE, CHANNELS, BLOCK_FRAMES)?;
    if asrc.required_input_frames() > MAX_ASRC_INPUT_FRAMES {
        bail!("ASRC input requirement exceeds fixed Aurora scratch capacity");
    }

    let mut post = SpeakerPostProcessor::from_environment()?;
    post.set_lipsync_frames(state.lipsync_frames.load(Ordering::Relaxed));
    let mut queue = FixedFrameQueue::new(INPUT_QUEUE_FRAMES);
    let mut asrc_input = vec![0.0_f32; MAX_ASRC_INPUT_FRAMES * CHANNELS];
    let mut output = vec![0.0_f32; BLOCK_SAMPLES];
    let mut output_bytes = vec![0_u8; BLOCK_SAMPLES * 4];
    let mut read_bytes = [0_u8; READ_BYTES];
    let mut read_samples = [0.0_f32; READ_SAMPLES];

    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    let mut reset_epoch = state.reset_epoch.load(Ordering::Acquire);
    let mut clock_epoch = state.clock_epoch.load(Ordering::Acquire);
    let mut lipsync_frames = state.lipsync_frames.load(Ordering::Acquire);
    let mut master_gain_mdb = state.master_gain_mdb.load(Ordering::Acquire);
    let mut muted = state.muted.load(Ordering::Acquire);
    let mut standby = state.standby.load(Ordering::Acquire);
    let mut elapsed_output_frames = BLOCK_FRAMES;
    let mut last_queued_frames = drift_target_frames;

    loop {
        if !read_exact_or_clean_eof(&mut stdin, &mut read_bytes)? {
            break;
        }
        for (bytes, sample) in read_bytes.chunks_exact(4).zip(read_samples.iter_mut()) {
            *sample = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            if !sample.is_finite() {
                *sample = 0.0;
            }
        }
        queue.push_interleaved(&read_samples)?;

        let observed_reset_epoch = state.reset_epoch.load(Ordering::Acquire);
        if observed_reset_epoch != reset_epoch {
            reset_epoch = observed_reset_epoch;
            queue.clear();
            asrc.reset();
            drift.reset();
            post.reset();
            elapsed_output_frames = BLOCK_FRAMES;
            last_queued_frames = drift_target_frames;
            continue;
        }

        let observed_lipsync = state.lipsync_frames.load(Ordering::Acquire);
        if observed_lipsync != lipsync_frames {
            lipsync_frames = observed_lipsync;
            post.set_lipsync_frames(lipsync_frames);
        }
        let observed_gain = state.master_gain_mdb.load(Ordering::Acquire);
        if observed_gain != master_gain_mdb {
            master_gain_mdb = observed_gain;
            post.set_master_gain_mdb(master_gain_mdb);
        }
        let observed_mute = state.muted.load(Ordering::Acquire);
        if observed_mute != muted {
            muted = observed_mute;
            post.set_mute(muted);
        }
        let observed_standby = state.standby.load(Ordering::Acquire);
        if observed_standby != standby {
            standby = observed_standby;
            post.set_standby(standby);
        }

        let observed_clock_epoch = state.clock_epoch.load(Ordering::Acquire);
        if observed_clock_epoch != clock_epoch {
            clock_epoch = observed_clock_epoch;
            let queued = state.queued_playback_frames.load(Ordering::Relaxed);
            let trend = queued as i64 - last_queued_frames as i64;
            last_queued_frames = queued;
            let report = drift.update(queued, trend, elapsed_output_frames.max(1))?;
            asrc.set_ratio(report.ratio)?;
            elapsed_output_frames = 0;
        }

        loop {
            let required = asrc.required_input_frames();
            if required == 0 || required > MAX_ASRC_INPUT_FRAMES {
                bail!("invalid ASRC input requirement {required}");
            }
            if queue.len_frames < required {
                break;
            }
            queue.pop_frames(&mut asrc_input[..required * CHANNELS], required)?;
            let report = asrc.process(
                &asrc_input[..required * CHANNELS],
                &mut output[..BLOCK_SAMPLES],
            )?;
            if report.output_frames != BLOCK_FRAMES {
                bail!("ASRC produced unexpected block size");
            }
            post.process_block(&mut output[..BLOCK_SAMPLES]);
            floats_to_bytes(&output[..BLOCK_SAMPLES], &mut output_bytes);
            stdout.write_all(&output_bytes)?;
            stdout.flush()?;
            elapsed_output_frames = elapsed_output_frames.saturating_add(BLOCK_FRAMES);
        }
    }
    stdout.flush()?;
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("aurora-s6-postprocess: {error:#}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn processor() -> SpeakerPostProcessor {
        SpeakerPostProcessor::new(80.0, 100.0, 120.0, 20.0, 0.0, 0.0, -3.0, -1.0, 50.0, 0)
            .unwrap()
    }

    #[test]
    fn fixed_queue_preserves_frame_order_across_wrap() {
        let mut queue = FixedFrameQueue::new(4);
        let first = (0..3 * CHANNELS).map(|v| v as f32).collect::<Vec<_>>();
        queue.push_interleaved(&first).unwrap();
        let mut two = vec![0.0; 2 * CHANNELS];
        queue.pop_frames(&mut two, 2).unwrap();
        assert_eq!(two, first[..2 * CHANNELS]);
        let second = (100..103)
            .flat_map(|frame| std::iter::repeat_n(frame as f32, CHANNELS))
            .collect::<Vec<_>>();
        queue.push_interleaved(&second[..2 * CHANNELS]).unwrap();
        let mut remaining = vec![0.0; 3 * CHANNELS];
        queue.pop_frames(&mut remaining, 3).unwrap();
        assert_eq!(&remaining[..CHANNELS], &first[2 * CHANNELS..3 * CHANNELS]);
        assert_eq!(&remaining[CHANNELS..2 * CHANNELS], &second[..CHANNELS]);
        assert_eq!(&remaining[2 * CHANNELS..], &second[CHANNELS..2 * CHANNELS]);
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
            post.process_block(&mut block);
            assert!(block.iter().all(|sample| sample.is_finite()));
            assert!(block.iter().all(|sample| sample.abs() <= db_to_linear(-1.0) + 1.0e-6));
        }
    }

    #[test]
    fn control_message_updates_only_requested_fields() {
        let state = ControlState::default();
        let mut message = [0_u8; CONTROL_MESSAGE_BYTES];
        message[0..4].copy_from_slice(&CONTROL_MAGIC);
        message[4..6].copy_from_slice(&CONTROL_VERSION.to_le_bytes());
        message[6..8].copy_from_slice(&CTRL_CLOCK_REPORT.to_le_bytes());
        message[8..16].copy_from_slice(&123_u64.to_le_bytes());
        message[16..24].copy_from_slice(&456_u64.to_le_bytes());
        message[24..28].copy_from_slice(&80_u32.to_le_bytes());
        message[28..32].copy_from_slice(&7_u32.to_le_bytes());
        apply_control_message(&state, &message);
        assert_eq!(state.sink_counter.load(Ordering::Relaxed), 123);
        assert_eq!(state.source_counter.load(Ordering::Relaxed), 456);
        assert_eq!(state.queued_playback_frames.load(Ordering::Relaxed), 80);
        assert_eq!(state.capture_flags.load(Ordering::Relaxed), 7);
        assert_eq!(state.clock_epoch.load(Ordering::Acquire), 1);
    }

    #[test]
    fn asrc_keeps_all_channels_coherent_at_clock_correction() {
        let mut asrc = RubatoAsrc::default();
        asrc.configure(SAMPLE_RATE, SAMPLE_RATE, CHANNELS, BLOCK_FRAMES).unwrap();
        asrc.set_ratio(0.999_9).unwrap();
        for _ in 0..8 {
            let required = asrc.required_input_frames();
            let mut input = vec![0.0_f32; required * CHANNELS];
            for frame in input.chunks_exact_mut(CHANNELS) {
                for (channel, sample) in frame.iter_mut().enumerate() {
                    *sample = channel as f32 * 0.01 + 0.1;
                }
            }
            let mut output = vec![0.0_f32; BLOCK_SAMPLES];
            asrc.process(&input, &mut output).unwrap();
            assert!(output.iter().all(|sample| sample.is_finite()));
        }
    }
}
