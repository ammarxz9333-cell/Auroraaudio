//! Low-latency post-render processing for the Galaxy S6 Aurora appliance.
//!
//! stdin:  raw interleaved f32, 48 kHz, 12-channel 7.1.4 from Omniphony
//! stdout: raw interleaved f32, 48 kHz, 12-channel 7.1.4 in 40-frame quanta
//! Both pipe boundaries use FL FR C LFE BL BR SL SR TFL TFR TBL TBR.
//! The shared DSP uses SL SR before BL BR; conversion stays at this boundary.
//!
//! An optional inherited control pipe (fd 3 by default) carries fixed-size
//! CLOCK_REPORT and local-control messages from `aurora-live-ingest`. Control is
//! consumed on a dedicated thread; the audio loop observes atomics only.

use anyhow::{bail, Context, Result};
use aurora_dsp_basic::output::{OutputDspConfig, OutputShapeError, SpeakerPostProcessor};
use aurora_realtime_engine::{
    AsynchronousResampler, DriftController, DriftControllerConfig, RubatoAsrc,
};
#[cfg(unix)]
use std::fs::File;
use std::io::{self, ErrorKind, Read, Write};
#[cfg(unix)]
use std::os::fd::{FromRawFd, RawFd};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: usize = 12;
const BLOCK_FRAMES: usize = 40;
const BLOCK_SAMPLES: usize = CHANNELS * BLOCK_FRAMES;
const READ_FRAMES: usize = 8;
const READ_SAMPLES: usize = READ_FRAMES * CHANNELS;
const READ_BYTES: usize = READ_SAMPLES * 4;
const MAX_ASRC_INPUT_FRAMES: usize = 128;
const INPUT_QUEUE_FRAMES: usize = 512;
const MAX_LIPSYNC_FRAMES: usize = 24_000; // 500 ms at 48 kHz.
const CONTROL_MESSAGE_BYTES: usize = 32;
const CONTROL_MAGIC: [u8; 4] = *b"APC0";
const CONTROL_VERSION: u16 = 1;

const CTRL_RESET: u16 = 1;
const CTRL_CLOCK_REPORT: u16 = 2;
const CTRL_LIPSYNC_FRAMES: u16 = 3;
const CTRL_MASTER_GAIN_MDB: u16 = 4;
const CTRL_MUTE: u16 = 5;
const CTRL_STANDBY: u16 = 6;

