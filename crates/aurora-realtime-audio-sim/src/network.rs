//! Deterministic network-audio transport used to validate Aurora's transport contract.

use std::collections::VecDeque;

use aurora_realtime_audio_api::{
    MediaTimestamp, NetworkAudioBlock, NetworkAudioFormat, NetworkAudioTransport,
    NetworkClockDiscipline, NetworkStreamConfig, NetworkTimingPolicy, NetworkTransportCapabilities,
    NetworkTransportError, NetworkTransportEvent, NetworkTransportFamily,
};

/// Metadata retained for one accepted simulated network block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimulatedNetworkBlock {
    /// Monotonic stream-local sequence number.
    pub sequence: u64,
    /// Timestamp of the first frame.
    pub timestamp: MediaTimestamp,
    /// Deterministic checksum of the submitted f32 payload.
    pub payload_checksum: u64,
}

/// Deterministic bounded implementation of Aurora's network transport contract.
///
/// It performs no socket I/O. Its purpose is to prove common lifecycle,
/// timestamp, buffering and fault semantics before an AOO or GenAVB adapter is
/// allowed to claim equivalent behavior.
#[derive(Debug)]
pub struct SimNetworkTransport {
    capabilities: NetworkTransportCapabilities,
    capacity_blocks: usize,
    config: Option<NetworkStreamConfig>,
    started: bool,
    queue: VecDeque<SimulatedNetworkBlock>,
    events: VecDeque<NetworkTransportEvent>,
}

impl SimNetworkTransport {
    /// Creates a bounded simulator with no prepared resources yet.
    pub fn new(max_channels: usize, capacity_blocks: usize) -> Self {
        Self {
            capabilities: NetworkTransportCapabilities {
                family: NetworkTransportFamily::Simulation,
                max_channels,
                scheduled_playout: true,
                hardware_timestamps: false,
                adaptive_rate_matching: true,
                packet_repair: true,
            },
            capacity_blocks,
            config: None,
            started: false,
            queue: VecDeque::with_capacity(capacity_blocks),
            events: VecDeque::with_capacity(capacity_blocks.saturating_add(4)),
        }
    }

    /// Returns the number of accepted blocks waiting for deterministic drain.
    pub fn queued_blocks(&self) -> usize {
        self.queue.len()
    }

    /// Drains one accepted block as if the network worker completed it.
    pub fn drain_one(&mut self) -> Option<SimulatedNetworkBlock> {
        self.queue.pop_front()
    }

    /// Returns the active prepared configuration, if any.
    pub fn prepared_config(&self) -> Option<NetworkStreamConfig> {
        self.config
    }
}

impl NetworkAudioTransport for SimNetworkTransport {
    fn capabilities(&self) -> NetworkTransportCapabilities {
        self.capabilities
    }

    fn prepare(&mut self, config: NetworkStreamConfig) -> Result<(), NetworkTransportError> {
        if self.capacity_blocks == 0 {
            return Err(NetworkTransportError::InvalidTimingPolicy);
        }
        config.format.validate(self.capabilities)?;
        config.timing.validate()?;
        self.queue.clear();
        self.events.clear();
        self.started = false;
        self.config = Some(config);
        self.events.push_back(NetworkTransportEvent::Prepared);
        Ok(())
    }

    fn start(&mut self) -> Result<(), NetworkTransportError> {
        if self.config.is_none() {
            return Err(NetworkTransportError::NotPrepared);
        }
        if self.started {
            return Err(NetworkTransportError::AlreadyStarted);
        }
        self.started = true;
        self.events.push_back(NetworkTransportEvent::Started);
        Ok(())
    }

    fn submit(&mut self, block: NetworkAudioBlock<'_>) -> Result<(), NetworkTransportError> {
        let config = self.config.ok_or(NetworkTransportError::NotPrepared)?;
        if !self.started {
            return Err(NetworkTransportError::NotPrepared);
        }
        if block.format != config.format {
            return Err(NetworkTransportError::InvalidFormat);
        }
        block.validate()?;
        if self.queue.len() == self.capacity_blocks {
            self.events.push_back(NetworkTransportEvent::Overflow);
            return Err(NetworkTransportError::WorkerFault);
        }
        self.queue.push_back(SimulatedNetworkBlock {
            sequence: block.sequence,
            timestamp: block.timestamp,
            payload_checksum: payload_checksum(block.samples),
        });
        Ok(())
    }

