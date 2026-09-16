use std::sync::{Arc, Mutex};

use aurora_network_esp_avb::{
    EspAvbEndpointMedium, EspAvbFanoutPlan, EspAvbTransportArray, EspAvbTransportSet,
    AURORA_7_1_4_CHANNELS, ESP_AVB_7_1_4_ENDPOINTS,
};
use aurora_realtime_audio_api::{
    MediaTimestamp, NetworkAudioBlock, NetworkAudioFormat, NetworkAudioTransport,
    NetworkClockDiscipline, NetworkStreamConfig, NetworkTransportCapabilities,
    NetworkTransportError, NetworkTransportEvent, NetworkTransportFamily,
    AURORA_NETWORK_MEDIA_RATE,
};

#[derive(Default)]
struct ProbeState {
    submissions: Vec<(u64, u64)>,
}

struct ProbeTransport {
    state: Arc<Mutex<ProbeState>>,
    prepared: bool,
    started: bool,
}

impl ProbeTransport {
    fn new(state: Arc<Mutex<ProbeState>>) -> Self {
        Self {
            state,
            prepared: false,
            started: false,
        }
    }
}

impl NetworkAudioTransport for ProbeTransport {
    fn capabilities(&self) -> NetworkTransportCapabilities {
        NetworkTransportCapabilities {
            family: NetworkTransportFamily::AvbTsn,
            max_channels: 2,
            scheduled_playout: true,
            hardware_timestamps: false,
            adaptive_rate_matching: false,
            packet_repair: false,
        }
    }

    fn prepare(&mut self, config: NetworkStreamConfig) -> Result<(), NetworkTransportError> {
        if config.format.sample_rate != AURORA_NETWORK_MEDIA_RATE
            || config.format.channels != 2
            || config.clock_discipline != NetworkClockDiscipline::PtpFollower
            || config.timing.maximum_rate_correction_ppm != 0.0
        {
            return Err(NetworkTransportError::WorkerFault);
        }
        self.prepared = true;
        self.started = false;
        Ok(())
    }

    fn start(&mut self) -> Result<(), NetworkTransportError> {
        if !self.prepared {
            return Err(NetworkTransportError::NotPrepared);
        }
        self.started = true;
        Ok(())
    }

    fn submit(&mut self, block: NetworkAudioBlock<'_>) -> Result<(), NetworkTransportError> {
        if !self.started {
            return Err(NetworkTransportError::NotPrepared);
        }
        block.validate()?;
        self.state
            .lock()
            .unwrap()
            .submissions
            .push((block.sequence, block.timestamp.frame_index));
        Ok(())
    }

    fn poll_event(&mut self) -> Option<NetworkTransportEvent> {
        None
    }

    fn stop(&mut self) -> Result<(), NetworkTransportError> {
        if !self.prepared {
            return Err(NetworkTransportError::NotPrepared);
        }
        self.started = false;
        Ok(())
    }

    fn reset(&mut self) {}
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    const BLOCK_FRAMES: usize = 48;
    let states: [Arc<Mutex<ProbeState>>; ESP_AVB_7_1_4_ENDPOINTS] =
        std::array::from_fn(|_| Arc::new(Mutex::new(ProbeState::default())));
    let transports: EspAvbTransportArray = std::array::from_fn(|index| {
        Box::new(ProbeTransport::new(Arc::clone(&states[index]))) as Box<dyn NetworkAudioTransport>
    });

    let plan = EspAvbFanoutPlan::canonical(EspAvbEndpointMedium::WirelessEsp32C6);
    let mut set = EspAvbTransportSet::new(plan, transports, BLOCK_FRAMES)?;
    set.prepare(480)?;
    set.start()?;

    let samples: Vec<f32> = (0..AURORA_7_1_4_CHANNELS * BLOCK_FRAMES)
        .map(|index| index as f32 / 1000.0)
        .collect();
    let input = NetworkAudioBlock {
        sequence: 23,
        timestamp: MediaTimestamp::new(96_000, AURORA_NETWORK_MEDIA_RATE)?,
        format: NetworkAudioFormat {
            sample_rate: AURORA_NETWORK_MEDIA_RATE,
            channels: AURORA_7_1_4_CHANNELS,
            block_frames: BLOCK_FRAMES,
        },
        samples: &samples,
    };
    set.submit(&input)?;

    for state in &states {
        let state = state.lock().unwrap();
        if state.submissions.as_slice() != [(23, 96_000)] {
            return Err("endpoint timeline diverged".into());
        }
    }
    set.stop()?;

    println!(
        "aurora-esp-avb-orchestration: PASS endpoints={} rate={} block={} clock=ptp-follower sequence=23 timestamp=96000",
        ESP_AVB_7_1_4_ENDPOINTS, AURORA_NETWORK_MEDIA_RATE, BLOCK_FRAMES
    );
    Ok(())
}
