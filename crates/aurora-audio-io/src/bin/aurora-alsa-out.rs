#[path = "../alsa_out.rs"]
mod alsa_out;

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("aurora-alsa-out is only supported on Linux/ALSA targets");
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
mod linux {
    use std::env;
    use std::error::Error;
    use std::ffi::{CStr, CString};
    use std::io::{self, Read};
    use std::os::raw::{c_char, c_int, c_long, c_ulong, c_void};
    use std::ptr;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::alsa_out::{
        db_to_linear, pack_frame_s32, AdaptiveClockController, CubicResampler12, Frame12,
        OUTPUT_SAMPLE_RATE, RENDER_CHANNELS, TDM_CHANNELS,
    };

    const SOURCE_BLOCK_FRAMES: usize = 256;
    const SND_PCM_STREAM_PLAYBACK: c_int = 0;
    const SND_PCM_ACCESS_RW_INTERLEAVED: c_int = 3;

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
               --target-ms MS             PI controller queue target (default 80)\n\
               --max-ppm PPM              max clock correction (default 300)\n\
               --gain-db DB               output headroom (default -3.0)\n\
               --source-timeout-ms MS     fail-closed source timeout (default 500)\n\
               --startup-timeout-ms MS    source priming timeout (default 5000)\n\
               --stats                    print 1 Hz queue/ppm/xrun telemetry"
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

    enum SourcePacket {
        Data(Vec<Frame12>),
        Error(String),
        Eof,
    }

    struct SourceReader {
        receiver: Receiver<SourcePacket>,
        buffered_frames: Arc<AtomicUsize>,
        current: Vec<Frame12>,
        index: usize,
        timeout: Duration,
    }

