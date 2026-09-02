#[path = "../alsa_out.rs"]
mod alsa_out;
#[path = "../alsa_pcm.rs"]
mod alsa_pcm;

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("aurora-alsa-out is only supported on Linux/ALSA targets");
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
mod linux {
    use std::env;
    use std::error::Error;
    use std::fs;
    use std::io::{self, Read};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
    use std::sync::{Arc, Condvar, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    use super::alsa_out::{
        db_to_linear, pack_frame_s32, AdaptiveClockController, BandlimitedResampler12, Frame12,
        OUTPUT_SAMPLE_RATE, RENDER_CHANNELS, RESAMPLER_LOOKAHEAD_FRAMES, RESAMPLER_PRIME_FRAMES,
        TDM_CHANNELS,
    };
    use super::alsa_pcm::{AlsaPlayback, AlsaPlaybackConfig};

    const SOURCE_BLOCK_FRAMES: usize = 256;
    const LATENCY_PROBE_INTERVAL: Duration = Duration::from_millis(250);
    const LATENCY_REPORT_INTERVAL: Duration = Duration::from_millis(250);
    const LATENCY_UNSET_BITS: u64 = u64::MAX;

    #[derive(Debug, Clone)]
    struct Config {
        device: String,
        period_frames: usize,
        buffer_periods: usize,
        queue_ms: usize,
        target_ms: usize,
        max_ppm: f64,
        gain_db: f32,
        source_timeout_ms: u64,
        startup_timeout_ms: u64,
        latency_file: Option<PathBuf>,
        stats: bool,
    }

    impl Default for Config {
        fn default() -> Self {
            Self {
                device: "hw:AuroraTDM16,0".to_owned(),
                period_frames: 256,
                buffer_periods: 4,
                queue_ms: 200,
                target_ms: 80,
                max_ppm: 300.0,
                gain_db: -3.0,
                source_timeout_ms: 500,
                startup_timeout_ms: 5_000,
                latency_file: Some(env::temp_dir().join("omniphony_delay")),
                stats: false,
            }
        }
    }

    impl Config {
        fn parse() -> Result<Self, String> {
            let mut cfg = Self::default();
            let mut args = env::args().skip(1);
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--device" => cfg.device = next_value(&mut args, "--device")?,
                    "--period" => cfg.period_frames = parse_value(&mut args, "--period")?,
                    "--buffer-periods" => {
                        cfg.buffer_periods = parse_value(&mut args, "--buffer-periods")?;
                    }
                    "--queue-ms" => cfg.queue_ms = parse_value(&mut args, "--queue-ms")?,
                    "--target-ms" => cfg.target_ms = parse_value(&mut args, "--target-ms")?,
                    "--max-ppm" => cfg.max_ppm = parse_value(&mut args, "--max-ppm")?,
                    "--gain-db" => cfg.gain_db = parse_value(&mut args, "--gain-db")?,
                    "--source-timeout-ms" => {
                        cfg.source_timeout_ms = parse_value(&mut args, "--source-timeout-ms")?;
                    }
                    "--startup-timeout-ms" => {
                        cfg.startup_timeout_ms = parse_value(&mut args, "--startup-timeout-ms")?;
                    }
                    "--latency-file" => {
                        cfg.latency_file =
                            Some(PathBuf::from(next_value(&mut args, "--latency-file")?));
                    }
                    "--no-latency-file" => cfg.latency_file = None,
                    "--stats" => cfg.stats = true,
                    "-h" | "--help" => {
                        print_help();
                        std::process::exit(0);
                    }
                    other => return Err(format!("unknown argument: {other}")),
                }
            }
            cfg.validate()?;
            Ok(cfg)
        }

        fn validate(&self) -> Result<(), String> {
            if self.period_frames == 0 {
                return Err("--period must be greater than zero".to_owned());
            }
            if self.buffer_periods < 2 {
                return Err("--buffer-periods must be at least 2".to_owned());
            }
            if self.queue_ms < 20 {
                return Err("--queue-ms must be at least 20".to_owned());
            }
            if self.target_ms == 0 || self.target_ms >= self.queue_ms {
                return Err(
                    "--target-ms must be greater than 0 and smaller than --queue-ms".to_owned(),
                );
            }
            if !(1.0..=1_000.0).contains(&self.max_ppm) {
                return Err("--max-ppm must be in 1..=1000".to_owned());
            }
            if self.gain_db > 0.0 || self.gain_db < -60.0 {
                return Err("--gain-db must be in -60..=0 dB".to_owned());
            }
            Ok(())
        }

