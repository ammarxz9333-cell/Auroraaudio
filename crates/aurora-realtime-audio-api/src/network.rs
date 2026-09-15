//! Aurora-owned network-audio contracts.
//!
//! This module deliberately does not expose AOO, AVB/TSN, RTP, PTP, or any
//! other upstream type. Network I/O belongs on a dedicated worker/control
//! thread. The real-time audio callback exchanges only bounded, preallocated
//! PCM blocks with that worker through Aurora-owned queues.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Canonical Aurora media rate for immersive PCM transport.
pub const AURORA_NETWORK_MEDIA_RATE: u32 = 48_000;

/// Network transport family selected behind Aurora's stable boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkTransportFamily {
    /// IEEE 1722 / AVB / TSN style transport, normally disciplined by gPTP.
    AvbTsn,
    /// Peer-to-peer UDP transport such as an AOO-backed adapter.
    PeerUdp,
    /// Deterministic in-process transport used only by simulation and tests.
    Simulation,
}

/// Ownership model for the media clock presented to a transport adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkClockDiscipline {
    /// Aurora owns the media timeline; the backend follows scheduled timestamps.
    AuroraMediaMaster,
    /// A gPTP/PTP grandmaster disciplines the network domain and Aurora maps its
    /// media timeline into that domain during setup/worker processing.
    PtpFollower,
    /// A peer clock is tracked through bounded asynchronous rate correction.
    AdaptiveRateFollower,
}

/// Setup-time capabilities reported by a network transport adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkTransportCapabilities {
    /// Transport family.
    pub family: NetworkTransportFamily,
    /// Maximum PCM channels supported by one logical Aurora stream.
    pub max_channels: usize,
    /// Whether packets/blocks can carry an explicit future playout timestamp.
    pub scheduled_playout: bool,
    /// Whether the backend exposes hardware-assisted network timestamps.
    pub hardware_timestamps: bool,
    /// Whether the backend can compensate long-term clock-rate mismatch.
    pub adaptive_rate_matching: bool,
    /// Whether the backend provides packet repair/retransmission or redundancy.
    pub packet_repair: bool,
}

/// PCM format crossing the Aurora-to-network worker boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkAudioFormat {
    /// Sample rate in hertz.
    pub sample_rate: u32,
    /// Interleaved channel count.
    pub channels: usize,
    /// Frames submitted per Aurora transport block.
    pub block_frames: usize,
}

impl NetworkAudioFormat {
    /// Validates the bounded format before a backend is prepared.
    pub fn validate(
        self,
        capabilities: NetworkTransportCapabilities,
    ) -> Result<(), NetworkTransportError> {
        if self.sample_rate == 0 || self.channels == 0 || self.block_frames == 0 {
            return Err(NetworkTransportError::InvalidFormat);
        }
        if self.channels > capabilities.max_channels {
            return Err(NetworkTransportError::UnsupportedChannelCount);
        }
        Ok(())
    }

    /// Number of interleaved samples in one complete block.
    pub fn samples_per_block(self) -> Option<usize> {
        self.channels.checked_mul(self.block_frames)
    }
}

/// Bounded latency and drift policy shared by wired and wireless adapters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NetworkTimingPolicy {
    /// Normal scheduled playout fill in frames.
    pub target_latency_frames: u32,
    /// Lower bound accepted by the worker-side controller.
    pub minimum_latency_frames: u32,
    /// Upper bound accepted by the worker-side controller.
    pub maximum_latency_frames: u32,
    /// Maximum absolute adaptive clock correction in parts per million.
    pub maximum_rate_correction_ppm: f64,
}

impl NetworkTimingPolicy {
    /// Validates policy ordering and finite clock limits.
    pub fn validate(self) -> Result<(), NetworkTransportError> {
        if self.minimum_latency_frames > self.target_latency_frames
            || self.target_latency_frames > self.maximum_latency_frames
        {
            return Err(NetworkTransportError::InvalidTimingPolicy);
        }
        if !self.maximum_rate_correction_ppm.is_finite() || self.maximum_rate_correction_ppm < 0.0 {
            return Err(NetworkTransportError::InvalidTimingPolicy);
        }
        Ok(())
    }
}

