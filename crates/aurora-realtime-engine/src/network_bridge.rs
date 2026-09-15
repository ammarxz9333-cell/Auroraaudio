use std::sync::Arc;

use aurora_realtime_audio_api::{
    MediaTimestamp, NetworkAudioBlock, NetworkAudioFormat, NetworkAudioTransport,
    NetworkTransportError,
};
use crossbeam_queue::ArrayQueue;
use thiserror::Error;

use crate::{TransportKind, TransportPrototype};

/// Metadata published alongside one preallocated PCM block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkBlockMetadata {
    /// Monotonic Aurora-generated sequence number.
    pub sequence: u64,
    /// Media timestamp of the first PCM frame.
    pub timestamp: MediaTimestamp,
}

/// Producer-side failures. Values are fixed-size and callback-safe to return.
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum NetworkBridgePushError {
    /// Submitted PCM does not match the prepared interleaved block shape.
    #[error("network bridge PCM block shape mismatch")]
    BufferShape,
    /// Timestamp and prepared PCM format use different sample-rate domains.
    #[error("network bridge timestamp rate mismatch")]
    TimestampRateMismatch,
    /// Timestamp does not immediately follow the previous accepted block.
    #[error("network bridge timestamp discontinuity")]
    TimestampDiscontinuity,
    /// The media frame timeline would overflow.
    #[error("network bridge timestamp overflow")]
    TimestampOverflow,
    /// The bounded callback-to-worker bridge is full.
    #[error("network bridge overflow")]
    Overflow,
}

/// Consumer-side failures detected before invoking a network backend.
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum NetworkBridgePopError {
    /// Worker scratch does not match the prepared interleaved block shape.
    #[error("network worker scratch shape mismatch")]
    BufferShape,
    /// Metadata and PCM queues lost lockstep, which is an internal invariant failure.
    #[error("network bridge metadata/PCM desynchronization")]
    Desynchronized,
}

/// Callback-owned producer for post-DSP PCM fan-out.
///
/// All storage is allocated by [`create_network_transport_bridge`] before this
/// object enters the audio callback. `try_push` performs no heap allocation,
/// locking, logging, formatting, socket I/O, or external-backend calls.
pub struct NetworkBlockProducer {
    pcm: TransportPrototype,
    metadata: Arc<ArrayQueue<NetworkBlockMetadata>>,
    format: NetworkAudioFormat,
    capacity_blocks: usize,
    next_sequence: u64,
    expected_timestamp: Option<MediaTimestamp>,
}

/// Worker-owned consumer paired with [`NetworkBlockProducer`].
#[derive(Clone)]
pub struct NetworkBlockConsumer {
    pcm: TransportPrototype,
    metadata: Arc<ArrayQueue<NetworkBlockMetadata>>,
    format: NetworkAudioFormat,
}

/// Creates a preallocated SPSC bridge between the realtime callback and one
/// network transport worker.
///
/// The bridge uses the realtime engine's fixed-block pool for PCM and a bounded
/// metadata queue with the same block capacity. A capacity of at least two is
/// required so producer and consumer can run independently without an
/// unbounded queue.
pub fn create_network_transport_bridge(
    format: NetworkAudioFormat,
    capacity_blocks: usize,
) -> Option<(NetworkBlockProducer, NetworkBlockConsumer)> {
    if format.sample_rate == 0
        || format.channels == 0
        || format.block_frames == 0
        || capacity_blocks < 2
    {
        return None;
    }
    let block_samples = format.samples_per_block()?;
    let pcm = TransportPrototype::new(
        TransportKind::FixedBlockPool,
        block_samples,
        capacity_blocks,
    )?;
    let metadata = Arc::new(ArrayQueue::new(capacity_blocks));
    Some((
        NetworkBlockProducer {
            pcm: pcm.clone(),
            metadata: Arc::clone(&metadata),
            format,
            capacity_blocks,
            next_sequence: 0,
            expected_timestamp: None,
        },
        NetworkBlockConsumer {
            pcm,
            metadata,
            format,
        },
    ))
}