        fn queue_frames(&self) -> usize {
            milliseconds_to_frames(self.queue_ms)
        }

        fn target_frames(&self) -> usize {
            milliseconds_to_frames(self.target_ms)
        }
    }

    fn print_help() {
        eprintln!(
            "Aurora R1 direct ALSA output\n\
             Reads 12ch interleaved f32le/48k PCM from stdin and writes canonical\n\
             16ch S32_LE/48k PCM directly to the Aurora ALSA hw device.\n\n\
             Options:\n\
               --device NAME              ALSA PCM (default hw:AuroraTDM16,0)\n\
               --period FRAMES            ALSA period (default 256)\n\
               --buffer-periods N         ALSA buffer periods (default 4)\n\
               --queue-ms MS              bounded source queue (default 200)\n\
               --target-ms MS             clock-control queue target (default 80)\n\
               --max-ppm PPM              max clock correction (default 300)\n\
               --gain-db DB               output headroom (default -3.0)\n\
               --source-timeout-ms MS     fail-closed source timeout (default 500)\n\
               --startup-timeout-ms MS    source priming timeout (default 5000)\n\
               --latency-file PATH        publish negative A/V delay seconds\n\
                                         (default /tmp/omniphony_delay)\n\
               --no-latency-file          disable A/V delay publication\n\
               --stats                    print 1 Hz queue/ppm/recovery telemetry"
        );
    }

    fn next_value<I>(args: &mut I, name: &str) -> Result<String, String>
    where
        I: Iterator<Item = String>,
    {
        args.next()
            .ok_or_else(|| format!("missing value for {name}"))
    }

    fn parse_value<I, T>(args: &mut I, name: &str) -> Result<T, String>
    where
        I: Iterator<Item = String>,
        T: std::str::FromStr,
        T::Err: std::fmt::Display,
    {
        let raw = next_value(args, name)?;
        raw.parse::<T>()
            .map_err(|error| format!("invalid {name} value '{raw}': {error}"))
    }

    fn milliseconds_to_frames(ms: usize) -> usize {
        (ms * OUTPUT_SAMPLE_RATE as usize) / 1_000
    }

    struct FrameQueue {
        state: Mutex<QueueState>,
        not_empty: Condvar,
        not_full: Condvar,
        buffered_frames: AtomicUsize,
        closed: AtomicBool,
    }

    struct QueueState {
        frames: Vec<Frame12>,
        read_index: usize,
        write_index: usize,
        len: usize,
        error: Option<String>,
        closed: bool,
    }

    impl FrameQueue {
        fn new(capacity_frames: usize) -> Self {
            let capacity_frames = capacity_frames.max(SOURCE_BLOCK_FRAMES * 2);
            Self {
                state: Mutex::new(QueueState {
                    frames: vec![[0.0; RENDER_CHANNELS]; capacity_frames],
                    read_index: 0,
                    write_index: 0,
                    len: 0,
                    error: None,
                    closed: false,
                }),
                not_empty: Condvar::new(),
                not_full: Condvar::new(),
                buffered_frames: AtomicUsize::new(0),
                closed: AtomicBool::new(false),
            }
        }

        fn push_frames(&self, frames: &[Frame12]) -> Result<(), String> {
            let mut source_offset = 0usize;
            while source_offset < frames.len() {
                let mut state = self
                    .state
                    .lock()
                    .map_err(|_| "PCM queue mutex poisoned".to_owned())?;
                while state.len == state.frames.len() && !state.closed {
                    state = self
                        .not_full
                        .wait(state)
                        .map_err(|_| "PCM queue mutex poisoned while waiting".to_owned())?;
                }
                if state.closed {
                    return Err("PCM queue closed".to_owned());
                }

                let capacity = state.frames.len();
                let writable = (capacity - state.len).min(frames.len() - source_offset);
                let first = writable.min(capacity - state.write_index);
                let write_index = state.write_index;
                state.frames[write_index..write_index + first]
                    .copy_from_slice(&frames[source_offset..source_offset + first]);
                state.write_index = (write_index + first) % capacity;
                state.len += first;
                source_offset += first;

                let second = writable - first;
                if second > 0 {
                    let write_index = state.write_index;
                    state.frames[write_index..write_index + second]
                        .copy_from_slice(&frames[source_offset..source_offset + second]);
                    state.write_index = (write_index + second) % capacity;
                    state.len += second;
                    source_offset += second;
                }

                self.buffered_frames.fetch_add(writable, Ordering::Release);
                drop(state);
                self.not_empty.notify_one();
            }
            Ok(())
        }

        fn pop_block(&self, destination: &mut [Frame12], timeout: Duration) -> io::Result<usize> {
            let state = self
                .state
                .lock()
                .map_err(|_| io::Error::other("PCM queue mutex poisoned"))?;
            let (mut state, wait_result) = self
                .not_empty
                .wait_timeout_while(state, timeout, |state| state.len == 0 && !state.closed)
                .map_err(|_| io::Error::other("PCM queue mutex poisoned while waiting"))?;

            if state.len == 0 {
                if state.closed {
                    if let Some(message) = state.error.as_deref() {
                        return Err(io::Error::new(io::ErrorKind::InvalidData, message));
                    }
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "Omniphony PCM stream ended",
                    ));
                }
                if wait_result.timed_out() {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "Omniphony PCM source timed out",
                    ));
                }
            }

            let capacity = state.frames.len();
            let readable = state.len.min(destination.len());
            let first = readable.min(capacity - state.read_index);
            destination[..first]
                .copy_from_slice(&state.frames[state.read_index..state.read_index + first]);
            state.read_index = (state.read_index + first) % capacity;
            state.len -= first;

            let second = readable - first;
            if second > 0 {
                destination[first..first + second]
                    .copy_from_slice(&state.frames[state.read_index..state.read_index + second]);
                state.read_index = (state.read_index + second) % capacity;
                state.len -= second;
            }

            drop(state);
            self.not_full.notify_one();
            Ok(readable)
        }

        fn mark_frame_consumed(&self) {
            let previous = self.buffered_frames.fetch_sub(1, Ordering::AcqRel);
            debug_assert!(previous > 0, "PCM queue accounting underflow");
        }

        fn buffered_frames(&self) -> usize {
            self.buffered_frames.load(Ordering::Acquire)
        }

        fn is_closed(&self) -> bool {
            self.closed.load(Ordering::Acquire)
        }

        fn close(&self, error: Option<String>) {
            if let Ok(mut state) = self.state.lock() {
                if !state.closed {
                    state.error = error;
                    state.closed = true;
                }
            }
            self.closed.store(true, Ordering::Release);
            self.not_empty.notify_all();
            self.not_full.notify_all();
        }
    }

    struct SourceReader {
        queue: Arc<FrameQueue>,
        current: [Frame12; SOURCE_BLOCK_FRAMES],
        len: usize,
        index: usize,
        timeout: Duration,
    }

    impl SourceReader {
        fn next_frame(&mut self) -> Result<Frame12, io::Error> {
            if self.index >= self.len {
                self.len = self.queue.pop_block(&mut self.current, self.timeout)?;
                self.index = 0;
            }
            let frame = self.current[self.index];
            self.index += 1;
            self.queue.mark_frame_consumed();
            Ok(frame)
        }
    }

    fn spawn_source_reader(queue: Arc<FrameQueue>) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let result = read_stdin_blocks(&queue);
            match result {
                Ok(()) => queue.close(None),
                Err(message) => queue.close(Some(message)),
            }
        })
    }

    fn read_stdin_blocks(queue: &FrameQueue) -> Result<(), String> {
        let stdin = io::stdin();
        let mut input = stdin.lock();
        let frame_bytes = RENDER_CHANNELS * std::mem::size_of::<f32>();
        let mut bytes = vec![0_u8; SOURCE_BLOCK_FRAMES * frame_bytes];
        let mut frames = [[0.0_f32; RENDER_CHANNELS]; SOURCE_BLOCK_FRAMES];

        loop {
            let mut filled = 0usize;
            while filled < bytes.len() {
                match input.read(&mut bytes[filled..]) {
                    Ok(0) => break,
                    Ok(count) => filled += count,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(format!("stdin read failed: {error}")),
                }
            }
            if filled == 0 {
                return Ok(());
            }
            if filled % frame_bytes != 0 {
                return Err(format!(
                    "raw f32 stream ended mid-frame: {filled} bytes is not divisible by {frame_bytes}"
                ));
            }

            let frame_count = filled / frame_bytes;
            for (frame_index, raw_frame) in bytes[..filled].chunks_exact(frame_bytes).enumerate() {
                for (channel, raw_sample) in raw_frame.chunks_exact(4).enumerate() {
                    frames[frame_index][channel] = f32::from_le_bytes([
                        raw_sample[0],
                        raw_sample[1],
                        raw_sample[2],
                        raw_sample[3],
                    ]);
                }
            }
            queue.push_frames(&frames[..frame_count])?;

            if filled < bytes.len() {
                return Ok(());
            }
        }
    }

    fn wait_for_prime(
        queue: &FrameQueue,
        target_frames: usize,
        timeout: Duration,
    ) -> Result<(), String> {
        let started = Instant::now();
        loop {
            let available = queue.buffered_frames();
            if available >= target_frames {
                return Ok(());
            }
            if queue.is_closed() {
                if available >= RESAMPLER_PRIME_FRAMES {
                    return Ok(());
                }
                return Err(format!(
                    "source ended before enough PCM was buffered ({available} frames)"
                ));
            }
            if started.elapsed() >= timeout {
                return Err(format!(
                    "timed out waiting for source prime: {available}/{target_frames} frames"
                ));
            }
            thread::sleep(Duration::from_millis(2));
        }
    }

    struct LatencyFileReporter {
        latest_total_ms_bits: Arc<AtomicU64>,
        stop: Arc<AtomicBool>,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl LatencyFileReporter {
        fn spawn(path: PathBuf) -> io::Result<Self> {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }

            let latest_total_ms_bits = Arc::new(AtomicU64::new(LATENCY_UNSET_BITS));
            let stop = Arc::new(AtomicBool::new(false));
            let worker_latest = Arc::clone(&latest_total_ms_bits);
            let worker_stop = Arc::clone(&stop);
            let handle = thread::Builder::new()
                .name("aurora-av-delay".to_owned())
                .spawn(move || {
                    let mut last_written_bits = LATENCY_UNSET_BITS;
                    let mut last_error_log = Instant::now() - Duration::from_secs(5);
                    while !worker_stop.load(Ordering::Acquire) {
                        let bits = worker_latest.load(Ordering::Acquire);
                        if bits != LATENCY_UNSET_BITS && bits != last_written_bits {
                            let total_ms = f64::from_bits(bits);
                            let delay_seconds = -(total_ms / 1_000.0);
                            match fs::write(&path, format!("{delay_seconds:.6}\n")) {
                                Ok(()) => last_written_bits = bits,
                                Err(error) => {
                                    if last_error_log.elapsed() >= Duration::from_secs(5) {
                                        eprintln!(
                                            "aurora-alsa-out: could not publish A/V delay to {}: {error}",
                                            path.display()
                                        );
                                        last_error_log = Instant::now();
                                    }
                                }
                            }
                        }
                        thread::sleep(LATENCY_REPORT_INTERVAL);
                    }
                    let _ = fs::remove_file(&path);
                })?;

            Ok(Self {
                latest_total_ms_bits,
                stop,
                handle: Some(handle),
            })
        }

        fn publish(&self, total_ms: f64) {
            self.latest_total_ms_bits
                .store(total_ms.to_bits(), Ordering::Release);
        }
    }

    impl Drop for LatencyFileReporter {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    struct LatencyProbe {
        reporter: Option<LatencyFileReporter>,
        last_probe: Instant,
        last_total_ms: Option<f64>,
    }

    impl LatencyProbe {
        fn new(path: Option<PathBuf>) -> io::Result<Self> {
            let reporter = path.map(LatencyFileReporter::spawn).transpose()?;
            Ok(Self {
                reporter,
                last_probe: Instant::now() - LATENCY_PROBE_INTERVAL,
                last_total_ms: None,
            })
        }

        fn maybe_sample(&mut self, queued_frames: usize, playback: &AlsaPlayback) -> Option<f64> {
            if self.last_probe.elapsed() < LATENCY_PROBE_INTERVAL {
                return self.last_total_ms;
            }
            self.last_probe = Instant::now();

            let alsa_frames = match playback.delay_frames() {
                Ok(frames) => frames,
                Err(error) => {
                    eprintln!("aurora-alsa-out: latency probe failed: {error}");
                    return self.last_total_ms;
                }
            };
            let total_frames = queued_frames
                .saturating_add(RESAMPLER_LOOKAHEAD_FRAMES)
                .saturating_add(alsa_frames);
            let total_ms = total_frames as f64 * 1_000.0 / f64::from(OUTPUT_SAMPLE_RATE);
            self.last_total_ms = Some(total_ms);
            if let Some(reporter) = self.reporter.as_ref() {
                reporter.publish(total_ms);
            }
            Some(total_ms)
        }
    }

    pub fn run() -> Result<(), Box<dyn Error>> {
        let config = Config::parse().map_err(|error| {
            io::Error::new(io::ErrorKind::InvalidInput, format!("{error}; use --help"))
        })?;
        let queue_frames = config.queue_frames();
        let target_frames = config.target_frames();
        let queue = Arc::new(FrameQueue::new(queue_frames));
        let _reader_handle = spawn_source_reader(Arc::clone(&queue));

        wait_for_prime(
            &queue,
            target_frames,
            Duration::from_millis(config.startup_timeout_ms),
        )?;

        let mut source = SourceReader {
            queue: Arc::clone(&queue),
            current: [[0.0; RENDER_CHANNELS]; SOURCE_BLOCK_FRAMES],
            len: 0,
            index: 0,
            timeout: Duration::from_millis(config.source_timeout_ms),
        };
        let mut next = || source.next_frame();
        let mut resampler = BandlimitedResampler12::new();
        resampler.prime(&mut next)?;

        let mut playback = AlsaPlayback::open(&AlsaPlaybackConfig {
            device: config.device.clone(),
            sample_rate: OUTPUT_SAMPLE_RATE,
            channels: TDM_CHANNELS,
            period_frames: config.period_frames,
            buffer_periods: config.buffer_periods,
        })
        .map_err(io::Error::other)?;
        let period_frames = playback.period_frames();
        let interval_seconds = period_frames as f64 / f64::from(OUTPUT_SAMPLE_RATE);
        let mut controller = AdaptiveClockController::new(target_frames, config.max_ppm);
        let gain = db_to_linear(config.gain_db);
        let mut output = vec![0_i32; period_frames * TDM_CHANNELS];
        let mut latency = LatencyProbe::new(config.latency_file.clone())?;
        let mut last_total_latency_ms = None;
        let mut last_stats = Instant::now();
        let mut periods_written = 0_u64;

        if playback.period_frames() != config.period_frames {
            eprintln!(
                "aurora-alsa-out: ALSA adjusted period {} -> {} frames",
                config.period_frames,
                playback.period_frames()
            );
        }
        eprintln!(
            "aurora-alsa-out: direct ALSA active device={} format=S32_LE rate={} channels={} period={} buffer={} queue_target={}f gain={:.2}dB max_ppm={:.1}",
            config.device,
            OUTPUT_SAMPLE_RATE,
            TDM_CHANNELS,
            playback.period_frames(),
            playback.buffer_frames(),
            controller.target_frames(),
            config.gain_db,
            config.max_ppm
        );

        loop {
            let queued = queue.buffered_frames();
            let ppm = controller.update(queued, interval_seconds);
            let source_step = AdaptiveClockController::step_from_ppm(ppm);

            for frame_index in 0..period_frames {
                let frame = resampler.render(source_step, &mut next)?;
                let packed = pack_frame_s32(&frame, gain);
                let start = frame_index * TDM_CHANNELS;
                output[start..start + TDM_CHANNELS].copy_from_slice(&packed);
            }

            let recovered = playback
                .write_interleaved(&output)
                .map_err(io::Error::other)?;
            if recovered {
                controller.reset_after_discontinuity();
            }
            periods_written = periods_written.saturating_add(1);

            let queued_now = queue.buffered_frames();
            if let Some(total_ms) = latency.maybe_sample(queued_now, &playback) {
                last_total_latency_ms = Some(total_ms);
            }

            if config.stats && last_stats.elapsed() >= Duration::from_secs(1) {
                eprintln!(
                    "aurora-alsa-out: queued={}f ({:.1}ms) total_latency={:.1}ms correction={:+.2}ppm recoveries={} periods={}",
                    queued_now,
                    queued_now as f64 * 1_000.0 / f64::from(OUTPUT_SAMPLE_RATE),
                    last_total_latency_ms.unwrap_or_default(),
                    ppm,
                    playback.recoveries(),
                    periods_written
                );
                last_stats = Instant::now();
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn main() {
    if let Err(error) = linux::run() {
        eprintln!("aurora-alsa-out: fatal: {error}");
        std::process::exit(1);
    }
}
