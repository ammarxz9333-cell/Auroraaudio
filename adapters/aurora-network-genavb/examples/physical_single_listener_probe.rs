#[cfg(target_os = "linux")]
mod linux_probe {
    use std::f32::consts::TAU;
    use std::os::raw::{c_int, c_short};
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use aurora_network_genavb::GenAvbNetworkTransport;
    use aurora_network_genavb_avdecc::{
        GenAvbAvdeccControl, GenAvbAvdeccEvent, GenAvbAvdeccEventKind,
    };
    use aurora_realtime_audio_api::{
        MediaTimestamp, NetworkAudioBlock, NetworkAudioFormat, NetworkAudioTransport,
        NetworkClockDiscipline, NetworkStreamConfig, NetworkTimingPolicy,
        AURORA_NETWORK_MEDIA_RATE,
    };

    const BLOCK_FRAMES: usize = 48;
    const CHANNELS: usize = 2;
    const TONE_HZ: f32 = 997.0;
    const TONE_AMPLITUDE: f32 = 0.10;
    const POLLIN: c_short = 0x0001;

    #[repr(C)]
    struct PollFd {
        fd: c_int,
        events: c_short,
        revents: c_short,
    }

    extern "C" {
        fn poll(fds: *mut PollFd, nfds: usize, timeout: c_int) -> c_int;
    }

    #[derive(Debug)]
    struct Args {
        shim: PathBuf,
        stream_index: u16,
        duration_ms: u64,
        target_latency_frames: u32,
        connect_timeout_ms: u64,
    }

    pub fn main() {
        if let Err(message) = run() {
            eprintln!("aurora-genavb-single-listener-host: FAIL {message}");
            std::process::exit(1);
        }
    }

    fn run() -> Result<(), String> {
        let args = parse_args()?;
        let mut control = GenAvbAvdeccControl::load(args.shim.clone())
            .map_err(|error| format!("control-load={error}"))?;
        control
            .open()
            .map_err(|error| format!("control-open={error}"))?;

        let event = wait_for_connect(
            &mut control,
            args.stream_index,
            Duration::from_millis(args.connect_timeout_ms),
        )?;
        validate_event(&event, args.stream_index)?;

        let mut talker = GenAvbNetworkTransport::load_avdecc(args.shim)
            .map_err(|error| format!("talker-load={error}"))?;
        let format = NetworkAudioFormat {
            sample_rate: AURORA_NETWORK_MEDIA_RATE,
            channels: CHANNELS,
            block_frames: BLOCK_FRAMES,
        };
        let timing = NetworkTimingPolicy {
            target_latency_frames: args.target_latency_frames,
            minimum_latency_frames: args.target_latency_frames,
            maximum_latency_frames: args.target_latency_frames,
            maximum_rate_correction_ppm: 0.0,
        };
        let config = NetworkStreamConfig {
            format,
            clock_discipline: NetworkClockDiscipline::PtpFollower,
            timing,
        };

        talker
            .prepare_from_avdecc(&control, args.stream_index, config)
            .map_err(|error| format!("prepare={error}"))?;
        talker.start().map_err(|error| format!("start={error}"))?;

        let blocks = args.duration_ms.max(1);
        let send_start = Instant::now();
        let mut samples = vec![0.0_f32; BLOCK_FRAMES * CHANNELS];
        for block_index in 0..blocks {
            if block_index != 0 {
                let deadline = send_start + Duration::from_micros(block_index * 1_000);
                let now = Instant::now();
                if deadline > now {
                    std::thread::sleep(deadline - now);
                }
            }

            reject_target_disconnect(&mut control, args.stream_index)?;
            fill_test_tone(&mut samples, block_index * BLOCK_FRAMES as u64);
            let frame_index = block_index * BLOCK_FRAMES as u64;
            let block = NetworkAudioBlock {
                sequence: block_index,
                timestamp: MediaTimestamp::new(frame_index, AURORA_NETWORK_MEDIA_RATE)
                    .map_err(|error| format!("timestamp={error:?}"))?,
                format,
                samples: &samples,
            };
            talker
                .submit(block)
                .map_err(|error| format!("submit-block-{block_index}={error}"))?;
        }

        talker.stop().map_err(|error| format!("stop={error}"))?;
        control
            .close()
            .map_err(|error| format!("control-close={error}"))?;

        let stream_id = hex_bytes(&event.stream_id, ':');
        let destination_mac = hex_bytes(&event.destination_mac, ':');
        println!(
            "{{\"schema\":\"aurora.genavb.single-listener-host-evidence.v1\",\"verdict\":\"HOST_PASS\",\"physical_complete\":false,\"stream_index\":{},\"port\":{},\"stream_class\":{},\"stream_id\":\"{}\",\"destination_mac\":\"{}\",\"sample_rate_hz\":{},\"channels\":{},\"bit_depth\":{},\"block_frames\":{},\"blocks_submitted\":{},\"target_latency_frames\":{},\"tone_hz\":{},\"listener_rx_evidence\":\"REQUIRED\",\"gptp_lock_evidence\":\"REQUIRED\"}}",
            event.stream_index,
            event.port,
            event.stream_class,
            stream_id,
            destination_mac,
            event.sample_rate_hz,
            event.channels,
            event.bit_depth,
            BLOCK_FRAMES,
            blocks,
            args.target_latency_frames,
            TONE_HZ,
        );
        println!(
            "aurora-genavb-single-listener-host: PASS connect=avdecc prepare=from-acmp send=aaf24-48k-stereo listener-rx=required gptp-lock=required"
        );
        Ok(())
    }

