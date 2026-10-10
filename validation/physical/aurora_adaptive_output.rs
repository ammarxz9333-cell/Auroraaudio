use aurora_realtime_engine::{
    create_adaptive_duplex_bridge, AdaptiveDuplexFault, DuplexBridgeConfig, DuplexFaultPolicy,
    DuplexHealth, DriftControllerConfig, RubatoAsrc,
};
use std::{
    env,
    io::{self, BufReader, BufWriter, Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

const DEFAULT_CHANNELS: usize = 12;
const DEFAULT_SAMPLE_RATE: u32 = 48_000;
const DEFAULT_BLOCK_FRAMES: usize = 256;
const DEFAULT_CAPACITY_FRAMES: usize = 16_384;
const DEFAULT_TARGET_FILL_FRAMES: usize = 4_096;
const BACKPRESSURE_SLEEP: Duration = Duration::from_millis(1);

#[derive(Debug, Clone, Copy)]
struct Config {
    channels: usize,
    sample_rate: u32,
    block_frames: usize,
    capacity_frames: usize,
    target_fill_frames: usize,
    self_test: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            channels: DEFAULT_CHANNELS,
            sample_rate: DEFAULT_SAMPLE_RATE,
            block_frames: DEFAULT_BLOCK_FRAMES,
            capacity_frames: DEFAULT_CAPACITY_FRAMES,
            target_fill_frames: DEFAULT_TARGET_FILL_FRAMES,
            self_test: false,
        }
    }
}

fn parse_args() -> Result<Config, String> {
    let mut config = Config::default();
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--channels" => {
                config.channels = args
                    .next()
                    .ok_or("--channels requires a value")?
                    .parse()
                    .map_err(|_| "invalid --channels")?;
            }
            "--sample-rate" => {
                config.sample_rate = args
                    .next()
                    .ok_or("--sample-rate requires a value")?
                    .parse()
                    .map_err(|_| "invalid --sample-rate")?;
            }
            "--block-frames" => {
                config.block_frames = args
                    .next()
                    .ok_or("--block-frames requires a value")?
                    .parse()
                    .map_err(|_| "invalid --block-frames")?;
            }
            "--capacity-frames" => {
                config.capacity_frames = args
                    .next()
                    .ok_or("--capacity-frames requires a value")?
                    .parse()
                    .map_err(|_| "invalid --capacity-frames")?;
            }
            "--target-fill-frames" => {
                config.target_fill_frames = args
                    .next()
                    .ok_or("--target-fill-frames requires a value")?
                    .parse()
                    .map_err(|_| "invalid --target-fill-frames")?;
            }
            "--self-test" => config.self_test = true,
            "-h" | "--help" => {
                eprintln!(
                    "usage: aurora-adaptive-output [--channels N] [--sample-rate HZ] \
                     [--block-frames N] [--capacity-frames N] [--target-fill-frames N]\n\
                     stdin/stdout: interleaved S32_LE PCM; --self-test performs no I/O"
                );
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }
    if config.channels == 0
        || config.sample_rate == 0
        || config.block_frames < 2
        || config.capacity_frames <= config.block_frames * 2
        || config.target_fill_frames <= config.block_frames
        || config.target_fill_frames >= config.capacity_frames
    {
        return Err("invalid adaptive-output configuration".to_owned());
    }
    Ok(config)
}

fn sample_to_s32(sample: f32) -> i32 {
    let finite = if sample.is_finite() { sample } else { 0.0 };
    (finite.clamp(-1.0, 1.0) * i32::MAX as f32).round() as i32
}

fn decode_s32(bytes: &[u8], output: &mut Vec<f32>) -> Result<usize, String> {
    if bytes.len() % 4 != 0 {
        return Err("S32_LE byte stream is not sample-aligned".to_owned());
    }
    output.clear();
    output.reserve(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        let sample = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        output.push((sample as f32 / i32::MAX as f32).clamp(-1.0, 1.0));
    }
    Ok(output.len())
}

fn read_fullish<R: Read>(reader: &mut R, buffer: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match reader.read(&mut buffer[filled..])? {
            0 => break,
            count => filled += count,
        }
    }
    Ok(filled)
}

fn bridge_config(config: Config) -> DuplexBridgeConfig {
    DuplexBridgeConfig {
        channels: config.channels,
        capacity_frames: config.capacity_frames,
        target_fill_frames: config.target_fill_frames,
        correction_threshold_frames: config.block_frames,
    }
}

