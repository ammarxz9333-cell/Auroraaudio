//! ESP-AVB embedded endpoint planning and worker-side stereo fanout.
//!
//! This crate does not pretend to be an ESP-IDF network driver. The pinned
//! `esp_avb` component runs on ESP32-P4/C6 endpoints and currently exposes at
//! most two PCM channels per stream. Aurora therefore needs an explicit,
//! deterministic control-plane mapping from one canonical 7.1.4 PCM bus to six
//! stereo endpoint streams before any physical sender/firmware can be selected.
//!
//! Fanout happens on the network worker side after Aurora rendering/system DSP.
//! It preserves the original sequence and media timestamp for every endpoint so
//! a later AVB sender can map that timestamp into the endpoint PTP/gPTP domain.

use std::fmt;

use aurora_realtime_audio_api::{
    MediaTimestamp, NetworkAudioBlock, NetworkAudioFormat, NetworkClockDiscipline,
    NetworkTransportCapabilities, NetworkTransportFamily, AURORA_NETWORK_MEDIA_RATE,
};

/// Canonical Aurora 7.1.4 channel count.
pub const AURORA_7_1_4_CHANNELS: usize = 12;
/// Pinned ESP-AVB maximum channels carried by one stream.
pub const ESP_AVB_CHANNELS_PER_STREAM: usize = 2;
/// Number of stereo endpoint streams required for canonical 7.1.4.
pub const ESP_AVB_7_1_4_ENDPOINTS: usize = 6;
/// Canonical Aurora 7.1.4 role order used by the fanout plan.
pub const AURORA_7_1_4_ROLES: [&str; AURORA_7_1_4_CHANNELS] = [
    "FL", "FR", "FC", "LFE", "SL", "SR", "SBL", "SBR", "TFL", "TFR", "TRL", "TRR",
];

/// Physical-medium family represented by the pinned ESP endpoint projects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EspAvbEndpointMedium {
    /// ESP32-P4 wired EMAC endpoint with hardware PTP timestamp support upstream.
    WiredEsp32P4,
    /// ESP32-C6 Wi-Fi endpoint with software-disciplined PTP clock upstream.
    WirelessEsp32C6,
}

/// One stereo ESP-AVB endpoint assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EspAvbEndpointSpec {
    /// Stable Aurora-side endpoint identifier.
    pub endpoint_id: String,
    /// Stream identifier reserved by the Aurora control plane.
    pub stream_id: u64,
    /// Endpoint medium/capability family.
    pub medium: EspAvbEndpointMedium,
    /// Two source channel indexes from canonical Aurora 7.1.4 PCM.
    pub channels: [usize; ESP_AVB_CHANNELS_PER_STREAM],
}

/// Exact six-node mapping for Aurora canonical 7.1.4.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EspAvbFanoutPlan {
    /// Six stereo endpoint assignments.
    pub endpoints: [EspAvbEndpointSpec; ESP_AVB_7_1_4_ENDPOINTS],
}

impl EspAvbFanoutPlan {
    /// Creates the canonical pairwise mapping:
    /// FL/FR, FC/LFE, SL/SR, SBL/SBR, TFL/TFR, TRL/TRR.
    pub fn canonical(medium: EspAvbEndpointMedium) -> Self {
        Self {
            endpoints: std::array::from_fn(|index| EspAvbEndpointSpec {
                endpoint_id: format!("esp-avb-node-{}", index + 1),
                stream_id: 0xA700_0000_u64 + index as u64,
                medium,
                channels: [index * 2, index * 2 + 1],
            }),
        }
    }

    /// Validates that every canonical 7.1.4 channel is assigned exactly once,
    /// with unique endpoint and stream identities.
    pub fn validate(&self) -> Result<(), EspAvbFanoutError> {
        let mut seen_channels = [false; AURORA_7_1_4_CHANNELS];

        for (index, endpoint) in self.endpoints.iter().enumerate() {
            if endpoint.endpoint_id.is_empty() {
                return Err(EspAvbFanoutError::InvalidPlan);
            }
            for other in self.endpoints.iter().skip(index + 1) {
                if endpoint.endpoint_id == other.endpoint_id
                    || endpoint.stream_id == other.stream_id
                {
                    return Err(EspAvbFanoutError::InvalidPlan);
                }
            }
            for channel in endpoint.channels {
                if channel >= AURORA_7_1_4_CHANNELS || seen_channels[channel] {
                    return Err(EspAvbFanoutError::InvalidPlan);
                }
                seen_channels[channel] = true;
            }
        }

        if seen_channels.iter().any(|seen| !seen) {
            return Err(EspAvbFanoutError::InvalidPlan);
        }
        Ok(())
    }

