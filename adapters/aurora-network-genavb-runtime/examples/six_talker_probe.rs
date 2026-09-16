use std::path::PathBuf;

use aurora_network_genavb_runtime::{
    GenAvbRuntimeError, GenAvbSixTalkerRuntime, GenAvbStereoBlockSet,
};
use aurora_network_genavb_session::{ExpectedStream, StereoEndpointRole};
use aurora_realtime_audio_api::{
    MediaTimestamp, NetworkAudioFormat, NetworkClockDiscipline, NetworkStreamConfig,
    NetworkTimingPolicy,
};

fn mapping() -> [ExpectedStream; 6] {
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

fn block_set<'a>(
    sequence: u64,
    frame_index: u64,
    samples: &'a [[f32; 96]; 6],
) -> GenAvbStereoBlockSet<'a> {
    GenAvbStereoBlockSet {
        sequence,
        timestamp: MediaTimestamp::new(frame_index, 48_000).unwrap(),
        format: stream_config().format,
        samples: std::array::from_fn(|index| &samples[index][..]),
    }
}

fn receive_initial_connections(
    runtime: &mut GenAvbSixTalkerRuntime,
) -> Result<(), GenAvbRuntimeError> {
    for _ in 0..5 {
        let update = runtime.receive_one()?.expect("CONNECT event");
        assert!(update.connected);
        assert!(!update.ready);
    }
    let update = runtime.receive_one()?.expect("sixth CONNECT event");
    assert!(update.connected);
    assert!(update.session_complete);
    assert!(update.ready);
    Ok(())
}

fn main() {
    let shim = PathBuf::from(std::env::args_os().nth(1).expect("shim path"));

    // A one-talker prepare fault invalidates the whole current prepare set, but
    // the complete AVDECC connection set can be retried transactionally.
    std::env::set_var("AURORA_GENAVB_MOCK_FAIL_PREPARE", "12");
    let mut prepare_fault =
        GenAvbSixTalkerRuntime::load(shim.clone(), mapping(), stream_config()).unwrap();
    prepare_fault.open().unwrap();
    for _ in 0..5 {
        prepare_fault.receive_one().unwrap();
    }
    assert!(matches!(
        prepare_fault.receive_one(),
        Err(GenAvbRuntimeError::Endpoint { endpoint: 2, .. })
    ));
    assert_eq!(prepare_fault.connected_count(), 6);
    assert_eq!(prepare_fault.prepared_count(), 0);
    assert!(!prepare_fault.is_ready());
    std::env::remove_var("AURORA_GENAVB_MOCK_FAIL_PREPARE");
    prepare_fault.reprepare_connected().unwrap();
    assert!(prepare_fault.is_ready());
    assert_eq!(prepare_fault.prepared_count(), 6);
    prepare_fault.close().unwrap();

    // Healthy epoch, submit fault recovery, partial-start rollback, then a live
    // DISCONNECT followed by reconnect and a fresh six-talker prepare epoch.
    std::env::set_var("AURORA_GENAVB_MOCK_DISCONNECT_RECONNECT", "1");
    let mut runtime = GenAvbSixTalkerRuntime::load(shim, mapping(), stream_config()).unwrap();
    runtime.open().unwrap();
    assert_eq!(runtime.poll_fd().unwrap(), 42);
    receive_initial_connections(&mut runtime).unwrap();
    assert_eq!(runtime.connected_count(), 6);
    assert_eq!(runtime.prepared_count(), 6);

    runtime.start().unwrap();
    let samples = [[0.25_f32; 96]; 6];
    runtime.submit(&block_set(1, 9_600, &samples)).unwrap();

    std::env::set_var("AURORA_GENAVB_MOCK_FAIL_SUBMIT", "14");
    assert!(matches!(
        runtime.submit(&block_set(2, 9_648, &samples)),
        Err(GenAvbRuntimeError::Endpoint { endpoint: 4, .. })
    ));
    std::env::remove_var("AURORA_GENAVB_MOCK_FAIL_SUBMIT");
    assert!(!runtime.is_started());
    assert!(!runtime.is_ready());
    assert_eq!(runtime.prepared_count(), 0);
    runtime.reprepare_connected().unwrap();
    assert!(runtime.is_ready());

    std::env::set_var("AURORA_GENAVB_MOCK_FAIL_START", "12");
    assert!(matches!(
        runtime.start(),
        Err(GenAvbRuntimeError::Endpoint { endpoint: 2, .. })
    ));
    std::env::remove_var("AURORA_GENAVB_MOCK_FAIL_START");
    assert!(runtime.is_ready());
    assert!(!runtime.is_started());
    runtime.start().unwrap();

    let disconnected = runtime.receive_one().unwrap().expect("DISCONNECT event");
    assert!(!disconnected.connected);
    assert!(disconnected.stopped_active_set);
    assert_eq!(disconnected.role, StereoEndpointRole::BackSurround);
    assert_eq!(runtime.connected_count(), 5);
    assert_eq!(runtime.prepared_count(), 0);
    assert!(!runtime.is_ready());

    let reconnected = runtime.receive_one().unwrap().expect("reCONNECT event");
    assert!(reconnected.connected);
    assert!(reconnected.session_complete);
    assert!(reconnected.ready);
    assert_eq!(runtime.connected_count(), 6);
    assert_eq!(runtime.prepared_count(), 6);

    runtime.start().unwrap();
    runtime.stop().unwrap();
    runtime.close().unwrap();
    std::env::remove_var("AURORA_GENAVB_MOCK_DISCONNECT_RECONNECT");

    println!("aurora-genavb-six-runtime: PASS connect=6 prepare-rollback=1 start-rollback=1 submit-abort=1 disconnect-stop=1 reconnect-reprepare=6");
}
