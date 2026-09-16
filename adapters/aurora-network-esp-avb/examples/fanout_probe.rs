use aurora_network_esp_avb::{
    EspAvbEndpointMedium, EspAvbFanoutPlan, PreparedEspAvbFanout,
    AURORA_7_1_4_CHANNELS, ESP_AVB_7_1_4_ENDPOINTS,
};
use aurora_realtime_audio_api::{
    MediaTimestamp, NetworkAudioBlock, NetworkAudioFormat, NetworkClockDiscipline,
    AURORA_NETWORK_MEDIA_RATE,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    const BLOCK_FRAMES: usize = 48;
    let plan = EspAvbFanoutPlan::canonical(EspAvbEndpointMedium::WirelessEsp32C6);
    plan.validate()?;
    if EspAvbFanoutPlan::required_clock_discipline() != NetworkClockDiscipline::PtpFollower {
        return Err("ESP-AVB plan must use PTP follower mapping".into());
    }

    let mut fanout = PreparedEspAvbFanout::prepare(plan, BLOCK_FRAMES)?;
    let mut samples = vec![0.0_f32; AURORA_7_1_4_CHANNELS * BLOCK_FRAMES];
    for frame in 0..BLOCK_FRAMES {
        for channel in 0..AURORA_7_1_4_CHANNELS {
            samples[frame * AURORA_7_1_4_CHANNELS + channel] =
                (frame * AURORA_7_1_4_CHANNELS + channel) as f32 / 1000.0;
        }
    }

    let input = NetworkAudioBlock {
        sequence: 17,
        timestamp: MediaTimestamp::new(48_000, AURORA_NETWORK_MEDIA_RATE)?,
        format: NetworkAudioFormat {
            sample_rate: AURORA_NETWORK_MEDIA_RATE,
            channels: AURORA_7_1_4_CHANNELS,
            block_frames: BLOCK_FRAMES,
        },
        samples: &samples,
    };
    fanout.split(&input)?;

    let mut checksum = 0.0_f64;
    for endpoint_index in 0..ESP_AVB_7_1_4_ENDPOINTS {
        let block = fanout.endpoint_block(endpoint_index)?;
        if block.sequence != input.sequence || block.timestamp != input.timestamp {
            return Err("endpoint timeline diverged from Aurora media timeline".into());
        }
        if block.format.channels != 2 || block.format.sample_rate != AURORA_NETWORK_MEDIA_RATE {
            return Err("endpoint format violated pinned stereo/48 kHz contract".into());
        }
        checksum += block.samples.iter().map(|sample| f64::from(*sample)).sum::<f64>();
    }
    if !checksum.is_finite() || checksum <= 0.0 {
        return Err("fanout checksum is invalid".into());
    }

    println!(
        "aurora-esp-avb-fanout: PASS nodes={} channels={} per_stream=2 rate={} clock=ptp-follower checksum={:.6}",
        ESP_AVB_7_1_4_ENDPOINTS,
        AURORA_7_1_4_CHANNELS,
        AURORA_NETWORK_MEDIA_RATE,
        checksum
    );
    Ok(())
}
