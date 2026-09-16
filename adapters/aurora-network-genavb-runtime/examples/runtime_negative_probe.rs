use std::path::PathBuf;

use aurora_network_esp_avb::{EspAvbEndpointMedium, EspAvbFanoutPlan};
use aurora_network_genavb_runtime::{
    GenAvbRuntimeError, RuntimeLifecycle, SixStreamGenAvbRuntime,
};
use aurora_network_genavb_session::{ExpectedStream, SessionError, StereoEndpointRole};
use aurora_realtime_audio_api::{
    MediaTimestamp, NetworkAudioBlock, NetworkAudioFormat, AURORA_NETWORK_MEDIA_RATE,
};

fn expected_streams() -> [ExpectedStream; 6] {
    [
        ExpectedStream {
            role: StereoEndpointRole::Front,
            stream_index: 10,
        },
        ExpectedStream {
            role: StereoEndpointRole::CenterLfe,
            stream_index: 11,
        },
        ExpectedStream {
            role: StereoEndpointRole::Surround,
            stream_index: 12,
        },
        ExpectedStream {
            role: StereoEndpointRole::BackSurround,
            stream_index: 13,
        },
        ExpectedStream {
            role: StereoEndpointRole::TopFront,
            stream_index: 14,
        },
        ExpectedStream {
            role: StereoEndpointRole::TopRear,
            stream_index: 15,
        },
    ]
}

fn runtime(shim: PathBuf) -> SixStreamGenAvbRuntime {
    let plan = EspAvbFanoutPlan::canonical(EspAvbEndpointMedium::WirelessEsp32C6);
    let mut runtime = SixStreamGenAvbRuntime::load(shim, expected_streams(), plan, 48)
        .expect("load six-stream runtime");
    runtime.open_control().expect("open AVDECC control");
    runtime
}

fn connect_six(runtime: &mut SixStreamGenAvbRuntime) {
    for _ in 0..6 {
        runtime
            .receive_control()
            .expect("receive CONNECT")
            .expect("CONNECT event");
    }
    assert_eq!(runtime.connected_count(), 6);
    assert!(runtime.session_complete());
}

fn block(sequence: u64, frame_index: u64) -> (Vec<f32>, NetworkAudioFormat, MediaTimestamp) {
    let samples = vec![0.125_f32; 48 * 12];
    let format = NetworkAudioFormat {
        sample_rate: AURORA_NETWORK_MEDIA_RATE,
        channels: 12,
        block_frames: 48,
    };
    let timestamp =
        MediaTimestamp::new(frame_index, AURORA_NETWORK_MEDIA_RATE).expect("valid timestamp");
    let _ = sequence;
    (samples, format, timestamp)
}

fn submit(runtime: &mut SixStreamGenAvbRuntime, sequence: u64, frame_index: u64) {
    let (samples, format, timestamp) = block(sequence, frame_index);
    let block = NetworkAudioBlock {
        sequence,
        timestamp,
        format,
        samples: &samples,
    };
    runtime.submit(&block).expect("submit immersive block");
}

fn scenario_prepare_fail(shim: PathBuf) {
    let mut runtime = runtime(shim);
    connect_six(&mut runtime);
    assert!(matches!(
        runtime.prepare_complete(480),
        Err(GenAvbRuntimeError::Endpoint { endpoint: 2, .. })
    ));
    assert_eq!(runtime.lifecycle(), RuntimeLifecycle::ControlOpen);
    assert!(runtime.session_complete());
    runtime.close_control().expect("close after prepare fault");
}

fn scenario_start_fail(shim: PathBuf) {
    let mut runtime = runtime(shim);
    connect_six(&mut runtime);
    runtime.prepare_complete(480).expect("prepare all six");
    assert!(matches!(
        runtime.start(),
        Err(GenAvbRuntimeError::Endpoint { endpoint: 2, .. })
    ));
    assert_eq!(runtime.lifecycle(), RuntimeLifecycle::ControlOpen);
    runtime.close_control().expect("close after start fault");
}

