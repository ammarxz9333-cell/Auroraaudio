#[path = "../alsa_capture.rs"]
mod alsa_capture;
#[path = "../alsa_out.rs"]
mod alsa_out;
#[path = "../alsa_pcm.rs"]
mod alsa_pcm;
#[path = "../iec61937.rs"]
mod iec61937;
#[path = "../orender_host.rs"]
mod orender_host;

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("aurora-appliance is only supported on Linux/ALSA targets");
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
mod linux {
    use std::env;
    use std::error::Error;
    use std::fs;
    use std::io;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
    use std::sync::{Arc, Condvar, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    use super::alsa_capture::{AlsaCapture, AlsaCaptureConfig};
    use super::alsa_out::{
        db_to_linear, pack_frame_s32, AdaptiveClockController, BandlimitedResampler12, Frame12,
        OUTPUT_SAMPLE_RATE, RENDER_CHANNELS, RESAMPLER_LOOKAHEAD_FRAMES, RESAMPLER_PRIME_FRAMES,
        TDM_CHANNELS,
    };
    use super::alsa_pcm::{AlsaPlayback, AlsaPlaybackConfig};
    use super::iec61937::{s32_samples_to_iec_words, BurstParser, CodecFilter};
    use super::orender_host::{OrenderHost, OrenderHostConfig};

    const SOURCE_BLOCK_FRAMES: usize = 256;
    const CAPTURE_RATE: u32 = 192_000;
    const CAPTURE_CHANNELS: usize = 2;
    const DEFAULT_CAPTURE_PERIOD: usize = 1024;
    const DEFAULT_OUTPUT_PERIOD: usize = 256;
    const LATENCY_UNSET: u64 = u64::MAX;

    #[derive(Debug, Clone)]
    struct Config {
        capture_device: String,
        playback_device: String,
        liborender: String,
        bridge: String,
        layout: String,
        omniphony_config: Option<String>,
        capture_period: usize,
        output_period: usize,
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
                capture_device: "hw:AuroraEARC,0".to_owned(),
                playback_device: "hw:AuroraTDM16,0".to_owned(),
                liborender: "/opt/aurora-deps/Omniphony/omniphony-renderer/target/release/liborender.so".to_owned(),
                bridge: "/opt/aurora-deps/harletty-bridge/target/release/libharletty_bridge.so".to_owned(),
                layout: "/opt/aurora/platforms/imx93/layouts/aurora-7.1.4.yaml".to_owned(),
                omniphony_config: None,
                capture_period: DEFAULT_CAPTURE_PERIOD,
                output_period: DEFAULT_OUTPUT_PERIOD,
                buffer_periods: 4,
                queue_ms: 200,
                target_ms: 80,
                max_ppm: 300.0,
                gain_db: -3.0,
                source_timeout_ms: 500,
                startup_timeout_ms: 5_000,
                latency_file: Some(env::temp_dir().join("aurora_delay")),
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
                    "--capture-device" => cfg.capture_device = next(&mut args, &arg)?,
                    "--playback-device" => cfg.playback_device = next(&mut args, &arg)?,
                    "--liborender" => cfg.liborender = next(&mut args, &arg)?,
                    "--bridge" => cfg.bridge = next(&mut args, &arg)?,
                    "--layout" => cfg.layout = next(&mut args, &arg)?,
                    "--omniphony-config" => cfg.omniphony_config = Some(next(&mut args, &arg)?),
                    "--capture-period" => cfg.capture_period = parse(&mut args, &arg)?,
                    "--output-period" => cfg.output_period = parse(&mut args, &arg)?,
                    "--buffer-periods" => cfg.buffer_periods = parse(&mut args, &arg)?,
                    "--queue-ms" => cfg.queue_ms = parse(&mut args, &arg)?,
                    "--target-ms" => cfg.target_ms = parse(&mut args, &arg)?,
                    "--max-ppm" => cfg.max_ppm = parse(&mut args, &arg)?,
                    "--gain-db" => cfg.gain_db = parse(&mut args, &arg)?,
                    "--source-timeout-ms" => cfg.source_timeout_ms = parse(&mut args, &arg)?,
                    "--startup-timeout-ms" => cfg.startup_timeout_ms = parse(&mut args, &arg)?,
                    "--latency-file" => cfg.latency_file = Some(PathBuf::from(next(&mut args, &arg)?)),
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
            if self.capture_period == 0 || self.output_period == 0 {
                return Err("period sizes must be greater than zero".to_owned());
            }
            if self.buffer_periods < 2 {
                return Err("--buffer-periods must be >= 2".to_owned());
            }
            if self.queue_ms < 20 || self.target_ms == 0 || self.target_ms >= self.queue_ms {
                return Err("require queue-ms >= 20 and 0 < target-ms < queue-ms".to_owned());
            }
            if !(1.0..=1_000.0).contains(&self.max_ppm) {
                return Err("--max-ppm must be in 1..=1000".to_owned());
            }
            if !(-60.0..=0.0).contains(&self.gain_db) {
                return Err("--gain-db must be in -60..=0".to_owned());
            }
            Ok(())
        }

        fn queue_frames(&self) -> usize {
            ms_to_frames(self.queue_ms)
        }

        fn target_frames(&self) -> usize {
            ms_to_frames(self.target_ms)
        }
    }