    fn poll_event(&mut self) -> Option<NetworkTransportEvent> {
        self.events.pop_front()
    }

    fn stop(&mut self) -> Result<(), NetworkTransportError> {
        if self.config.is_none() {
            return Err(NetworkTransportError::NotPrepared);
        }
        self.started = false;
        self.events.push_back(NetworkTransportEvent::Stopped);
        Ok(())
    }

    fn reset(&mut self) {
        self.queue.clear();
        self.events.clear();
        self.started = false;
    }
}

fn payload_checksum(samples: &[f32]) -> u64 {
    samples.iter().fold(0xcbf29ce484222325_u64, |state, sample| {
        state
            .wrapping_mul(0x100000001b3)
            .wrapping_add(u64::from(sample.to_bits()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_realtime_audio_api::{NetworkAudioTransport, AURORA_NETWORK_MEDIA_RATE};

    fn stream_config() -> NetworkStreamConfig {
        NetworkStreamConfig {
            format: NetworkAudioFormat {
                sample_rate: AURORA_NETWORK_MEDIA_RATE,
                channels: 12,
                block_frames: 48,
            },
            clock_discipline: NetworkClockDiscipline::AuroraMediaMaster,
            timing: NetworkTimingPolicy {
                target_latency_frames: 480,
                minimum_latency_frames: 240,
                maximum_latency_frames: 960,
                maximum_rate_correction_ppm: 250.0,
            },
        }
    }

    #[test]
    fn simulated_transport_preserves_timestamp_and_payload_identity() {
        let mut transport = SimNetworkTransport::new(32, 2);
        let config = stream_config();
        transport.prepare(config).unwrap();
        transport.start().unwrap();
        let samples = (0..12 * 48)
            .map(|sample| sample as f32 / 1024.0)
            .collect::<Vec<_>>();
        let timestamp = MediaTimestamp::new(96_000, 48_000).unwrap();
        transport
            .submit(NetworkAudioBlock {
                sequence: 41,
                timestamp,
                format: config.format,
                samples: &samples,
            })
            .unwrap();

        let accepted = transport.drain_one().unwrap();
        assert_eq!(accepted.sequence, 41);
        assert_eq!(accepted.timestamp, timestamp);
        assert_eq!(accepted.payload_checksum, payload_checksum(&samples));
    }

    #[test]
    fn simulated_transport_is_bounded_and_reports_overflow() {
        let mut transport = SimNetworkTransport::new(12, 1);
        let config = stream_config();
        transport.prepare(config).unwrap();
        transport.start().unwrap();
        let samples = vec![0.0; 12 * 48];
        let first = NetworkAudioBlock {
            sequence: 0,
            timestamp: MediaTimestamp::new(0, 48_000).unwrap(),
            format: config.format,
            samples: &samples,
        };
        transport.submit(first).unwrap();
        let second = NetworkAudioBlock {
            sequence: 1,
            timestamp: MediaTimestamp::new(48, 48_000).unwrap(),
            format: config.format,
            samples: &samples,
        };
        assert_eq!(
            transport.submit(second),
            Err(NetworkTransportError::WorkerFault)
        );
        assert_eq!(transport.queued_blocks(), 1);
        assert_eq!(transport.poll_event(), Some(NetworkTransportEvent::Prepared));
        assert_eq!(transport.poll_event(), Some(NetworkTransportEvent::Started));
        assert_eq!(transport.poll_event(), Some(NetworkTransportEvent::Overflow));
    }

    #[test]
    fn simulated_transport_rejects_format_drift_after_prepare() {
        let mut transport = SimNetworkTransport::new(32, 2);
        let config = stream_config();
        transport.prepare(config).unwrap();
        transport.start().unwrap();
        let samples = vec![0.0; 2 * 48];
        let wrong_format = NetworkAudioFormat {
            sample_rate: 48_000,
            channels: 2,
            block_frames: 48,
        };
        assert_eq!(
            transport.submit(NetworkAudioBlock {
                sequence: 0,
                timestamp: MediaTimestamp::new(0, 48_000).unwrap(),
                format: wrong_format,
                samples: &samples,
            }),
            Err(NetworkTransportError::InvalidFormat)
        );
    }
}