fn scenario_submit_fail(shim: PathBuf) {
    let mut runtime = runtime(shim);
    connect_six(&mut runtime);
    runtime.prepare_complete(480).expect("prepare all six");
    runtime.start().expect("start all six");

    let (samples, format, timestamp) = block(23, 96_000);
    let block = NetworkAudioBlock {
        sequence: 23,
        timestamp,
        format,
        samples: &samples,
    };
    assert!(matches!(
        runtime.submit(&block),
        Err(GenAvbRuntimeError::Endpoint { endpoint: 2, .. })
    ));
    assert_eq!(runtime.lifecycle(), RuntimeLifecycle::ControlOpen);
    runtime.close_control().expect("close after submit fault");
}

fn scenario_duplicate_connect(shim: PathBuf) {
    let mut runtime = runtime(shim);
    runtime
        .receive_control()
        .expect("first CONNECT")
        .expect("first CONNECT event");
    assert!(matches!(
        runtime.receive_control(),
        Err(GenAvbRuntimeError::Session(SessionError::DuplicateConnect(10)))
    ));
    assert_eq!(runtime.connected_count(), 0);
    assert!(!runtime.session_complete());
    assert_eq!(runtime.lifecycle(), RuntimeLifecycle::ControlOpen);
    runtime.close_control().expect("close after duplicate CONNECT");
}

fn scenario_reordered(shim: PathBuf) {
    let mut runtime = runtime(shim);
    connect_six(&mut runtime);
    runtime
        .prepare_complete(480)
        .expect("canonical prepare despite reordered CONNECT arrival");
    runtime.start().expect("start reordered session");
    submit(&mut runtime, 23, 96_000);
    runtime.stop().expect("stop reordered session");
    runtime.close_control().expect("close reordered session");
}

fn scenario_reconnect(shim: PathBuf) {
    let mut runtime = runtime(shim);
    connect_six(&mut runtime);
    runtime.prepare_complete(480).expect("prepare first epoch");
    runtime.start().expect("start first epoch");
    submit(&mut runtime, 23, 96_000);

    let update = runtime
        .receive_control()
        .expect("receive active DISCONNECT")
        .expect("DISCONNECT event");
    assert_eq!(update.role, StereoEndpointRole::Surround);
    assert!(!update.connected);
    assert_eq!(runtime.connected_count(), 5);
    assert!(!runtime.session_complete());
    assert_eq!(runtime.lifecycle(), RuntimeLifecycle::ControlOpen);
    assert!(matches!(
        runtime.start(),
        Err(GenAvbRuntimeError::InvalidLifecycle {
            expected: RuntimeLifecycle::Prepared,
            actual: RuntimeLifecycle::ControlOpen,
        })
    ));

    let update = runtime
        .receive_control()
        .expect("receive reconnect")
        .expect("reconnect CONNECT event");
    assert_eq!(update.role, StereoEndpointRole::Surround);
    assert!(update.connected);
    assert!(update.complete);
    assert_eq!(runtime.connected_count(), 6);
    assert!(runtime.session_complete());

    // A complete control-plane session is not enough: stale prepared state is
    // never restartable until all six talkers enter a fresh prepare epoch.
    assert!(matches!(
        runtime.start(),
        Err(GenAvbRuntimeError::InvalidLifecycle {
            expected: RuntimeLifecycle::Prepared,
            actual: RuntimeLifecycle::ControlOpen,
        })
    ));

    runtime
        .prepare_complete(480)
        .expect("fresh prepare after reconnect");
    runtime.start().expect("fresh start after reconnect");
    submit(&mut runtime, 24, 96_048);
    runtime.stop().expect("stop recovered epoch");
    runtime.close_control().expect("close recovered session");
}

fn main() {
    let mut args = std::env::args_os().skip(1);
    let scenario = args
        .next()
        .expect("scenario")
        .into_string()
        .expect("UTF-8 scenario");
    let shim = PathBuf::from(args.next().expect("shim path"));
    assert!(args.next().is_none(), "unexpected extra argument");

    std::env::set_var("AURORA_GENAVB_MOCK_MODE", &scenario);
    match scenario.as_str() {
        "prepare-fail" => scenario_prepare_fail(shim),
        "start-fail" => scenario_start_fail(shim),
        "submit-fail" => scenario_submit_fail(shim),
        "duplicate-connect" => scenario_duplicate_connect(shim),
        "reordered" => scenario_reordered(shim),
        "reconnect" => scenario_reconnect(shim),
        other => panic!("unknown scenario {other}"),
    }

    println!("aurora-genavb-runtime-negative: PASS scenario={scenario}");
}