/// Aurora media timestamp represented as an absolute frame index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MediaTimestamp {
    /// Absolute frame number in the declared sample-rate domain.
    pub frame_index: u64,
    /// Sample rate defining the frame domain.
    pub sample_rate: u32,
}

impl MediaTimestamp {
    /// Creates a timestamp, rejecting an invalid zero-rate domain.
    pub fn new(frame_index: u64, sample_rate: u32) -> Result<Self, NetworkTransportError> {
        if sample_rate == 0 {
            return Err(NetworkTransportError::InvalidTimestamp);
        }
        Ok(Self {
            frame_index,
            sample_rate,
        })
    }

    /// Advances by a bounded number of frames without wrapping the timeline.
    pub fn checked_advance(self, frames: usize) -> Result<Self, NetworkTransportError> {
        let frames = u64::try_from(frames).map_err(|_| NetworkTransportError::TimestampOverflow)?;
        let frame_index = self
            .frame_index
            .checked_add(frames)
            .ok_or(NetworkTransportError::TimestampOverflow)?;
        Ok(Self {
            frame_index,
            sample_rate: self.sample_rate,
        })
    }
}

/// Borrowed worker-side PCM block with sequence and scheduled media time.
#[derive(Debug)]
pub struct NetworkAudioBlock<'a> {
    /// Monotonic stream-local sequence number.
    pub sequence: u64,
    /// Timestamp of the first frame in `samples`.
    pub timestamp: MediaTimestamp,
    /// Declared block format.
    pub format: NetworkAudioFormat,
    /// Interleaved f32 PCM samples.
    pub samples: &'a [f32],
}

impl NetworkAudioBlock<'_> {
    /// Validates timestamp domain and exact interleaved shape.
    pub fn validate(&self) -> Result<(), NetworkTransportError> {
        if self.timestamp.sample_rate != self.format.sample_rate {
            return Err(NetworkTransportError::TimestampRateMismatch);
        }
        let required = self
            .format
            .samples_per_block()
            .ok_or(NetworkTransportError::InvalidFormat)?;
        if self.samples.len() != required {
            return Err(NetworkTransportError::BufferShape);
        }
        Ok(())
    }
}

/// Worker-side stream preparation shared across transport implementations.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NetworkStreamConfig {
    /// PCM format at the worker boundary.
    pub format: NetworkAudioFormat,
    /// Clock ownership/discipline policy.
    pub clock_discipline: NetworkClockDiscipline,
    /// Latency and rate-control bounds.
    pub timing: NetworkTimingPolicy,
}

/// Fixed transport state/fault events safe to forward to telemetry queues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkTransportEvent {
    /// Backend is prepared but not sending.
    Prepared,
    /// Stream entered active transport.
    Started,
    /// One or more packets were repaired/retransmitted.
    PacketRepair,
    /// Receive/playout worker observed an underflow.
    Underflow,
    /// Send queue observed bounded overflow.
    Overflow,
    /// Clock correction reached the configured safety bound.
    ClockCorrectionLimit,
    /// Backend stopped.
    Stopped,
}

/// Fail-closed errors used by transport preparation and worker processing.
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum NetworkTransportError {
    /// Format contains a zero or overflowing dimension.
    #[error("invalid network audio format")]
    InvalidFormat,
    /// Channel count exceeds backend capability.
    #[error("network transport channel count is unsupported")]
    UnsupportedChannelCount,
    /// Latency/drift policy is unordered or non-finite.
    #[error("invalid network timing policy")]
    InvalidTimingPolicy,
    /// Timestamp uses a zero sample-rate domain.
    #[error("invalid network media timestamp")]
    InvalidTimestamp,
    /// Timestamp arithmetic exceeded the representable media timeline.
    #[error("network media timestamp overflow")]
    TimestampOverflow,
    /// PCM block timestamp and format use different clock domains.
    #[error("network block timestamp rate does not match PCM rate")]
    TimestampRateMismatch,
    /// PCM sample count does not match channels multiplied by block frames.
    #[error("network PCM block shape is invalid")]
    BufferShape,
    /// Adapter is not prepared for worker operation.
    #[error("network transport is not prepared")]
    NotPrepared,
    /// Adapter is already active.
    #[error("network transport is already started")]
    AlreadyStarted,
    /// Backend-specific worker failed without exposing heap-owned diagnostics to
    /// the real-time side.
    #[error("network transport worker failed")]
    WorkerFault,
}