    /// Returns the role pair assigned to one endpoint.
    pub fn role_pair(
        &self,
        endpoint_index: usize,
    ) -> Result<[&'static str; ESP_AVB_CHANNELS_PER_STREAM], EspAvbFanoutError> {
        let endpoint = self
            .endpoints
            .get(endpoint_index)
            .ok_or(EspAvbFanoutError::InvalidEndpointIndex)?;
        Ok([
            AURORA_7_1_4_ROLES[endpoint.channels[0]],
            AURORA_7_1_4_ROLES[endpoint.channels[1]],
        ])
    }

    /// Backend capability truth for one endpoint family.
    pub fn endpoint_capabilities(
        &self,
        endpoint_index: usize,
    ) -> Result<NetworkTransportCapabilities, EspAvbFanoutError> {
        let endpoint = self
            .endpoints
            .get(endpoint_index)
            .ok_or(EspAvbFanoutError::InvalidEndpointIndex)?;
        Ok(NetworkTransportCapabilities {
            family: NetworkTransportFamily::AvbTsn,
            max_channels: ESP_AVB_CHANNELS_PER_STREAM,
            scheduled_playout: true,
            hardware_timestamps: matches!(endpoint.medium, EspAvbEndpointMedium::WiredEsp32P4),
            // This path is PTP/gPTP disciplined. It must not silently add a
            // second adaptive rate controller on top of Aurora's clock policy.
            adaptive_rate_matching: false,
            // The pinned AVB path is scheduled transport, not retransmission.
            packet_repair: false,
        })
    }

    /// ESP endpoint streams require network-time mapping as PTP followers.
    pub const fn required_clock_discipline() -> NetworkClockDiscipline {
        NetworkClockDiscipline::PtpFollower
    }
}

/// Prepared worker-side splitter with all storage allocated before streaming.
pub struct PreparedEspAvbFanout {
    plan: EspAvbFanoutPlan,
    input_format: NetworkAudioFormat,
    endpoint_format: NetworkAudioFormat,
    buffers: [Vec<f32>; ESP_AVB_7_1_4_ENDPOINTS],
    current_sequence: Option<u64>,
    current_timestamp: Option<MediaTimestamp>,
    next_sequence: Option<u64>,
    next_timestamp: Option<MediaTimestamp>,
}

impl PreparedEspAvbFanout {
    /// Allocates fixed endpoint buffers on the control/worker thread.
    pub fn prepare(plan: EspAvbFanoutPlan, block_frames: usize) -> Result<Self, EspAvbFanoutError> {
        plan.validate()?;
        if block_frames == 0 {
            return Err(EspAvbFanoutError::InvalidFormat);
        }
        let input_format = NetworkAudioFormat {
            sample_rate: AURORA_NETWORK_MEDIA_RATE,
            channels: AURORA_7_1_4_CHANNELS,
            block_frames,
        };
        let endpoint_format = NetworkAudioFormat {
            sample_rate: AURORA_NETWORK_MEDIA_RATE,
            channels: ESP_AVB_CHANNELS_PER_STREAM,
            block_frames,
        };
        let endpoint_samples = endpoint_format
            .samples_per_block()
            .ok_or(EspAvbFanoutError::InvalidFormat)?;
        input_format
            .samples_per_block()
            .ok_or(EspAvbFanoutError::InvalidFormat)?;

        Ok(Self {
            plan,
            input_format,
            endpoint_format,
            buffers: std::array::from_fn(|_| vec![0.0; endpoint_samples]),
            current_sequence: None,
            current_timestamp: None,
            next_sequence: None,
            next_timestamp: None,
        })
    }

    /// Splits one exact canonical 7.1.4 block into six stereo endpoint buffers.
    /// No allocation or network I/O occurs in this method after preparation.
    pub fn split(&mut self, block: &NetworkAudioBlock<'_>) -> Result<(), EspAvbFanoutError> {
        block
            .validate()
            .map_err(|_| EspAvbFanoutError::InvalidFormat)?;
        if block.format != self.input_format {
            return Err(EspAvbFanoutError::InvalidFormat);
        }
        if let Some(expected) = self.next_sequence {
            if block.sequence != expected {
                return Err(EspAvbFanoutError::TimelineDiscontinuity);
            }
        }
        if let Some(expected) = self.next_timestamp {
            if block.timestamp != expected {
                return Err(EspAvbFanoutError::TimelineDiscontinuity);
            }
        }

        for frame in 0..self.input_format.block_frames {
            let input_base = frame * AURORA_7_1_4_CHANNELS;
            let output_base = frame * ESP_AVB_CHANNELS_PER_STREAM;
            for endpoint_index in 0..ESP_AVB_7_1_4_ENDPOINTS {
                let channels = self.plan.endpoints[endpoint_index].channels;
                let output = &mut self.buffers[endpoint_index];
                output[output_base] = block.samples[input_base + channels[0]];
                output[output_base + 1] = block.samples[input_base + channels[1]];
            }
        }

        let next_sequence = block
            .sequence
            .checked_add(1)
            .ok_or(EspAvbFanoutError::TimelineDiscontinuity)?;
        let next_timestamp = block
            .timestamp
            .checked_advance(block.format.block_frames)
            .map_err(|_| EspAvbFanoutError::TimelineDiscontinuity)?;

        self.current_sequence = Some(block.sequence);
        self.current_timestamp = Some(block.timestamp);
        self.next_sequence = Some(next_sequence);
        self.next_timestamp = Some(next_timestamp);
        Ok(())
    }