    fn print_help() {
        eprintln!(
            "Aurora R2 single-process appliance\n\
             Direct ALSA eARC capture -> IEC61937 -> liborender -> clock match -> TDM ALSA.\n\n\
             --capture-device NAME      default hw:AuroraEARC,0\n\
             --playback-device NAME     default hw:AuroraTDM16,0\n\
             --liborender PATH          liborender.so path\n\
             --bridge PATH              Harletty bridge path\n\
             --layout PATH              Aurora 7.1.4 layout YAML\n\
             --omniphony-config PATH    optional Omniphony config YAML\n\
             --queue-ms MS              decoded PCM queue (default 200)\n\
             --target-ms MS             clock-control target (default 80)\n\
             --max-ppm PPM              correction clamp (default 300)\n\
             --gain-db DB               output headroom (default -3)\n\
             --latency-file PATH        software A/V latency report\n\
             --no-latency-file          disable report\n\
             --stats                    1 Hz diagnostics"
        );
    }

    fn next<I>(args: &mut I, name: &str) -> Result<String, String>
    where
        I: Iterator<Item = String>,
    {
        args.next().ok_or_else(|| format!("missing value for {name}"))
    }

    fn parse<I, T>(args: &mut I, name: &str) -> Result<T, String>
    where
        I: Iterator<Item = String>,
        T: std::str::FromStr,
        T::Err: std::fmt::Display,
    {
        let raw = next(args, name)?;
        raw.parse::<T>()
            .map_err(|error| format!("invalid {name} '{raw}': {error}"))
    }

    fn ms_to_frames(ms: usize) -> usize {
        ms.saturating_mul(OUTPUT_SAMPLE_RATE as usize) / 1_000
    }

    struct FrameQueue {
        state: Mutex<QueueState>,
        not_empty: Condvar,
        not_full: Condvar,
        buffered: AtomicUsize,
        closed: AtomicBool,
    }

    struct QueueState {
        frames: Vec<Frame12>,
        read: usize,
        write: usize,
        len: usize,
        closed: bool,
        error: Option<String>,
    }

    impl FrameQueue {
        fn new(capacity: usize) -> Self {
            let capacity = capacity.max(SOURCE_BLOCK_FRAMES * 2);
            Self {
                state: Mutex::new(QueueState {
                    frames: vec![[0.0; RENDER_CHANNELS]; capacity],
                    read: 0,
                    write: 0,
                    len: 0,
                    closed: false,
                    error: None,
                }),
                not_empty: Condvar::new(),
                not_full: Condvar::new(),
                buffered: AtomicUsize::new(0),
                closed: AtomicBool::new(false),
            }
        }

