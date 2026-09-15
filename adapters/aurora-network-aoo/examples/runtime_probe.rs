use std::env;

use aurora_network_aoo::{AooNetworkTransport, AooRuntimeConfig};
use aurora_realtime_audio_api::{
    MediaTimestamp, NetworkAudioBlock, NetworkAudioFormat, NetworkAudioTransport,
    NetworkClockDiscipline, NetworkStreamConfig, NetworkTimingPolicy, NetworkTransportError,
    NetworkTransportEvent,
};

const CHANNELS: usize = 12;
const SAMPLE_RATE: u32 = 48_000;
const BLOCK_FRAMES: usize = 48;
const BLOCKS_PER_EPOCH: u64 = 2_048;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let shim = env::var("AURORA_AOO_SHIM")?;
    let mut transport = AooNetworkTransport::load(AooRuntimeConfig::local_only(shim, 1))?;
    let format = NetworkAudioFormat {
        sample_rate: SAMPLE_RATE,
        channels: CHANNELS,
        block_frames: BLOCK_FRAMES,
    };
    transport.prepare(NetworkStreamConfig {
        format,
        clock_discipline: NetworkClockDiscipline::AuroraMediaMaster,
        timing: NetworkTimingPolicy {
            target_latency_frames: 480,
            minimum_latency_frames: 240,
            maximum_latency_frames: 960,
            maximum_rate_correction_ppm: 0.0,
        },
    })?;
    assert_eq!(transport.poll_event(), Some(NetworkTransportEvent::Prepared));

    let mut samples = vec![0.0_f32; CHANNELS * BLOCK_FRAMES];
    run_epoch(&mut transport, format, &mut samples)?;
    run_epoch(&mut transport, format, &mut samples)?;

    println!(
        "aurora-aoo-runtime-adapter: PASS channels={CHANNELS} rate={SAMPLE_RATE} block={BLOCK_FRAMES} epochs=2 blocks_per_epoch={BLOCKS_PER_EPOCH} dynamic-src=disabled timeline=fail-closed"
    );
    Ok(())
}

fn run_epoch(
    transport: &mut AooNetworkTransport,
    format: NetworkAudioFormat,
    samples: &mut [f32],
) -> Result<(), NetworkTransportError> {
    transport.start()?;
    assert_eq!(transport.poll_event(), Some(NetworkTransportEvent::Started));

    for sequence in 0..BLOCKS_PER_EPOCH {
        for frame in 0..BLOCK_FRAMES {
            for channel in 0..CHANNELS {
                let index = frame * CHANNELS + channel;
                samples[index] = ((channel + 1) as f32 * 0.001)
                    + ((sequence % 17) as f32 * 0.00001)
                    + (frame as f32 * 0.000001);
            }
        }
        let timestamp = MediaTimestamp::new(sequence * BLOCK_FRAMES as u64, SAMPLE_RATE)?;
        transport.submit(NetworkAudioBlock {
            sequence,
            timestamp,
            format,
            samples,
        })?;

        if sequence == 0 {
            let bad = transport.submit(NetworkAudioBlock {
                sequence: 2,
                timestamp: timestamp.checked_advance(BLOCK_FRAMES)?,
                format,
                samples,
            });
            assert_eq!(bad, Err(NetworkTransportError::WorkerFault));
        }
    }

    transport.stop()?;
    assert_eq!(transport.poll_event(), Some(NetworkTransportEvent::Stopped));
    Ok(())
}