fn controller_config(config: Config) -> DriftControllerConfig {
    DriftControllerConfig {
        input_rate: config.sample_rate,
        output_rate: config.sample_rate,
        target_fill_frames: config.target_fill_frames,
        maximum_correction_ppm: 500.0,
        proportional_gain_ppm: 500.0,
        integral_gain_ppm_per_second: 0.2,
        maximum_step_ppm: 2.0,
        fatal_saturation_updates: 1_500,
    }
}

fn fault_policy(config: Config) -> DuplexFaultPolicy {
    DuplexFaultPolicy {
        maximum_excursion_frames: config.capacity_frames.saturating_sub(config.target_fill_frames),
        ..DuplexFaultPolicy::default()
    }
}

fn run_self_test(config: Config) -> Result<(), String> {
    let (producer, mut consumer, status) = create_adaptive_duplex_bridge(
        bridge_config(config),
        fault_policy(config),
        controller_config(config),
        Box::new(RubatoAsrc::default()),
        config.block_frames,
    )
    .map_err(|error| error.to_string())?;

    let mut phase = 0.0_f32;
    let mut make_frames = |frames: usize| {
        let mut samples = Vec::with_capacity(frames * config.channels);
        for _ in 0..frames {
            let base = phase.sin() * 0.2;
            phase += 0.03;
            for channel in 0..config.channels {
                samples.push(base * (1.0 - channel as f32 / (config.channels as f32 * 2.0)));
            }
        }
        samples
    };

    producer.push_interleaved(
        &make_frames(config.target_fill_frames + config.block_frames),
        config.channels,
    );
    let mut output = vec![0.0_f32; config.block_frames * config.channels];
    let mut peak = 0.0_f32;
    for _ in 0..64 {
        producer.push_interleaved(&make_frames(config.block_frames), config.channels);
        consumer.read_interleaved(&mut output, config.channels);
        if output.iter().any(|sample| !sample.is_finite()) {
            return Err("adaptive self-test produced non-finite PCM".to_owned());
        }
        for sample in &output {
            peak = peak.max(sample.abs());
        }
    }
    let snapshot = status.snapshot();
    if snapshot.fault != AdaptiveDuplexFault::None
        || snapshot.duplex.health == DuplexHealth::Fatal
        || snapshot.duplex.overflow_count != 0
        || snapshot.duplex.underflow_count != 0
        || peak <= 1.0e-8
    {
        return Err(format!("adaptive self-test failed: {snapshot:?} peak={peak}"));
    }
    eprintln!(
        "AURORA-ADAPTIVE-OUTPUT-SELFTEST-PASS channels={} rate={} block={} peak={:.6} ratio={:.9}",
        config.channels, config.sample_rate, config.block_frames, peak, snapshot.current_ratio
    );
    Ok(())
}