    fn parse_args() -> Result<Args, String> {
        let mut args = std::env::args().skip(1);
        let shim = args.next().map(PathBuf::from).ok_or_else(usage)?;
        let stream_index = parse_value::<u16>(args.next(), "stream-index")?;
        let duration_ms = parse_optional::<u64>(args.next(), 5_000, "duration-ms")?;
        let target_latency_frames =
            parse_optional::<u32>(args.next(), 480, "target-latency-frames")?;
        let connect_timeout_ms = parse_optional::<u64>(args.next(), 30_000, "connect-timeout-ms")?;
        if args.next().is_some() {
            return Err(usage());
        }
        if duration_ms == 0 || target_latency_frames == 0 || connect_timeout_ms == 0 {
            return Err("duration, target latency and connect timeout must be non-zero".into());
        }
        Ok(Args {
            shim,
            stream_index,
            duration_ms,
            target_latency_frames,
            connect_timeout_ms,
        })
    }

    fn usage() -> String {
        "usage: physical_single_listener_probe <shim.so> <stream-index> [duration-ms=5000] [target-latency-frames=480] [connect-timeout-ms=30000]".into()
    }

    fn parse_value<T>(value: Option<String>, name: &str) -> Result<T, String>
    where
        T: std::str::FromStr,
        T::Err: std::fmt::Display,
    {
        let raw = value.ok_or_else(usage)?;
        raw.parse::<T>()
            .map_err(|error| format!("invalid {name} {raw:?}: {error}"))
    }

    fn parse_optional<T>(value: Option<String>, default: T, name: &str) -> Result<T, String>
    where
        T: std::str::FromStr,
        T::Err: std::fmt::Display,
    {
        match value {
            Some(raw) => raw
                .parse::<T>()
                .map_err(|error| format!("invalid {name} {raw:?}: {error}")),
            None => Ok(default),
        }
    }

    fn wait_for_connect(
        control: &mut GenAvbAvdeccControl,
        stream_index: u16,
        timeout: Duration,
    ) -> Result<GenAvbAvdeccEvent, String> {
        let fd = control
            .poll_fd()
            .map_err(|error| format!("control-poll-fd={error}"))?;
        let deadline = Instant::now() + timeout;
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(format!("connect-timeout stream-index={stream_index}"));
            }
            let remaining = deadline - now;
            if !poll_readable(fd, remaining)? {
                return Err(format!("connect-timeout stream-index={stream_index}"));
            }
            match control
                .receive()
                .map_err(|error| format!("control-receive={error}"))?
            {
                None => {}
                Some(event)
                    if event.stream_index == stream_index
                        && event.kind == GenAvbAvdeccEventKind::Connect =>
                {
                    return Ok(event);
                }
                Some(event)
                    if event.stream_index == stream_index
                        && event.kind == GenAvbAvdeccEventKind::Disconnect =>
                {
                    return Err(format!(
                        "disconnect-before-connect stream-index={stream_index}"
                    ));
                }
                Some(_) => {}
            }
        }
    }

    fn reject_target_disconnect(
        control: &mut GenAvbAvdeccControl,
        stream_index: u16,
    ) -> Result<(), String> {
        let fd = control
            .poll_fd()
            .map_err(|error| format!("control-poll-fd={error}"))?;
        while poll_readable(fd, Duration::ZERO)? {
            match control
                .receive()
                .map_err(|error| format!("control-receive-during-send={error}"))?
            {
                Some(event)
                    if event.stream_index == stream_index
                        && event.kind == GenAvbAvdeccEventKind::Disconnect =>
                {
                    return Err(format!("active-disconnect stream-index={stream_index}"));
                }
                Some(_) | None => {}
            }
        }
        Ok(())
    }

    fn validate_event(event: &GenAvbAvdeccEvent, stream_index: u16) -> Result<(), String> {
        if event.kind != GenAvbAvdeccEventKind::Connect
            || event.stream_index != stream_index
            || event.sample_rate_hz != AURORA_NETWORK_MEDIA_RATE
            || event.channels as usize != CHANNELS
            || event.bit_depth != 24
        {
            return Err(format!("unsupported-connect={event:?}"));
        }
        Ok(())
    }

    fn poll_readable(fd: c_int, timeout: Duration) -> Result<bool, String> {
        let timeout_ms = timeout
            .as_millis()
            .min(c_int::MAX as u128)
            .try_into()
            .map_err(|_| "poll timeout conversion failed".to_string())?;
        let mut poll_fd = PollFd {
            fd,
            events: POLLIN,
            revents: 0,
        };
        let rc = unsafe { poll(&mut poll_fd, 1, timeout_ms) };
        if rc < 0 {
            return Err(format!("poll-failed rc={rc}"));
        }
        if rc == 0 {
            return Ok(false);
        }
        Ok((poll_fd.revents & POLLIN) != 0)
    }

    fn fill_test_tone(samples: &mut [f32], start_frame: u64) {
        for frame in 0..BLOCK_FRAMES {
            let absolute_frame = start_frame + frame as u64;
            let phase = TAU * TONE_HZ * absolute_frame as f32 / AURORA_NETWORK_MEDIA_RATE as f32;
            let value = TONE_AMPLITUDE * phase.sin();
            samples[frame * CHANNELS] = value;
            samples[frame * CHANNELS + 1] = -value;
        }
    }

    fn hex_bytes(bytes: &[u8], separator: char) -> String {
        let mut output = String::with_capacity(bytes.len() * 3);
        for (index, byte) in bytes.iter().enumerate() {
            if index != 0 {
                output.push(separator);
            }
            use std::fmt::Write as _;
            let _ = write!(output, "{byte:02x}");
        }
        output
    }
}

#[cfg(target_os = "linux")]
fn main() {
    linux_probe::main();
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("physical_single_listener_probe is supported only on Linux/NXP deployments");
    std::process::exit(2);
}