impl NetworkBlockProducer {
    /// Pushes one post-DSP PCM block into the bounded worker bridge.
    pub fn try_push(
        &mut self,
        timestamp: MediaTimestamp,
        samples: &[f32],
    ) -> Result<NetworkBlockMetadata, NetworkBridgePushError> {
        if samples.len() != self.format.samples_per_block().unwrap_or(usize::MAX) {
            return Err(NetworkBridgePushError::BufferShape);
        }
        if timestamp.sample_rate != self.format.sample_rate {
            return Err(NetworkBridgePushError::TimestampRateMismatch);
        }
        if self
            .expected_timestamp
            .is_some_and(|expected| expected != timestamp)
        {
            return Err(NetworkBridgePushError::TimestampDiscontinuity);
        }
        let next_timestamp = timestamp
            .checked_advance(self.format.block_frames)
            .map_err(|_| NetworkBridgePushError::TimestampOverflow)?;
        if self.metadata.len() >= self.capacity_blocks {
            return Err(NetworkBridgePushError::Overflow);
        }
        if !self.pcm.try_push_block(samples) {
            return Err(NetworkBridgePushError::Overflow);
        }
        let metadata = NetworkBlockMetadata {
            sequence: self.next_sequence,
            timestamp,
        };
        if self.metadata.push(metadata).is_err() {
            // With one producer and matching capacities this cannot occur after
            // the preflight check and successful PCM publication. Do not hide a
            // violated invariant as a recoverable discontinuity.
            return Err(NetworkBridgePushError::Overflow);
        }
        self.next_sequence = self.next_sequence.wrapping_add(1);
        self.expected_timestamp = Some(next_timestamp);
        Ok(metadata)
    }

    /// Returns the number of complete blocks currently queued for the worker.
    pub fn queued_blocks(&self) -> usize {
        self.metadata.len()
    }

    /// Resets sequence/timestamp ownership after the worker drained all blocks.
    ///
    /// Returns false rather than discarding queued audio.
    pub fn reset_timeline_if_empty(&mut self, next_sequence: u64) -> bool {
        if !self.metadata.is_empty() || self.pcm.queued_samples() != 0 {
            return false;
        }
        self.next_sequence = next_sequence;
        self.expected_timestamp = None;
        true
    }
}

impl NetworkBlockConsumer {
    /// Pops one complete block into caller-owned worker scratch.
    pub fn try_pop(
        &self,
        output: &mut [f32],
    ) -> Result<Option<NetworkBlockMetadata>, NetworkBridgePopError> {
        if output.len() != self.format.samples_per_block().unwrap_or(usize::MAX) {
            return Err(NetworkBridgePopError::BufferShape);
        }
        let Some(metadata) = self.metadata.pop() else {
            return Ok(None);
        };
        if !self.pcm.try_pop_block(output) {
            return Err(NetworkBridgePopError::Desynchronized);
        }
        Ok(Some(metadata))
    }

    /// Pops at most one block and submits it to a prepared worker-thread backend.
    ///
    /// Network/backend I/O is performed only here on the worker side, never by
    /// the callback producer.
    pub fn try_submit_one<T: NetworkAudioTransport>(
        &self,
        transport: &mut T,
        scratch: &mut [f32],
    ) -> Result<bool, NetworkTransportError> {
        let metadata = self
            .try_pop(scratch)
            .map_err(|_| NetworkTransportError::WorkerFault)?;
        let Some(metadata) = metadata else {
            return Ok(false);
        };
        transport.submit(NetworkAudioBlock {
            sequence: metadata.sequence,
            timestamp: metadata.timestamp,
            format: self.format,
            samples: scratch,
        })?;
        Ok(true)
    }