    impl SourceReader {
        fn next_frame(&mut self) -> Result<Frame12, io::Error> {
            loop {
                if self.index < self.current.len() {
                    let frame = self.current[self.index];
                    self.index += 1;
                    self.buffered_frames.fetch_sub(1, Ordering::AcqRel);
                    return Ok(frame);
                }

                self.current.clear();
                self.index = 0;
                match self.receiver.recv_timeout(self.timeout) {
                    Ok(SourcePacket::Data(frames)) => self.current = frames,
                    Ok(SourcePacket::Error(message)) => {
                        return Err(io::Error::new(io::ErrorKind::InvalidData, message));
                    }
                    Ok(SourcePacket::Eof) => {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "Omniphony PCM stream ended",
                        ));
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "Omniphony PCM source timed out",
                        ));
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        return Err(io::Error::new(
                            io::ErrorKind::BrokenPipe,
                            "Omniphony PCM source disconnected",
                        ));
                    }
                }
            }
        }
    }

    fn spawn_source_reader(
        sender: SyncSender<SourcePacket>,
        buffered_frames: Arc<AtomicUsize>,
        finished: Arc<AtomicBool>,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let result = read_stdin_blocks(&sender, &buffered_frames);
            if let Err(message) = result {
                let _ = sender.send(SourcePacket::Error(message));
            }
            let _ = sender.send(SourcePacket::Eof);
            finished.store(true, Ordering::Release);
        })
    }

    fn read_stdin_blocks(
        sender: &SyncSender<SourcePacket>,
        buffered_frames: &AtomicUsize,
    ) -> Result<(), String> {
        let stdin = io::stdin();
        let mut input = stdin.lock();
        let frame_bytes = RENDER_CHANNELS * std::mem::size_of::<f32>();
        let mut bytes = vec![0_u8; SOURCE_BLOCK_FRAMES * frame_bytes];

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
            let mut frames = Vec::with_capacity(frame_count);
            for raw_frame in bytes[..filled].chunks_exact(frame_bytes) {
                let mut frame = [0.0_f32; RENDER_CHANNELS];
                for (channel, raw_sample) in raw_frame.chunks_exact(4).enumerate() {
                    frame[channel] = f32::from_le_bytes([
                        raw_sample[0],
                        raw_sample[1],
                        raw_sample[2],
                        raw_sample[3],
                    ]);
                }
                frames.push(frame);
            }

            buffered_frames.fetch_add(frames.len(), Ordering::AcqRel);
            let sent_len = frames.len();
            if sender.send(SourcePacket::Data(frames)).is_err() {
                buffered_frames.fetch_sub(sent_len, Ordering::AcqRel);
                return Ok(());
            }
            if filled < bytes.len() {
                return Ok(());
            }
        }
    }

    struct HwParams(*mut c_void);

    impl Drop for HwParams {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { snd_pcm_hw_params_free(self.0) };
            }
        }
    }

    struct AlsaPlayback {
        pcm: *mut c_void,
        channels: usize,
        xruns: u64,
        period_frames: usize,
        buffer_frames: usize,
    }

    impl AlsaPlayback {
        fn open(config: &Config) -> Result<Self, String> {
            let device = CString::new(config.device.as_str())
                .map_err(|_| "ALSA device name contains NUL".to_owned())?;
            let mut pcm = ptr::null_mut();
            alsa_check(
                unsafe { snd_pcm_open(&mut pcm, device.as_ptr(), SND_PCM_STREAM_PLAYBACK, 0) },
                "snd_pcm_open",
            )?;

            let setup = (|| -> Result<(usize, usize), String> {
                let mut raw_params = ptr::null_mut();
                alsa_check(
                    unsafe { snd_pcm_hw_params_malloc(&mut raw_params) },
                    "snd_pcm_hw_params_malloc",
                )?;
                let params = HwParams(raw_params);
                alsa_check(
                    unsafe { snd_pcm_hw_params_any(pcm, params.0) },
                    "snd_pcm_hw_params_any",
                )?;
                alsa_check(
                    unsafe {
                        snd_pcm_hw_params_set_access(pcm, params.0, SND_PCM_ACCESS_RW_INTERLEAVED)
                    },
                    "set RW_INTERLEAVED",
                )?;

                let format_name = CString::new("S32_LE").expect("static format name");
                let format = unsafe { snd_pcm_format_value(format_name.as_ptr()) };
                if format < 0 {
                    return Err("ALSA does not recognize S32_LE".to_owned());
                }
                alsa_check(
                    unsafe { snd_pcm_hw_params_set_format(pcm, params.0, format) },
                    "set S32_LE",
                )?;
                alsa_check(
                    unsafe { snd_pcm_hw_params_set_channels(pcm, params.0, TDM_CHANNELS as u32) },
                    "set 16 channels",
                )?;
                alsa_check(
                    unsafe { snd_pcm_hw_params_set_rate(pcm, params.0, OUTPUT_SAMPLE_RATE, 0) },
                    "set 48000 Hz",
                )?;

                let mut period = config.period_frames as c_ulong;
                let mut direction: c_int = 0;
                alsa_check(
                    unsafe {
                        snd_pcm_hw_params_set_period_size_near(
                            pcm,
                            params.0,
                            &mut period,
                            &mut direction,
                        )
                    },
                    "set ALSA period",
                )?;
                let mut buffer = period.saturating_mul(config.buffer_periods as c_ulong);
                alsa_check(
                    unsafe { snd_pcm_hw_params_set_buffer_size_near(pcm, params.0, &mut buffer) },
                    "set ALSA buffer",
                )?;
                alsa_check(
                    unsafe { snd_pcm_hw_params(pcm, params.0) },
                    "apply ALSA hw params",
                )?;
                alsa_check(unsafe { snd_pcm_prepare(pcm) }, "prepare ALSA PCM")?;
                Ok((period as usize, buffer as usize))
            })();

            let (period_frames, buffer_frames) = match setup {
                Ok(values) => values,
                Err(error) => {
                    unsafe { snd_pcm_close(pcm) };
                    return Err(error);
                }
            };

            if period_frames != config.period_frames {
                eprintln!(
                    "aurora-alsa-out: ALSA adjusted period {} -> {} frames",
                    config.period_frames, period_frames
                );
            }

            Ok(Self {
                pcm,
                channels: TDM_CHANNELS,
                xruns: 0,
                period_frames,
                buffer_frames,
            })
        }

        fn write_interleaved(&mut self, samples: &[i32]) -> Result<(), String> {
            if samples.len() % self.channels != 0 {
                return Err("internal error: ALSA write is not frame-aligned".to_owned());
            }

            let total_frames = samples.len() / self.channels;
            let mut frame_offset = 0usize;
            while frame_offset < total_frames {
                let sample_offset = frame_offset * self.channels;
                let remaining = total_frames - frame_offset;
                let result = unsafe {
                    snd_pcm_writei(
                        self.pcm,
                        samples[sample_offset..].as_ptr().cast::<c_void>(),
                        remaining as c_ulong,
                    )
                };

                if result > 0 {
                    frame_offset += result as usize;
                    continue;
                }
                if result == 0 {
                    return Err("ALSA write returned zero frames".to_owned());
                }

                let error_code = result as c_int;
                let recovered = unsafe { snd_pcm_recover(self.pcm, error_code, 1) };
                if recovered < 0 {
                    return Err(format!(
                        "ALSA write failed: {}; recovery failed: {}",
                        alsa_error_text(error_code),
                        alsa_error_text(recovered)
                    ));
                }
                self.xruns += 1;
            }
            Ok(())
        }
    }

    impl Drop for AlsaPlayback {
        fn drop(&mut self) {
            if !self.pcm.is_null() {
                unsafe {
                    snd_pcm_close(self.pcm);
                }
            }
        }
    }

    fn alsa_check(code: c_int, operation: &str) -> Result<(), String> {
        if code < 0 {
            Err(format!("{operation}: {}", alsa_error_text(code)))
        } else {
            Ok(())
        }
    }

    fn alsa_error_text(code: c_int) -> String {
        let message = unsafe { snd_strerror(code) };
        if message.is_null() {
            return format!("ALSA error {code}");
        }
        unsafe { CStr::from_ptr(message) }
            .to_string_lossy()
            .into_owned()
    }

    fn wait_for_prime(
        buffered_frames: &AtomicUsize,
        finished: &AtomicBool,
        target_frames: usize,
        timeout: Duration,
    ) -> Result<(), String> {
        let started = Instant::now();
        loop {
            let available = buffered_frames.load(Ordering::Acquire);
            if available >= target_frames {
                return Ok(());
            }
            if finished.load(Ordering::Acquire) {
                if available >= 3 {
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

    pub fn run() -> Result<(), Box<dyn Error>> {
        let config = Config::parse().map_err(|error| {
            io::Error::new(io::ErrorKind::InvalidInput, format!("{error}; use --help"))
        })?;
        let queue_frames = config.queue_frames();
        let target_frames = config.target_frames();
        let block_capacity = queue_frames.div_ceil(SOURCE_BLOCK_FRAMES).max(2);
        let (sender, receiver) = mpsc::sync_channel::<SourcePacket>(block_capacity);
        let buffered_frames = Arc::new(AtomicUsize::new(0));
        let source_finished = Arc::new(AtomicBool::new(false));
        let _reader_handle = spawn_source_reader(
            sender,
            Arc::clone(&buffered_frames),
            Arc::clone(&source_finished),
        );

        wait_for_prime(
            &buffered_frames,
            &source_finished,
            target_frames,
            Duration::from_millis(config.startup_timeout_ms),
        )?;

        let mut source = SourceReader {
            receiver,
            buffered_frames: Arc::clone(&buffered_frames),
            current: Vec::new(),
            index: 0,
            timeout: Duration::from_millis(config.source_timeout_ms),
        };
        let mut next = || source.next_frame();
        let mut resampler = CubicResampler12::default();
        resampler.prime(&mut next)?;

        let mut playback = AlsaPlayback::open(&config).map_err(io::Error::other)?;
        let period_frames = playback.period_frames;
        let interval_seconds = period_frames as f64 / f64::from(OUTPUT_SAMPLE_RATE);
        let mut controller = AdaptiveClockController::new(target_frames, config.max_ppm);
        let gain = db_to_linear(config.gain_db);
        let mut output = Vec::<i32>::with_capacity(period_frames * TDM_CHANNELS);
        let mut last_stats = Instant::now();
        let mut periods_written = 0_u64;

        eprintln!(
            "aurora-alsa-out: direct ALSA active device={} format=S32_LE rate={} channels={} period={} buffer={} queue_target={}f gain={:.2}dB max_ppm={:.1}",
            config.device,
            OUTPUT_SAMPLE_RATE,
            TDM_CHANNELS,
            playback.period_frames,
            playback.buffer_frames,
            controller.target_frames(),
            config.gain_db,
            config.max_ppm
        );

        loop {
            let queued = buffered_frames.load(Ordering::Acquire);
            let ppm = controller.update(queued, interval_seconds);
            let source_step = AdaptiveClockController::step_from_ppm(ppm);
            output.clear();
            for _ in 0..period_frames {
                let frame = resampler.render(source_step, &mut next)?;
                let packed = pack_frame_s32(&frame, gain);
                output.extend_from_slice(&packed);
            }
            playback
                .write_interleaved(&output)
                .map_err(io::Error::other)?;
            periods_written += 1;

            if config.stats && last_stats.elapsed() >= Duration::from_secs(1) {
                let queued_now = buffered_frames.load(Ordering::Acquire);
                eprintln!(
                    "aurora-alsa-out: queued={}f ({:.1}ms) correction={:+.2}ppm xruns={} periods={}",
                    queued_now,
                    queued_now as f64 * 1_000.0 / f64::from(OUTPUT_SAMPLE_RATE),
                    ppm,
                    playback.xruns,
                    periods_written
                );
                last_stats = Instant::now();
            }
        }
    }

    #[link(name = "asound")]
    extern "C" {
        fn snd_pcm_open(
            pcm: *mut *mut c_void,
            name: *const c_char,
            stream: c_int,
            mode: c_int,
        ) -> c_int;
        fn snd_pcm_close(pcm: *mut c_void) -> c_int;
        fn snd_pcm_prepare(pcm: *mut c_void) -> c_int;
        fn snd_pcm_writei(pcm: *mut c_void, buffer: *const c_void, frames: c_ulong) -> c_long;
        fn snd_pcm_recover(pcm: *mut c_void, error: c_int, silent: c_int) -> c_int;
        fn snd_pcm_hw_params_malloc(params: *mut *mut c_void) -> c_int;
        fn snd_pcm_hw_params_free(params: *mut c_void);
        fn snd_pcm_hw_params_any(pcm: *mut c_void, params: *mut c_void) -> c_int;
        fn snd_pcm_hw_params_set_access(
            pcm: *mut c_void,
            params: *mut c_void,
            access: c_int,
        ) -> c_int;
        fn snd_pcm_hw_params_set_format(
            pcm: *mut c_void,
            params: *mut c_void,
            format: c_int,
        ) -> c_int;
        fn snd_pcm_hw_params_set_channels(
            pcm: *mut c_void,
            params: *mut c_void,
            channels: u32,
        ) -> c_int;
        fn snd_pcm_hw_params_set_rate(
            pcm: *mut c_void,
            params: *mut c_void,
            rate: u32,
            direction: c_int,
        ) -> c_int;
        fn snd_pcm_hw_params_set_period_size_near(
            pcm: *mut c_void,
            params: *mut c_void,
            frames: *mut c_ulong,
            direction: *mut c_int,
        ) -> c_int;
        fn snd_pcm_hw_params_set_buffer_size_near(
            pcm: *mut c_void,
            params: *mut c_void,
            frames: *mut c_ulong,
        ) -> c_int;
        fn snd_pcm_hw_params(pcm: *mut c_void, params: *mut c_void) -> c_int;
        fn snd_pcm_format_value(name: *const c_char) -> c_int;
        fn snd_strerror(error: c_int) -> *const c_char;
    }
}

#[cfg(target_os = "linux")]
fn main() {
    if let Err(error) = linux::run() {
        eprintln!("aurora-alsa-out: fatal: {error}");
        std::process::exit(1);
    }
}