    /// Returns one stereo endpoint block after `split` has completed.
    pub fn endpoint_block(
        &self,
        endpoint_index: usize,
    ) -> Result<NetworkAudioBlock<'_>, EspAvbFanoutError> {
        let samples = self
            .buffers
            .get(endpoint_index)
            .ok_or(EspAvbFanoutError::InvalidEndpointIndex)?;
        let sequence = self
            .current_sequence
            .ok_or(EspAvbFanoutError::NoBlockReady)?;
        let timestamp = self
            .current_timestamp
            .ok_or(EspAvbFanoutError::NoBlockReady)?;
        Ok(NetworkAudioBlock {
            sequence,
            timestamp,
            format: self.endpoint_format,
            samples,
        })
    }

    /// Returns the immutable deployment plan.
    pub fn plan(&self) -> &EspAvbFanoutPlan {
        &self.plan
    }

    /// Clears timeline history for an explicit new stream epoch while retaining
    /// all prepared buffer capacity.
    pub fn reset(&mut self) {
        self.current_sequence = None;
        self.current_timestamp = None;
        self.next_sequence = None;
        self.next_timestamp = None;
    }
}

/// Fail-closed fanout/control-plane errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EspAvbFanoutError {
    /// Endpoint identities, stream IDs or channel coverage are invalid.
    InvalidPlan,
    /// Input format does not match canonical 12-channel / 48 kHz preparation.
    InvalidFormat,
    /// Endpoint index is outside the six-node plan.
    InvalidEndpointIndex,
    /// No block has been split in the current stream epoch.
    NoBlockReady,
    /// Sequence or media timestamp is not exactly continuous.
    TimelineDiscontinuity,
}

impl fmt::Display for EspAvbFanoutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPlan => formatter.write_str("invalid ESP-AVB endpoint fanout plan"),
            Self::InvalidFormat => formatter.write_str("invalid ESP-AVB fanout PCM format"),
            Self::InvalidEndpointIndex => formatter.write_str("invalid ESP-AVB endpoint index"),
            Self::NoBlockReady => formatter.write_str("no ESP-AVB endpoint block is ready"),
            Self::TimelineDiscontinuity => {
                formatter.write_str("ESP-AVB fanout sequence/timestamp discontinuity")
            }
        }
    }
}