fn run_stream(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let (producer, mut consumer, status) = create_adaptive_duplex_bridge(
        bridge_config(config),
        fault_policy(config),
        controller_config(config),
        Box::new(RubatoAsrc::default()),
        config.block_frames,
    )?;

    let frame_bytes = config
        .channels
        .checked_mul(4)
        .ok_or("frame byte count overflow")?;
    let chunk_bytes = config
        .block_frames
        .checked_mul(frame_bytes)
        .ok_or("chunk byte count overflow")?;

    let stdin = io::stdin();
    let mut input = BufReader::new(stdin.lock());
    let mut byte_buffer = vec![0_u8; chunk_bytes];
    let mut samples = Vec::<f32>::with_capacity(config.block_frames * config.channels);

    let mut prefetched_frames = 0usize;
    while prefetched_frames < config.target_fill_frames {
        let read = read_fullish(&mut input, &mut byte_buffer)?;
        if read == 0 {
            return Err("input ended before adaptive buffer primed".into());
        }
        if read % frame_bytes != 0 {
            return Err(format!("S32_LE input ended on a partial {}-byte frame", frame_bytes).into());
        }
        decode_s32(&byte_buffer[..read], &mut samples)?;
        let frames = samples.len() / config.channels;
        producer.push_interleaved(&samples, config.channels);
        prefetched_frames += frames;
    }

    let done = Arc::new(AtomicBool::new(false));
    let done_for_output = Arc::clone(&done);
    let status_for_output = Arc::clone(&status);
    let output_config = config;

    let output_thread = thread::spawn(move || -> Result<u64, String> {
        let stdout = io::stdout();
        let mut writer = BufWriter::new(stdout.lock());
        let mut output = vec![0.0_f32; output_config.block_frames * output_config.channels];
        let mut bytes = vec![0_u8; output.len() * 4];
        let mut written_frames = 0_u64;

        loop {
            let before = status_for_output.snapshot();
            consumer.read_interleaved(&mut output, output_config.channels);
            let after = status_for_output.snapshot();

            if after.fault != AdaptiveDuplexFault::None || after.duplex.health == DuplexHealth::Fatal {
                return Err(format!("adaptive output faulted: {after:?}"));
            }
            if done_for_output.load(Ordering::Acquire)
                && after.duplex.underflow_count > before.duplex.underflow_count
            {
                break;
            }

            for (sample, dst) in output.iter().zip(bytes.chunks_exact_mut(4)) {
                dst.copy_from_slice(&sample_to_s32(*sample).to_le_bytes());
            }
            writer.write_all(&bytes).map_err(|error| error.to_string())?;
            written_frames = written_frames.saturating_add(output_config.block_frames as u64);
        }
        writer.flush().map_err(|error| error.to_string())?;
        Ok(written_frames)
    });

    let mut input_frames = prefetched_frames as u64;
    loop {
        let read = read_fullish(&mut input, &mut byte_buffer)?;
        if read == 0 {
            break;
        }
        if read % frame_bytes != 0 {
            done.store(true, Ordering::Release);
            return Err(format!("S32_LE input ended on a partial {}-byte frame", frame_bytes).into());
        }
        decode_s32(&byte_buffer[..read], &mut samples)?;
        let frames = samples.len() / config.channels;

        while status.snapshot().duplex.fill_frames + frames
            >= config.capacity_frames.saturating_sub(config.block_frames * 2)
        {
            thread::sleep(BACKPRESSURE_SLEEP);
            let snapshot = status.snapshot();
            if snapshot.fault != AdaptiveDuplexFault::None
                || snapshot.duplex.health == DuplexHealth::Fatal
            {
                done.store(true, Ordering::Release);
                return Err(format!("adaptive output faulted during producer backpressure: {snapshot:?}").into());
            }
        }

        producer.push_interleaved(&samples, config.channels);
        input_frames = input_frames.saturating_add(frames as u64);
    }

    done.store(true, Ordering::Release);
    let written_frames = output_thread
        .join()
        .map_err(|_| "adaptive output thread panicked")?
        .map_err(|error| format!("adaptive output thread failed: {error}"))?;

    let snapshot = status.snapshot();
    if snapshot.fault != AdaptiveDuplexFault::None || snapshot.duplex.health == DuplexHealth::Fatal {
        return Err(format!("adaptive output ended faulted: {snapshot:?}").into());
    }
    if snapshot.duplex.overflow_count != 0 {
        return Err(format!("adaptive output overflowed: {snapshot:?}").into());
    }

    eprintln!(
        "AURORA-ADAPTIVE-OUTPUT-PASS input_frames={} output_frames={} fill={} min_fill={} max_fill={} \
ratio={:.9} correction_ppm={:.3} estimated_input_clock_ppm={:.3} trusted={} \
underflows={} overflows={} fault={:?}",
        input_frames,
        written_frames,
        snapshot.duplex.fill_frames,
        snapshot.duplex.min_fill_frames,
        snapshot.duplex.max_fill_frames,
        snapshot.current_ratio,
        snapshot.correction_ppm,
        snapshot.estimated_input_clock_ppm,
        snapshot.clock_estimate_trusted,
        snapshot.duplex.underflow_count,
        snapshot.duplex.overflow_count,
        snapshot.fault,
    );
    Ok(())
}

fn main() {
    let config = match parse_args() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("AURORA-ADAPTIVE-OUTPUT-FAIL: {error}");
            std::process::exit(2);
        }
    };

    let result = if config.self_test {
        run_self_test(config).map_err(|error| error.into())
    } else {
        run_stream(config)
    };

    if let Err(error) = result {
        eprintln!("AURORA-ADAPTIVE-OUTPUT-FAIL: {error}");
        std::process::exit(1);
    }
}
