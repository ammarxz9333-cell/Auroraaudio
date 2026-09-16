use std::path::PathBuf;

use aurora_network_esp_avb::{EspAvbEndpointMedium, EspAvbFanoutPlan};
use aurora_network_genavb_runtime::{RuntimeLifecycle, SixStreamGenAvbRuntime};
use aurora_network_genavb_session::{ExpectedStream, StereoEndpointRole};
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

fn main() {
    let mut args = std::env::args_os().skip(1);
    let shim = PathBuf::from(args.next().expect("shim path"));
    assert!(args.next().is_none(), "unexpected extra argument");

    let plan = EspAvbFanoutPlan::canonical(EspAvbEndpointMedium::WirelessEsp32C6);
    let mut runtime = SixStreamGenAvbRuntime::load(shim, expected_streams(), plan, 48)
        .expect("load six-stream runtime");
    assert_eq!(runtime.lifecycle(), RuntimeLifecycle::Loaded);

    runtime.open_control().expect("open AVDECC control");
    assert_eq!(runtime.poll_fd().expect("poll fd"), 42);

    for connected in 1..=6 {
        let update = runtime
            .receive_control()
            .expect("receive CONNECT")
            .expect("CONNECT event");
        assert!(update.connected);
        assert_eq!(runtime.connected_count(), connected);
        assert_eq!(runtime.session_complete(), connected == 6);
    }

    runtime
        .prepare_complete(480)
        .expect("prepare six AVDECC talkers");
    assert_eq!(runtime.lifecycle(), RuntimeLifecycle::Prepared);
    runtime.start().expect("start six AVDECC talkers");
    assert_eq!(runtime.lifecycle(), RuntimeLifecycle::Started);

    let samples = vec![0.125_f32; 48 * 12];
    let block = NetworkAudioBlock {
        sequence: 23,
        timestamp: MediaTimestamp::new(96_000, AURORA_NETWORK_MEDIA_RATE)
            .expect("valid media timestamp"),
        format: NetworkAudioFormat {
            sample_rate: AURORA_NETWORK_MEDIA_RATE,
            channels: 12,
            block_frames: 48,
        },
        samples: &samples,
    };
    runtime.submit(&block).expect("submit one 7.1.4 block");

    let update = runtime
        .receive_control()
        .expect("receive DISCONNECT")
        .expect("DISCONNECT event");
    assert!(!update.connected);
    assert_eq!(update.role, StereoEndpointRole::Surround);
    assert_eq!(runtime.lifecycle(), RuntimeLifecycle::ControlOpen);
    assert_eq!(runtime.connected_count(), 5);
    assert!(!runtime.session_complete());

    runtime.close_control().expect("close AVDECC control");
    assert_eq!(runtime.lifecycle(), RuntimeLifecycle::Loaded);

    println!(
        "aurora-genavb-runtime: PASS connects=6 prepare=6 start=6 submit=6 disconnect=abort-all"
    );
}