impl std::error::Error for EspAvbFanoutError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_samples(block_frames: usize) -> Vec<f32> {
        let mut samples = vec![0.0; block_frames * AURORA_7_1_4_CHANNELS];
        for frame in 0..block_frames {
            for channel in 0..AURORA_7_1_4_CHANNELS {
                samples[frame * AURORA_7_1_4_CHANNELS + channel] = (frame * 100 + channel) as f32;
            }
        }
        samples
    }

    #[test]
    fn canonical_plan_covers_every_role_exactly_once() {
        let plan = EspAvbFanoutPlan::canonical(EspAvbEndpointMedium::WirelessEsp32C6);
        assert_eq!(plan.validate(), Ok(()));
        assert_eq!(plan.role_pair(0).unwrap(), ["FL", "FR"]);
        assert_eq!(plan.role_pair(1).unwrap(), ["FC", "LFE"]);
        assert_eq!(plan.role_pair(5).unwrap(), ["TRL", "TRR"]);
        assert_eq!(
            EspAvbFanoutPlan::required_clock_discipline(),
            NetworkClockDiscipline::PtpFollower
        );
    }

    #[test]
    fn duplicate_channel_or_stream_fails_closed() {
        let mut plan = EspAvbFanoutPlan::canonical(EspAvbEndpointMedium::WirelessEsp32C6);
        plan.endpoints[5].channels[1] = 0;
        assert_eq!(plan.validate(), Err(EspAvbFanoutError::InvalidPlan));

        let mut plan = EspAvbFanoutPlan::canonical(EspAvbEndpointMedium::WirelessEsp32C6);
        plan.endpoints[5].stream_id = plan.endpoints[0].stream_id;
        assert_eq!(plan.validate(), Err(EspAvbFanoutError::InvalidPlan));
    }

    #[test]
    fn endpoint_capabilities_keep_p4_and_c6_timestamp_truth_separate() {
        let wired = EspAvbFanoutPlan::canonical(EspAvbEndpointMedium::WiredEsp32P4);
        let wireless = EspAvbFanoutPlan::canonical(EspAvbEndpointMedium::WirelessEsp32C6);
        let wired_caps = wired.endpoint_capabilities(0).unwrap();
        let wireless_caps = wireless.endpoint_capabilities(0).unwrap();
        assert_eq!(wired_caps.family, NetworkTransportFamily::AvbTsn);
        assert_eq!(wired_caps.max_channels, 2);
        assert!(wired_caps.hardware_timestamps);
        assert!(!wireless_caps.hardware_timestamps);
        assert!(!wired_caps.adaptive_rate_matching);
        assert!(!wireless_caps.packet_repair);
    }

    #[test]
    fn split_preserves_channel_mapping_sequence_and_timestamp() {
        let block_frames = 3;
        let samples = make_samples(block_frames);
        let format = NetworkAudioFormat {
            sample_rate: AURORA_NETWORK_MEDIA_RATE,
            channels: AURORA_7_1_4_CHANNELS,
            block_frames,
        };
        let input = NetworkAudioBlock {
            sequence: 11,
            timestamp: MediaTimestamp::new(1_000, AURORA_NETWORK_MEDIA_RATE).unwrap(),
            format,
            samples: &samples,
        };
        let plan = EspAvbFanoutPlan::canonical(EspAvbEndpointMedium::WirelessEsp32C6);
        let mut fanout = PreparedEspAvbFanout::prepare(plan, block_frames).unwrap();
        fanout.split(&input).unwrap();

        for endpoint_index in 0..ESP_AVB_7_1_4_ENDPOINTS {
            let block = fanout.endpoint_block(endpoint_index).unwrap();
            assert_eq!(block.sequence, input.sequence);
            assert_eq!(block.timestamp, input.timestamp);
            assert_eq!(block.format.channels, 2);
            let source = [endpoint_index * 2, endpoint_index * 2 + 1];
            for frame in 0..block_frames {
                assert_eq!(
                    block.samples[frame * 2],
                    samples[frame * AURORA_7_1_4_CHANNELS + source[0]]
                );
                assert_eq!(
                    block.samples[frame * 2 + 1],
                    samples[frame * AURORA_7_1_4_CHANNELS + source[1]]
                );
            }
        }
    }

    #[test]
    fn timeline_discontinuity_fails_closed_and_reset_starts_new_epoch() {
        let block_frames = 2;
        let samples = make_samples(block_frames);
        let format = NetworkAudioFormat {
            sample_rate: AURORA_NETWORK_MEDIA_RATE,
            channels: AURORA_7_1_4_CHANNELS,
            block_frames,
        };
        let plan = EspAvbFanoutPlan::canonical(EspAvbEndpointMedium::WiredEsp32P4);
        let mut fanout = PreparedEspAvbFanout::prepare(plan, block_frames).unwrap();
        let first = NetworkAudioBlock {
            sequence: 0,
            timestamp: MediaTimestamp::new(0, AURORA_NETWORK_MEDIA_RATE).unwrap(),
            format,
            samples: &samples,
        };
        fanout.split(&first).unwrap();

        let skipped = NetworkAudioBlock {
            sequence: 2,
            timestamp: MediaTimestamp::new(block_frames as u64, AURORA_NETWORK_MEDIA_RATE).unwrap(),
            format,
            samples: &samples,
        };
        assert_eq!(
            fanout.split(&skipped),
            Err(EspAvbFanoutError::TimelineDiscontinuity)
        );

        fanout.reset();
        fanout.split(&skipped).unwrap();
        assert_eq!(fanout.endpoint_block(0).unwrap().sequence, 2);
    }

    #[test]
    fn wrong_rate_or_channel_count_is_rejected() {
        let block_frames = 2;
        let plan = EspAvbFanoutPlan::canonical(EspAvbEndpointMedium::WirelessEsp32C6);
        let mut fanout = PreparedEspAvbFanout::prepare(plan, block_frames).unwrap();
        let samples = vec![0.0; 11 * block_frames];
        let wrong = NetworkAudioBlock {
            sequence: 0,
            timestamp: MediaTimestamp::new(0, 44_100).unwrap(),
            format: NetworkAudioFormat {
                sample_rate: 44_100,
                channels: 11,
                block_frames,
            },
            samples: &samples,
        };
        assert_eq!(fanout.split(&wrong), Err(EspAvbFanoutError::InvalidFormat));
    }
}