/// Worker-thread network transport contract.
///
/// Implementations may perform network and operating-system I/O here. These
/// methods MUST NOT be called from Aurora's real-time audio callback. The audio
/// callback must communicate with the worker through bounded preallocated queues.
pub trait NetworkAudioTransport: Send {
    /// Reports static/backend setup capabilities.
    fn capabilities(&self) -> NetworkTransportCapabilities;

    /// Validates and prepares all worker-side resources before activation.
    fn prepare(&mut self, config: NetworkStreamConfig) -> Result<(), NetworkTransportError>;

    /// Starts worker-side transport.
    fn start(&mut self) -> Result<(), NetworkTransportError>;

    /// Submits one complete timestamped block from a bounded non-callback queue.
    fn submit(&mut self, block: NetworkAudioBlock<'_>) -> Result<(), NetworkTransportError>;

    /// Returns one pending fixed-size event without formatting/logging.
    fn poll_event(&mut self) -> Option<NetworkTransportEvent>;

    /// Stops worker-side transport and keeps prepared capacity reusable.
    fn stop(&mut self) -> Result<(), NetworkTransportError>;

    /// Clears sequence/buffer history without changing prepared capacities.
    fn reset(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps() -> NetworkTransportCapabilities {
        NetworkTransportCapabilities {
            family: NetworkTransportFamily::Simulation,
            max_channels: 32,
            scheduled_playout: true,
            hardware_timestamps: false,
            adaptive_rate_matching: true,
            packet_repair: true,
        }
    }

    #[test]
    fn timing_policy_fails_closed_on_invalid_bounds() {
        let valid = NetworkTimingPolicy {
            target_latency_frames: 480,
            minimum_latency_frames: 240,
            maximum_latency_frames: 960,
            maximum_rate_correction_ppm: 250.0,
        };
        assert_eq!(valid.validate(), Ok(()));

        assert_eq!(
            NetworkTimingPolicy {
                minimum_latency_frames: 600,
                ..valid
            }
            .validate(),
            Err(NetworkTransportError::InvalidTimingPolicy)
        );
        assert_eq!(
            NetworkTimingPolicy {
                maximum_rate_correction_ppm: f64::NAN,
                ..valid
            }
            .validate(),
            Err(NetworkTransportError::InvalidTimingPolicy)
        );
    }

    #[test]
    fn block_requires_exact_shape_and_timestamp_domain() {
        let format = NetworkAudioFormat {
            sample_rate: AURORA_NETWORK_MEDIA_RATE,
            channels: 12,
            block_frames: 48,
        };
        assert_eq!(format.validate(caps()), Ok(()));
        let samples = vec![0.0; 12 * 48];
        let block = NetworkAudioBlock {
            sequence: 7,
            timestamp: MediaTimestamp::new(12_000, 48_000).unwrap(),
            format,
            samples: &samples,
        };
        assert_eq!(block.validate(), Ok(()));

        let wrong_rate = NetworkAudioBlock {
            timestamp: MediaTimestamp::new(12_000, 44_100).unwrap(),
            ..block
        };
        assert_eq!(
            wrong_rate.validate(),
            Err(NetworkTransportError::TimestampRateMismatch)
        );
    }

    #[test]
    fn timestamp_advance_is_checked() {
        let timestamp = MediaTimestamp::new(u64::MAX - 1, 48_000).unwrap();
        assert_eq!(
            timestamp.checked_advance(2),
            Err(NetworkTransportError::TimestampOverflow)
        );
    }
}