    /// Returns the prepared PCM format for worker scratch allocation.
    pub fn format(&self) -> NetworkAudioFormat {
        self.format
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_realtime_audio_api::{
        NetworkAudioTransport, NetworkClockDiscipline, NetworkStreamConfig, NetworkTimingPolicy,
        AURORA_NETWORK_MEDIA_RATE,
    };
    use aurora_test_alloc::count_allocations;

    fn format() -> NetworkAudioFormat {
        NetworkAudioFormat {
            sample_rate: AURORA_NETWORK_MEDIA_RATE,
            channels: 12,
            block_frames: 48,
        }
    }

    #[test]
    fn callback_to_worker_bridge_preserves_block_and_timeline() {
        let format = format();
        let (mut producer, consumer) = create_network_transport_bridge(format, 3).unwrap();
        let first = (0..12 * 48)
            .map(|value| value as f32 / 2048.0)
            .collect::<Vec<_>>();
        let second = first.iter().map(|sample| -*sample).collect::<Vec<_>>();
        let timestamp = MediaTimestamp::new(96_000, 48_000).unwrap();
        let first_meta = producer.try_push(timestamp, &first).unwrap();
        let second_meta = producer
            .try_push(timestamp.checked_advance(48).unwrap(), &second)
            .unwrap();
        assert_eq!(first_meta.sequence, 0);
        assert_eq!(second_meta.sequence, 1);

        let mut scratch = vec![0.0; 12 * 48];
        assert_eq!(consumer.try_pop(&mut scratch).unwrap(), Some(first_meta));
        assert_eq!(scratch, first);
        assert_eq!(consumer.try_pop(&mut scratch).unwrap(), Some(second_meta));
        assert_eq!(scratch, second);
        assert_eq!(consumer.try_pop(&mut scratch).unwrap(), None);
    }

    #[test]
    fn callback_push_is_allocation_free_after_prepare() {
        let format = format();
        let (mut producer, consumer) = create_network_transport_bridge(format, 2).unwrap();
        let samples = vec![0.25; 12 * 48];
        let timestamp = MediaTimestamp::new(0, 48_000).unwrap();
        let allocations = count_allocations(|| {
            producer.try_push(timestamp, &samples).unwrap();
        });
        assert_eq!(allocations, 0);

        let mut scratch = vec![0.0; 12 * 48];
        let allocations = count_allocations(|| {
            consumer.try_pop(&mut scratch).unwrap().unwrap();
        });
        assert_eq!(allocations, 0);
    }

    #[test]
    fn discontinuity_and_overflow_fail_closed() {
        let format = format();
        let (mut producer, _consumer) = create_network_transport_bridge(format, 2).unwrap();
        let samples = vec![0.0; 12 * 48];
        producer
            .try_push(MediaTimestamp::new(0, 48_000).unwrap(), &samples)
            .unwrap();
        assert_eq!(
            producer.try_push(MediaTimestamp::new(96, 48_000).unwrap(), &samples),
            Err(NetworkBridgePushError::TimestampDiscontinuity)
        );
        producer
            .try_push(MediaTimestamp::new(48, 48_000).unwrap(), &samples)
            .unwrap();
        assert_eq!(
            producer.try_push(MediaTimestamp::new(96, 48_000).unwrap(), &samples),
            Err(NetworkBridgePushError::Overflow)
        );
    }

    struct RecordingTransport {
        prepared: bool,
        last: Option<NetworkBlockMetadata>,
    }

    impl NetworkAudioTransport for RecordingTransport {
        fn capabilities(&self) -> aurora_realtime_audio_api::NetworkTransportCapabilities {
            aurora_realtime_audio_api::NetworkTransportCapabilities {
                family: aurora_realtime_audio_api::NetworkTransportFamily::Simulation,
                max_channels: 12,
                scheduled_playout: true,
                hardware_timestamps: false,
                adaptive_rate_matching: false,
                packet_repair: false,
            }
        }

        fn prepare(&mut self, _config: NetworkStreamConfig) -> Result<(), NetworkTransportError> {
            self.prepared = true;
            Ok(())
        }

        fn start(&mut self) -> Result<(), NetworkTransportError> {
            if !self.prepared {
                return Err(NetworkTransportError::NotPrepared);
            }
            Ok(())
        }

        fn submit(&mut self, block: NetworkAudioBlock<'_>) -> Result<(), NetworkTransportError> {
            if !self.prepared {
                return Err(NetworkTransportError::NotPrepared);
            }
            block.validate()?;
            self.last = Some(NetworkBlockMetadata {
                sequence: block.sequence,
                timestamp: block.timestamp,
            });
            Ok(())
        }

        fn poll_event(&mut self) -> Option<aurora_realtime_audio_api::NetworkTransportEvent> {
            None
        }

        fn stop(&mut self) -> Result<(), NetworkTransportError> {
            Ok(())
        }

        fn reset(&mut self) {
            self.last = None;
        }
    }

    #[test]
    fn worker_pump_submits_aurora_owned_block_to_backend_contract() {
        let format = format();
        let (mut producer, consumer) = create_network_transport_bridge(format, 2).unwrap();
        let samples = vec![0.125; 12 * 48];
        let timestamp = MediaTimestamp::new(48_000, 48_000).unwrap();
        producer.try_push(timestamp, &samples).unwrap();

        let mut backend = RecordingTransport {
            prepared: false,
            last: None,
        };
        backend
            .prepare(NetworkStreamConfig {
                format,
                clock_discipline: NetworkClockDiscipline::AuroraMediaMaster,
                timing: NetworkTimingPolicy {
                    target_latency_frames: 480,
                    minimum_latency_frames: 240,
                    maximum_latency_frames: 960,
                    maximum_rate_correction_ppm: 0.0,
                },
            })
            .unwrap();
        backend.start().unwrap();
        let mut scratch = vec![0.0; 12 * 48];
        assert!(consumer.try_submit_one(&mut backend, &mut scratch).unwrap());
        assert_eq!(
            backend.last,
            Some(NetworkBlockMetadata {
                sequence: 0,
                timestamp,
            })
        );
    }
}
