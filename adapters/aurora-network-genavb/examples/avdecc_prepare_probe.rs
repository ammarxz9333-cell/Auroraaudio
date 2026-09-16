use std::path::PathBuf;

use aurora_network_genavb::GenAvbNetworkTransport;
use aurora_network_genavb_avdecc::GenAvbAvdeccControl;
use aurora_realtime_audio_api::{
    NetworkAudioFormat, NetworkAudioTransport, NetworkClockDiscipline, NetworkStreamConfig,
    NetworkTimingPolicy, NetworkTransportError,
};

fn stream_config() -> NetworkStreamConfig {
    NetworkStreamConfig {
        format: NetworkAudioFormat {
            sample_rate: 48_000,
            channels: 2,
            block_frames: 48,
        },
        clock_discipline: NetworkClockDiscipline::PtpFollower,
        timing: NetworkTimingPolicy {
            target_latency_frames: 480,
            minimum_latency_frames: 480,
            maximum_latency_frames: 480,
            maximum_rate_correction_ppm: 0.0,
        },
    }
}

fn main() {
    let mut args = std::env::args_os().skip(1);
    let primary = PathBuf::from(args.next().expect("primary shim path"));
    let secondary = PathBuf::from(args.next().expect("secondary shim path"));
    assert!(args.next().is_none(), "unexpected extra argument");

    let mut control = GenAvbAvdeccControl::load(primary.clone()).expect("load AVDECC control");
    control.open().expect("open AVDECC control");

    let mut talker =
        GenAvbNetworkTransport::load_avdecc(primary.clone()).expect("load AVDECC talker");
    talker
        .prepare_from_avdecc(&control, 4, stream_config())
        .expect("prepare talker from AVDECC state");
    assert_eq!(
        talker.poll_event(),
        Some(aurora_realtime_audio_api::NetworkTransportEvent::Prepared)
    );

    let mut wrong_shim =
        GenAvbNetworkTransport::load_avdecc(secondary).expect("load second AVDECC talker");
    assert_eq!(
        wrong_shim.prepare_from_avdecc(&control, 4, stream_config()),
        Err(NetworkTransportError::WorkerFault)
    );
    assert_eq!(
        wrong_shim.prepare(stream_config()),
        Err(NetworkTransportError::WorkerFault)
    );

    control.close().expect("close AVDECC control");
    println!("aurora-genavb-rust-avdecc: PASS prepare=ffi same-shim=required static-placeholder=forbidden");
}