        fn push_interleaved(&self, samples: &[f32]) -> Result<(), String> {
            if samples.len() % RENDER_CHANNELS != 0 {
                return Err("renderer output is not 12-channel frame aligned".to_owned());
            }
            let mut source = samples.chunks_exact(RENDER_CHANNELS);
            while source.len() > 0 {
                let mut state = self.state.lock().map_err(|_| "PCM queue poisoned".to_owned())?;
                while state.len == state.frames.len() && !state.closed {
                    state = self
                        .not_full
                        .wait(state)
                        .map_err(|_| "PCM queue poisoned while waiting".to_owned())?;
                }
                if state.closed {
                    return Err("PCM queue closed".to_owned());
                }
                let writable = (state.frames.len() - state.len).min(source.len());
                for raw in source.by_ref().take(writable) {
                    let write = state.write;
                    state.frames[write].copy_from_slice(raw);
                    state.write = (write + 1) % state.frames.len();
                    state.len += 1;
                }
                self.buffered.fetch_add(writable, Ordering::Release);
                drop(state);
                self.not_empty.notify_one();
            }
            Ok(())
        }

        fn pop(&self, out: &mut [Frame12], timeout: Duration) -> io::Result<usize> {
            let state = self
                .state
                .lock()
                .map_err(|_| io::Error::other("PCM queue poisoned"))?;
            let (mut state, wait) = self
                .not_empty
                .wait_timeout_while(state, timeout, |s| s.len == 0 && !s.closed)
                .map_err(|_| io::Error::other("PCM queue poisoned while waiting"))?;
            if state.len == 0 {
                if state.closed {
                    if let Some(error) = state.error.as_deref() {
                        return Err(io::Error::new(io::ErrorKind::InvalidData, error));
                    }
                    return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "producer ended"));
                }
                if wait.timed_out() {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "producer timed out"));
                }
            }
            let count = state.len.min(out.len());
            for dst in &mut out[..count] {
                *dst = state.frames[state.read];
                state.read = (state.read + 1) % state.frames.len();
                state.len -= 1;
            }
            drop(state);
            self.not_full.notify_one();
            Ok(count)
        }

        fn consumed(&self) {
            let previous = self.buffered.fetch_sub(1, Ordering::AcqRel);
            debug_assert!(previous > 0);
        }

        fn buffered(&self) -> usize {
            self.buffered.load(Ordering::Acquire)
        }

        fn close(&self, error: Option<String>) {
            if let Ok(mut state) = self.state.lock() {
                state.closed = true;
                state.error = error;
            }
            self.closed.store(true, Ordering::Release);
            self.not_empty.notify_all();
            self.not_full.notify_all();
        }
    }

    struct QueueReader {
        queue: Arc<FrameQueue>,
        block: [Frame12; SOURCE_BLOCK_FRAMES],
        len: usize,
        index: usize,
        timeout: Duration,
    }

    impl QueueReader {
        fn next_frame(&mut self) -> io::Result<Frame12> {
            if self.index >= self.len {
                self.len = self.queue.pop(&mut self.block, self.timeout)?;
                self.index = 0;
            }
            let frame = self.block[self.index];
            self.index += 1;
            self.queue.consumed();
            Ok(frame)
        }
    }

    fn wait_for_prime(queue: &FrameQueue, target: usize, timeout: Duration) -> Result<(), String> {
        let start = Instant::now();
        loop {
            let buffered = queue.buffered();
            if buffered >= target.max(RESAMPLER_PRIME_FRAMES) {
                return Ok(());
            }
            if queue.closed.load(Ordering::Acquire) {
                return Err(format!("producer stopped during startup at {buffered} frames"));
            }
            if start.elapsed() >= timeout {
                return Err(format!("startup prime timeout: {buffered}/{target} frames"));
            }
            thread::sleep(Duration::from_millis(2));
        }
    }

    fn spawn_ingress(
        queue: Arc<FrameQueue>,
        config: Config,
        engine_latency: Arc<AtomicU64>,
        object_count: Arc<AtomicU64>,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let result = run_ingress(&queue, &config, &engine_latency, &object_count);
            match result {
                Ok(()) => queue.close(None),
                Err(error) => queue.close(Some(error)),
            }
        })
    }

    fn run_ingress(
        queue: &FrameQueue,
        config: &Config,
        engine_latency: &AtomicU64,
        object_count: &AtomicU64,
    ) -> Result<(), String> {
        let mut capture = AlsaCapture::open(&AlsaCaptureConfig {
            device: config.capture_device.clone(),
            sample_rate: CAPTURE_RATE,
            channels: CAPTURE_CHANNELS,
            period_frames: config.capture_period,
            buffer_periods: config.buffer_periods,
        })?;
        let capture_period = capture.period_frames();
        let mut capture_samples = vec![0_i32; capture_period * CAPTURE_CHANNELS];
        let mut iec_words = Vec::with_capacity(capture_samples.len() * 2);
        let mut parser = BurstParser::new(CodecFilter::Eac3);
        let mut renderer = OrenderHost::open(&OrenderHostConfig {
            library_path: config.liborender.clone(),
            bridge_path: config.bridge.clone(),
            speaker_layout_path: config.layout.clone(),
            config_yaml_path: config.omniphony_config.clone(),
        })?;

        eprintln!(
            "aurora-appliance: ingress active capture={} rate={} ch={} period={} buffer={} renderer={}",
            config.capture_device,
            CAPTURE_RATE,
            CAPTURE_CHANNELS,
            capture.period_frames(),
            capture.buffer_frames(),
            renderer.build_id()
        );

        let mut last_stats = Instant::now();
        let mut bursts = 0_u64;
        loop {
            capture.read_interleaved(&mut capture_samples)?;
            s32_samples_to_iec_words(&capture_samples, &mut iec_words);
            let mut callback_error: Option<String> = None;
            parser.push_each(&iec_words, |data_type, payload| {
                if callback_error.is_some() {
                    return;
                }
                if data_type != 0x15 {
                    callback_error = Some(format!("unexpected IEC61937 data type 0x{data_type:02x}"));
                    return;
                }
                let render_result = renderer.process_raw(payload, |samples, _frames| {
                    if callback_error.is_none() {
                        if let Err(error) = queue.push_interleaved(samples) {
                            callback_error = Some(error);
                        }
                    }
                });
                if let Err(error) = render_result {
                    callback_error = Some(error);
                    return;
                }
                engine_latency.store(renderer.output_latency_samples(), Ordering::Release);
                object_count.store(u64::from(renderer.object_count()), Ordering::Release);
                bursts = bursts.saturating_add(1);
            });
            if let Some(error) = callback_error {
                return Err(error);
            }

            if config.stats && last_stats.elapsed() >= Duration::from_secs(1) {
                eprintln!(
                    "aurora-appliance: ingress bursts={} objects={} capture_recoveries={} queue={}f",
                    bursts,
                    object_count.load(Ordering::Acquire),
                    capture.recoveries(),
                    queue.buffered()
                );
                bursts = 0;
                last_stats = Instant::now();
            }
        }
    }

    fn publish_latency(
        path: &Option<PathBuf>,
        queue_frames: usize,
        playback: &AlsaPlayback,
        engine_frames: u64,
    ) -> Option<f64> {
        let path = path.as_ref()?;
        let dma_frames = playback.delay_frames().ok()?;
        let total_frames = queue_frames
            .saturating_add(dma_frames)
            .saturating_add(RESAMPLER_LOOKAHEAD_FRAMES)
            .saturating_add(engine_frames as usize);
        let seconds = total_frames as f64 / f64::from(OUTPUT_SAMPLE_RATE);
        let temp = path.with_extension("tmp");
        let content = format!("-{seconds:.9}\n");
        if fs::write(&temp, content).is_ok() {
            let _ = fs::rename(&temp, path);
        }
        Some(seconds * 1_000.0)
    }

    pub fn run() -> Result<(), Box<dyn Error>> {
        let config = Config::parse().map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let queue = Arc::new(FrameQueue::new(config.queue_frames()));
        let engine_latency = Arc::new(AtomicU64::new(LATENCY_UNSET));
        let object_count = Arc::new(AtomicU64::new(0));
        let ingress_handle = spawn_ingress(
            Arc::clone(&queue),
            config.clone(),
            Arc::clone(&engine_latency),
            Arc::clone(&object_count),
        );

        wait_for_prime(
            &queue,
            config.target_frames(),
            Duration::from_millis(config.startup_timeout_ms),
        )?;

        let mut reader = QueueReader {
            queue: Arc::clone(&queue),
            block: [[0.0; RENDER_CHANNELS]; SOURCE_BLOCK_FRAMES],
            len: 0,
            index: 0,
            timeout: Duration::from_millis(config.source_timeout_ms),
        };
        let mut next_frame = || reader.next_frame();
        let mut resampler = BandlimitedResampler12::new();
        resampler.prime(&mut next_frame)?;

        let mut playback = AlsaPlayback::open(&AlsaPlaybackConfig {
            device: config.playback_device.clone(),
            sample_rate: OUTPUT_SAMPLE_RATE,
            channels: TDM_CHANNELS,
            period_frames: config.output_period,
            buffer_periods: config.buffer_periods,
        })
        .map_err(io::Error::other)?;
        let period = playback.period_frames();
        let interval = period as f64 / f64::from(OUTPUT_SAMPLE_RATE);
        let gain = db_to_linear(config.gain_db);
        let mut controller = AdaptiveClockController::new(config.target_frames(), config.max_ppm);
        let mut output = Vec::<i32>::with_capacity(period * TDM_CHANNELS);
        let mut last_stats = Instant::now();
        let mut last_latency = Instant::now();
        let mut periods = 0_u64;

        eprintln!(
            "aurora-appliance: output active device={} rate={} slots={} period={} buffer={} target={}f gain={:.2}dB",
            config.playback_device,
            OUTPUT_SAMPLE_RATE,
            TDM_CHANNELS,
            playback.period_frames(),
            playback.buffer_frames(),
            controller.target_frames(),
            config.gain_db
        );

        loop {
            let queued = queue.buffered();
            let ppm = controller.update(queued, interval);
            let source_step = AdaptiveClockController::step_from_ppm(ppm);
            output.clear();
            for _ in 0..period {
                let frame = resampler.render(source_step, &mut next_frame)?;
                output.extend_from_slice(&pack_frame_s32(&frame, gain));
            }
            let recovered = playback.write_interleaved(&output).map_err(io::Error::other)?;
            if recovered {
                controller.reset_after_discontinuity();
            }
            periods = periods.saturating_add(1);

            let mut latency_ms = None;
            if last_latency.elapsed() >= Duration::from_millis(250) {
                let engine = engine_latency.load(Ordering::Acquire);
                if engine != LATENCY_UNSET {
                    latency_ms = publish_latency(&config.latency_file, queue.buffered(), &playback, engine);
                }
                last_latency = Instant::now();
            }

            if config.stats && last_stats.elapsed() >= Duration::from_secs(1) {
                eprintln!(
                    "aurora-appliance: output queue={}f ({:.1}ms) correction={:+.1}ppm objects={} recoveries={} latency_ms={} periods={}",
                    queue.buffered(),
                    queue.buffered() as f64 * 1_000.0 / f64::from(OUTPUT_SAMPLE_RATE),
                    ppm,
                    object_count.load(Ordering::Acquire),
                    playback.recoveries(),
                    latency_ms.map_or_else(|| "n/a".to_owned(), |value| format!("{value:.1}")),
                    periods
                );
                last_stats = Instant::now();
            }
        }

        #[allow(unreachable_code)]
        {
            let _ = ingress_handle.join();
            Ok(())
        }
    }
}

#[cfg(target_os = "linux")]
fn main() {
    if let Err(error) = linux::run() {
        eprintln!("aurora-appliance: fatal: {error}");
        std::process::exit(1);
    }
}