fn env_f32(name: &str, default: f32) -> Result<f32> {
    match std::env::var(name) {
        Ok(value) => {
            let parsed = value
                .parse::<f32>()
                .with_context(|| format!("invalid {name}={value}"))?;
            if !parsed.is_finite() {
                bail!("{name} must be finite");
            }
            Ok(parsed)
        }
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

fn postprocessor_from_environment() -> Result<SpeakerPostProcessor> {
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
    let lipsync_frames = (lipsync_ms * SAMPLE_RATE as f32 / 1_000.0).round() as usize;
    let mut processor = SpeakerPostProcessor::new(OutputDspConfig {
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
    })?;
    if let Some(path) = std::env::var_os("AURORA_SPEAKER_CALIBRATION") {
        let bytes = std::fs::read(&path).context("read speaker calibration")?;
        let config = serde_json::from_slice(&bytes).context("parse speaker calibration")?;
        processor.configure_calibration(&config)?;
    }
    Ok(processor)
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
    match kind {
        CTRL_RESET => {
            state.reset_epoch.fetch_add(1, Ordering::Release);
        }
        CTRL_CLOCK_REPORT => {
            state.sink_counter.store(data0, Ordering::Relaxed);
            state.source_counter.store(data1, Ordering::Relaxed);
            state
                .queued_playback_frames
                .store(data2 as usize, Ordering::Relaxed);
            state.clock_epoch.fetch_add(1, Ordering::Release);
        }
        CTRL_LIPSYNC_FRAMES => {
            state
                .lipsync_frames
                .store((data0 as usize).min(MAX_LIPSYNC_FRAMES), Ordering::Release);
        }
        CTRL_MASTER_GAIN_MDB => {
            state.master_gain_mdb.store(data0 as i64, Ordering::Release);
        }
        CTRL_MUTE => state.muted.store(data0 != 0, Ordering::Release),
        CTRL_STANDBY => state.standby.store(data0 != 0, Ordering::Release),
        _ => {}
    }
}

#[cfg(unix)]
fn spawn_source_control(state: Arc<ControlState>) -> Result<()> {
    use std::os::unix::{
        fs::{FileTypeExt, PermissionsExt},
        net::UnixDatagram,
    };
    let Some(path) = std::env::var_os("AURORA_DSP_CONTROL_SOCKET") else {
        return Ok(());
    };
    let bind = || UnixDatagram::bind(&path);
    let socket = match bind() {
        Ok(socket) => socket,
        Err(error) if error.kind() == ErrorKind::AddrInUse => {
            // Remove only a socket with a refused connection, never a live
            // process endpoint or an arbitrary file at the configured path.
            if !std::fs::symlink_metadata(&path)?.file_type().is_socket() {
                return Err(error.into());
            }
            let probe = UnixDatagram::unbound()?;
            let result = probe
                .connect(&path)
                .and_then(|_| probe.send(&[]).map(|_| ()));
            if !matches!(result, Err(ref e) if e.kind() == ErrorKind::ConnectionRefused) {
                return Err(error.into());
            }
            std::fs::remove_file(&path)?;
            bind()?
        }
        Err(error) => return Err(error.into()),
    };
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    std::thread::Builder::new()
        .name("aurora-source-control".into())
        .spawn(move || {
            let mut bytes = [0_u8; CONTROL_MESSAGE_BYTES + 1];
            loop {
                match socket.recv(&mut bytes) {
                    Ok(CONTROL_MESSAGE_BYTES) => {
                        if read_le16(&bytes[6..8]) == CTRL_LIPSYNC_FRAMES {
                            let message: &[u8; CONTROL_MESSAGE_BYTES] =
                                (&bytes[..CONTROL_MESSAGE_BYTES]).try_into().unwrap();
                            apply_control_message(&state, message);
                        }
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        })?;
    Ok(())
}

#[cfg(not(unix))]
fn spawn_source_control(_state: Arc<ControlState>) -> Result<()> {
    if std::env::var_os("AURORA_DSP_CONTROL_SOCKET").is_some() {
        bail!("source DSP socket requires Unix");
    }
    Ok(())
}

#[cfg(unix)]
fn spawn_control_reader(fd: usize, state: Arc<ControlState>) -> Result<()> {
    if fd < 3 || fd > i32::MAX as usize {
        bail!("control descriptor must be an inherited non-stdio descriptor");
    }
    // SAFETY: F_GETFD only queries the supplied integer descriptor.
    if unsafe { libc::fcntl(fd as RawFd, libc::F_GETFD) } < 0 {
        return Err(io::Error::last_os_error()).context("invalid inherited control descriptor");
    }
    std::thread::Builder::new()
        .name("aurora-post-control".to_owned())
        .spawn(move || {
            // SAFETY: the descriptor is intentionally transferred to this
            // thread by the execing parent and is owned here until thread exit.
            let mut file = unsafe { File::from_raw_fd(fd as RawFd) };
            let mut message = [0_u8; CONTROL_MESSAGE_BYTES];
            loop {
                match file.read_exact(&mut message) {
                    Ok(()) => apply_control_message(&state, &message),
                    Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        })
        .context("failed to spawn Aurora postprocessor control thread")?;
    Ok(())
}

#[cfg(not(unix))]
fn spawn_control_reader(_fd: usize, _state: Arc<ControlState>) -> Result<()> {
    let _parse: fn(&ControlState, &[u8; CONTROL_MESSAGE_BYTES]) = apply_control_message;
    bail!("aurora-s6-postprocess requires Unix file-descriptor passing");
}

fn read_exact_or_clean_eof<R: Read>(reader: &mut R, buffer: &mut [u8]) -> io::Result<bool> {
    let mut read_total = 0;
    while read_total < buffer.len() {
        match reader.read(&mut buffer[read_total..]) {
            Ok(0) if read_total == 0 => return Ok(false),
            Ok(0) => {
                return Err(io::Error::new(
                    ErrorKind::UnexpectedEof,
                    "truncated raw-f32 block",
                ))
            }
            Ok(count) => read_total += count,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(true)
}

// The permutation is its own inverse. Input shape is checked by the wrapper.
fn swap_surround_order(samples: &mut [f32]) {
    for frame in samples.chunks_exact_mut(CHANNELS) {
        frame.swap(4, 6);
        frame.swap(5, 7);
    }
}

fn process_wire_block(
    post: &mut SpeakerPostProcessor,
    samples: &mut [f32],
) -> std::result::Result<(), OutputShapeError> {
    if samples.len() % CHANNELS != 0 {
        samples.fill(0.0);
        return Err(OutputShapeError);
    }
    swap_surround_order(samples);
    let result = post.process_block(samples);
    swap_surround_order(samples);
    result
}

fn floats_to_bytes(samples: &[f32], output: &mut [u8]) {
    debug_assert_eq!(output.len(), samples.len() * 4);
    for (sample, bytes) in samples.iter().zip(output.chunks_exact_mut(4)) {
        bytes.copy_from_slice(&sample.to_le_bytes());
    }
}

fn run() -> Result<()> {
    let drift_target_frames = env_usize("AURORA_DRIFT_TARGET_FRAMES", 120)?;
    if !(BLOCK_FRAMES..=4_096).contains(&drift_target_frames) {
        bail!("AURORA_DRIFT_TARGET_FRAMES must be between 40 and 4096");
    }

    let state = Arc::new(ControlState::default());
    state.lipsync_frames.store(
        (env_f32("AURORA_LIPSYNC_MS", 0.0)?.clamp(0.0, 500.0) * SAMPLE_RATE as f32 / 1_000.0)
            .round() as usize,
        Ordering::Relaxed,
    );
    if std::env::var_os("AURORA_CONTROL_FD").is_some() {
        spawn_control_reader(env_usize("AURORA_CONTROL_FD", 3)?, Arc::clone(&state))?;
    }
    spawn_source_control(Arc::clone(&state))?;

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

    let mut post = postprocessor_from_environment()?;
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
    let mut last_sink_counter: Option<u64> = None;
    let mut last_source_counter: Option<u64> = None;

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
            last_sink_counter = None;
            last_source_counter = None;
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
            let sink = state.sink_counter.load(Ordering::Relaxed);
            let source = state.source_counter.load(Ordering::Relaxed);
            let counters_regressed = matches!(last_sink_counter, Some(last) if sink < last)
                || matches!(last_source_counter, Some(last) if source < last);
            last_sink_counter = Some(sink);
            last_source_counter = Some(source);
            if counters_regressed {
                queue.clear();
                asrc.reset();
                drift.reset();
                post.reset();
                elapsed_output_frames = BLOCK_FRAMES;
                continue;
            }

            let queued = state.queued_playback_frames.load(Ordering::Relaxed);
            let report = drift.update(queued, 0, elapsed_output_frames.max(1))?;
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
            process_wire_block(&mut post, &mut output[..BLOCK_SAMPLES])?;
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
    use aurora_dsp_basic::output::{ChannelCalibration, SpeakerCalibration};

    #[global_allocator]
    static ALLOCATOR: aurora_test_alloc::CountingAllocator = aurora_test_alloc::CountingAllocator;

    #[test]
    fn wire_channels_receive_their_own_role_calibration() {
        let calibration = SpeakerCalibration {
            schema_version: 1,
            sample_rate: SAMPLE_RATE,
            channels: aurora_core::StandardLayout::SevenOneFour
                .canonical_roles()
                .iter()
                .enumerate()
                .map(|(index, role)| ChannelCalibration {
                    role: role.clone(),
                    trim_db: index as f32 - 6.0,
                    delay_frames: index,
                    invert_polarity: index % 2 == 0,
                    peq: Vec::new(),
                })
                .collect(),
        };
        let mut actual_post = SpeakerPostProcessor::new(OutputDspConfig::default()).unwrap();
        let mut reference_post = SpeakerPostProcessor::new(OutputDspConfig::default()).unwrap();
        actual_post.configure_calibration(&calibration).unwrap();
        reference_post.configure_calibration(&calibration).unwrap();
        // Canonical index for each external wire channel, derived from role names.
        let canonical_index = [0, 1, 2, 3, 6, 7, 4, 5, 8, 9, 10, 11];
        let mut wire = [0.0_f32; BLOCK_SAMPLES];
        let mut canonical = [0.0_f32; BLOCK_SAMPLES];
        for (channel, &index) in canonical_index.iter().enumerate() {
            wire[channel] = (channel + 1) as f32 * 0.001;
            canonical[index] = wire[channel];
        }
        reference_post.process_block(&mut canonical).unwrap();
        let allocations = aurora_test_alloc::count_allocations(|| {
            process_wire_block(&mut actual_post, &mut wire).unwrap();
        });
        assert_eq!(allocations, 0);
        for (actual_frame, expected_frame) in wire
            .chunks_exact(CHANNELS)
            .zip(canonical.chunks_exact(CHANNELS))
        {
            for (channel, &index) in canonical_index.iter().enumerate() {
                assert_eq!(actual_frame[channel], expected_frame[index]);
            }
        }
    }

    #[test]
    fn incomplete_wire_frame_is_silenced() {
        let mut post = SpeakerPostProcessor::new(OutputDspConfig::default()).unwrap();
        let mut block = [0.5_f32; CHANNELS + 1];
        assert_eq!(
            process_wire_block(&mut post, &mut block),
            Err(OutputShapeError)
        );
        assert!(block.iter().all(|sample| *sample == 0.0));
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
            .flat_map(|frame| std::iter::repeat(frame as f32).take(CHANNELS))
            .collect::<Vec<_>>();
        queue.push_interleaved(&second[..2 * CHANNELS]).unwrap();
        let mut remaining = vec![0.0; 3 * CHANNELS];
        queue.pop_frames(&mut remaining, 3).unwrap();
        assert_eq!(&remaining[..CHANNELS], &first[2 * CHANNELS..3 * CHANNELS]);
        assert_eq!(&remaining[CHANNELS..2 * CHANNELS], &second[..CHANNELS]);
        assert_eq!(&remaining[2 * CHANNELS..], &second[CHANNELS..2 * CHANNELS]);
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
        assert_eq!(state.clock_epoch.load(Ordering::Acquire), 1);
    }

    #[test]
    fn asrc_keeps_all_channels_coherent_at_clock_correction() {
        let mut asrc = RubatoAsrc::default();
        asrc.configure(SAMPLE_RATE, SAMPLE_RATE, CHANNELS, BLOCK_FRAMES)
            .unwrap();
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
