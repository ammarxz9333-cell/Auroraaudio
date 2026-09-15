use std::env;
use std::thread;
use std::time::Duration;

use aurora_network_aoo::{AooNetworkTransport, AooRuntimeConfig, AooSinkEndpoint};
use aurora_realtime_audio_api::{
    MediaTimestamp, NetworkAudioBlock, NetworkAudioFormat, NetworkAudioTransport,
    NetworkClockDiscipline, NetworkStreamConfig, NetworkTimingPolicy,
};

const CHANNELS: usize = 12;
const SAMPLE_RATE: u32 = 48_000;
const BLOCK_FRAMES: usize = 48;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let shim = env::var("AURORA_AOO_SHIM")?;
    let sink_host = env::var("AURORA_AOO_SINK_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let sink_port = env_u16("AURORA_AOO_SINK_PORT", 9_999)?;
    let sink_id = env_i32("AURORA_AOO_SINK_ID", 1)?;
    let local_port = env_u16("AURORA_AOO_LOCAL_PORT", 9_998)?;
    let epochs = env_u64("AURORA_AOO_EPOCHS", 1)?;
    let blocks_per_epoch = env_u64("AURORA_AOO_BLOCKS_PER_EPOCH", 4_000)?;
    let gap_ms = env_u64("AURORA_AOO_GAP_MS", 0)?;

    let mut transport = AooNetworkTransport::load(AooRuntimeConfig {
        shim_path: shim.into(),
        source_id: 1,
        local_port,
        sink: Some(AooSinkEndpoint {
            host: sink_host,
            port: sink_port,
            id: sink_id,
        }),
    })?;
    let format = NetworkAudioFormat {
        sample_rate: SAMPLE_RATE,
        channels: CHANNELS,
        block_frames: BLOCK_FRAMES,
    };
    transport.prepare(NetworkStreamConfig {
        format,
        clock_discipline: NetworkClockDiscipline::AuroraMediaMaster,
        timing: NetworkTimingPolicy {
            target_latency_frames: 2_400,
            minimum_latency_frames: 480,
            maximum_latency_frames: 9_600,
            maximum_rate_correction_ppm: 0.0,
        },
    })?;

    let mut samples = vec![0.0_f32; CHANNELS * BLOCK_FRAMES];
    for epoch in 0..epochs {
        transport.start()?;
        println!("aurora-aoo-network-soak: epoch={} start", epoch + 1);
        for sequence in 0..blocks_per_epoch {
            fill_signal(&mut samples, sequence);
            transport.submit(NetworkAudioBlock {
                sequence,
                timestamp: MediaTimestamp::new(sequence * BLOCK_FRAMES as u64, SAMPLE_RATE)?,
                format,
                samples: &samples,
            })?;
            thread::sleep(Duration::from_millis(1));
        }
        transport.stop()?;
        println!("aurora-aoo-network-soak: epoch={} stop", epoch + 1);
        if epoch + 1 < epochs && gap_ms != 0 {
            thread::sleep(Duration::from_millis(gap_ms));
        }
    }

    println!(
        "aurora-aoo-network-soak: PASS epochs={epochs} blocks_per_epoch={blocks_per_epoch} sink_port={sink_port}"
    );
    Ok(())
}

fn fill_signal(samples: &mut [f32], sequence: u64) {
    for frame in 0..BLOCK_FRAMES {
        for channel in 0..CHANNELS {
            let index = frame * CHANNELS + channel;
            samples[index] = 0.02
                + (channel as f32 * 0.001)
                + ((sequence % 101) as f32 * 0.00001)
                + (frame as f32 * 0.000001);
        }
    }
}

fn env_u64(name: &str, default: u64) -> Result<u64, Box<dyn std::error::Error>> {
    match env::var(name) {
        Ok(value) => Ok(value.parse()?),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(Box::new(error)),
    }
}

fn env_u16(name: &str, default: u16) -> Result<u16, Box<dyn std::error::Error>> {
    let value = env_u64(name, u64::from(default))?;
    Ok(u16::try_from(value)?)
}

fn env_i32(name: &str, default: i32) -> Result<i32, Box<dyn std::error::Error>> {
    match env::var(name) {
        Ok(value) => Ok(value.parse()?),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(Box::new(error)),
    }
}
