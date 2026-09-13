//! Hardware-agnostic live encoded-audio decode and object-render preparation.
//!
//! This module runs on the media/control side, not inside the hard realtime audio callback. It
//! turns validated encoded packets into decoded PCM plus renderer gains while enforcing explicit
//! object-scene semantics, bounded acquisition, fail-closed faults, and discontinuity recovery.

use aurora_core::{AudioBlock, AudioFormat, AudioObject, Listener, Speaker};
use aurora_decoder_api::{
    DecodedBatch, DecoderError, DecoderOutputSemantics, DecoderPacket, StreamingDecoder,
};
use aurora_renderer_api::{RenderObject, Renderer, RendererError, RendererScratch, SpeakerGain};
use thiserror::Error;

/// Default IEC61937 data type for E-AC-3.
pub const IEC61937_EAC3_DATA_TYPE: u8 = 0x15;

/// Live decoder lifecycle visible to diagnostics and callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LiveDecodeState {
    /// No stable decoder output has been observed yet.
    #[default]
    Searching,
    /// Valid packets are arriving but the stability threshold has not yet been met.
    Priming,
    /// Stable validated output may be released downstream.
    Running,
    /// Output is muted while a fresh stable sequence is reacquired after an error.
    Muted,
    /// Error budget was exhausted. Explicit recovery is required.
    Faulted,
}

/// Fail-closed policy for a live immersive decoder path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveDecodePolicy {
    /// Consecutive valid packet results required before output becomes audible.
    pub required_stable_packets: u32,
    /// Consecutive decoder/validation failures allowed before entering `Faulted`.
    pub maximum_consecutive_errors: u32,
    /// Consecutive successfully accepted packets that emit no frame before entering `Faulted`.
    pub maximum_consecutive_empty_packets: u32,
    /// Maximum object count accepted from one decoded frame.
    pub maximum_objects: usize,
    /// Require native source object semantics rather than channel PCM or synthetic upmix.
    pub require_native_objects: bool,
    /// Optional required IEC61937 data type. `Some(0x15)` is the E-AC-3/JOC path.
    pub required_data_type: Option<u8>,
}

impl Default for LiveDecodePolicy {
    fn default() -> Self {
        Self {
            required_stable_packets: 2,
            maximum_consecutive_errors: 3,
            maximum_consecutive_empty_packets: 8,
            maximum_objects: 32,
            require_native_objects: true,
            required_data_type: Some(IEC61937_EAC3_DATA_TYPE),
        }
    }
}

/// Bounded aggregate counters for the live decode path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LiveDecodeMetrics {
    /// Packets presented to the runtime.
    pub packets_received: u64,
    /// Packets that produced at least one validated frame.
    pub packets_with_frames: u64,
    /// Valid decoded frames observed, including priming frames that remain muted.
    pub decoded_frames: u64,
    /// Frames released after the runtime reached `Running`.
    pub emitted_frames: u64,
    /// Decoder or semantic/shape validation failures.
    pub failures: u64,
    /// Explicit stream discontinuities/relocks.
    pub discontinuities: u64,
    /// Explicit recoveries from `Faulted`.
    pub recoveries: u64,
    /// Highest observed native object count in one frame.
    pub maximum_observed_objects: usize,
}

/// One decoded frame ready for downstream PCM/object rendering/mixing.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveRenderedFrame {
    /// Decoder PCM output.
    pub audio: AudioBlock,
    /// Native object metadata accompanying the block.
    pub objects: Vec<AudioObject>,
    /// Object-major, speaker-minor gains for the configured output layout.
    pub speaker_gains: Vec<SpeakerGain>,
}

/// Result of one packet submission.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveProcessReport {
    /// Runtime state after processing the packet.
    pub state: LiveDecodeState,
    /// True only when decoded frames were released downstream.
    pub audible: bool,
    /// Valid frames released after stable acquisition. Priming output is intentionally discarded.
    pub frames: Vec<LiveRenderedFrame>,
    /// Aggregate counters after this packet.
    pub metrics: LiveDecodeMetrics,
}

/// Live runtime setup or processing failure.
#[derive(Debug, Error)]
pub enum LiveDecodeError {
    /// Policy values cannot produce a bounded runtime.
    #[error("invalid live decode policy")]
    InvalidPolicy,
    /// Requested native-object semantics are not provided by the decoder adapter.
    #[error("decoder `{name}` exposes {actual:?}, native object-scene semantics are required")]
    DecoderSemantics {
        /// Decoder name.
        name: &'static str,
        /// Decoder semantic class.
        actual: DecoderOutputSemantics,
    },
    /// The runtime is latched faulted until explicit recovery.
    #[error("live decoder is faulted; explicit recovery is required")]
    Faulted,
    /// Packet transport/data-type contract does not match the configured live path.
    #[error("live decoder packet contract rejected: {0}")]
    PacketContract(&'static str),
    /// Decoder adapter failure.
    #[error(transparent)]
    Decoder(#[from] DecoderError),
    /// Decoded frame violates Aurora's PCM/object contract.
    #[error("decoded live frame rejected: {0}")]
    InvalidDecodedFrame(String),
    /// Renderer setup or processing failure.
    #[error(transparent)]
    Renderer(#[from] RendererError),
}

/// Media-side live immersive runtime with replaceable decoder and renderer backends.
pub struct LiveImmersiveRuntime<D, R> {
    decoder: D,
    renderer: R,
    output_format: AudioFormat,
    listener: Listener,
    policy: LiveDecodePolicy,
    renderer_scratch: RendererScratch,
    output_channels: usize,
    state: LiveDecodeState,
    stable_packets: u32,
    consecutive_errors: u32,
    consecutive_empty_packets: u32,
    metrics: LiveDecodeMetrics,
}

impl<D, R> LiveImmersiveRuntime<D, R>
where
    D: StreamingDecoder,
    R: Renderer,
{
    /// Configures decoder and renderer before accepting live packets.
    pub fn new(
        mut decoder: D,
        mut renderer: R,
        output_format: AudioFormat,
        listener: Listener,
        layout: Vec<Speaker>,
        policy: LiveDecodePolicy,
    ) -> Result<Self, LiveDecodeError> {
        if policy.required_stable_packets == 0
            || policy.maximum_consecutive_errors == 0
            || policy.maximum_consecutive_empty_packets == 0
            || policy.maximum_objects == 0
            || output_format.sample_rate == 0
            || output_format.channel_count == 0
            || output_format.block_size == 0
            || layout.is_empty()
        {
            return Err(LiveDecodeError::InvalidPolicy);
        }
        let info = decoder.info();
        if policy.require_native_objects && info.output_semantics != DecoderOutputSemantics::ObjectScene {
            return Err(LiveDecodeError::DecoderSemantics {
                name: info.name,
                actual: info.output_semantics,
            });
        }
        decoder.configure(output_format)?;
        renderer.configure(
            layout,
            output_format.sample_rate,
            output_format.block_size,
            policy.maximum_objects,
        )?;
        let output_channels = renderer.output_channel_count();
        if output_channels == 0 {
            return Err(LiveDecodeError::InvalidPolicy);
        }
        let renderer_scratch = RendererScratch::new(renderer.required_scratch_size()?);
        Ok(Self {
            decoder,
            renderer,
            output_format,
            listener,
            policy,
            renderer_scratch,
            output_channels,
            state: LiveDecodeState::Searching,
            stable_packets: 0,
            consecutive_errors: 0,
            consecutive_empty_packets: 0,
            metrics: LiveDecodeMetrics::default(),
        })
    }

    /// Current live lifecycle state.
    pub fn state(&self) -> LiveDecodeState {
        self.state
    }

    /// Current aggregate metrics.
    pub fn metrics(&self) -> LiveDecodeMetrics {
        self.metrics
    }

    /// Pushes one validated ingress packet through decoder and renderer preparation.
    pub fn push_packet(
        &mut self,
        packet: DecoderPacket<'_>,
    ) -> Result<LiveProcessReport, LiveDecodeError> {
        if self.state == LiveDecodeState::Faulted {
            return Err(LiveDecodeError::Faulted);
        }
        self.metrics.packets_received = self.metrics.packets_received.saturating_add(1);

        if packet.discontinuity {
            self.mark_discontinuity();
        }
        if let Some(required) = self.policy.required_data_type {
            if packet.data_type != Some(required) {
                self.register_failure();
                return Err(LiveDecodeError::PacketContract(
                    "unexpected or missing IEC61937 data type",
                ));
            }
        }
        if packet.payload.is_empty() {
            self.register_failure();
            return Err(LiveDecodeError::PacketContract("empty decoder payload"));
        }

        let batch = match self.decoder.push_packet(packet) {
            Ok(batch) => batch,
            Err(error) => {
                self.register_failure();
                return Err(LiveDecodeError::Decoder(error));
            }
        };
        self.accept_batch(batch)
    }

    /// Marks a gap, seek, relock, format change, or clock epoch transition.
    ///
    /// The decoder and renderer histories are cleared and output stays muted until a new stable
    /// sequence reaches the configured acquisition threshold.
    pub fn mark_discontinuity(&mut self) {
        self.decoder.reset_stream();
        self.renderer.reset();
        self.metrics.discontinuities = self.metrics.discontinuities.saturating_add(1);
        self.state = LiveDecodeState::Muted;
        self.stable_packets = 0;
        self.consecutive_empty_packets = 0;
    }

    /// Explicitly clears a latched fault and starts a fresh muted acquisition epoch.
    pub fn recover(&mut self) {
        self.decoder.reset_stream();
        self.renderer.reset();
        self.state = LiveDecodeState::Searching;
        self.stable_packets = 0;
        self.consecutive_errors = 0;
        self.consecutive_empty_packets = 0;
        self.metrics.recoveries = self.metrics.recoveries.saturating_add(1);
    }

    fn accept_batch(&mut self, batch: DecodedBatch) -> Result<LiveProcessReport, LiveDecodeError> {
        if batch.frames.is_empty() {
            self.consecutive_empty_packets = self.consecutive_empty_packets.saturating_add(1);
            self.stable_packets = 0;
            self.state = LiveDecodeState::Priming;
            if self.consecutive_empty_packets >= self.policy.maximum_consecutive_empty_packets {
                self.register_failure();
                self.state = LiveDecodeState::Faulted;
                return Err(LiveDecodeError::InvalidDecodedFrame(
                    "decoder emitted no frames for too many consecutive packets".to_owned(),
                ));
            }
            return Ok(self.report(false, Vec::new()));
        }

        if self.policy.require_native_objects && !batch.native_objects_present {
            self.register_failure();
            return Err(LiveDecodeError::InvalidDecodedFrame(
                "native object-scene evidence missing from decoded batch".to_owned(),
            ));
        }

        let mut prepared = Vec::with_capacity(batch.frames.len());
        for frame in batch.frames {
            self.validate_frame(&frame.audio, &frame.objects)?;
            let render_objects = frame
                .objects
                .iter()
                .map(|object| RenderObject {
                    position: object.position,
                    gain: db_to_linear(object.gain_db),
                })
                .collect::<Vec<_>>();
            let mut speaker_gains = vec![
                SpeakerGain::default();
                render_objects.len().saturating_mul(self.output_channels)
            ];
            if !render_objects.is_empty() {
                self.renderer.render_gains(
                    &self.listener,
                    &render_objects,
                    &mut speaker_gains,
                    &mut self.renderer_scratch,
                )?;
            }
            self.metrics.maximum_observed_objects = self
                .metrics
                .maximum_observed_objects
                .max(frame.objects.len());
            self.metrics.decoded_frames = self.metrics.decoded_frames.saturating_add(1);
            prepared.push(LiveRenderedFrame {
                audio: frame.audio,
                objects: frame.objects,
                speaker_gains,
            });
        }

        self.metrics.packets_with_frames = self.metrics.packets_with_frames.saturating_add(1);
        self.consecutive_errors = 0;
        self.consecutive_empty_packets = 0;
        self.stable_packets = self.stable_packets.saturating_add(1);
        if self.stable_packets >= self.policy.required_stable_packets {
            self.state = LiveDecodeState::Running;
            self.metrics.emitted_frames = self
                .metrics
                .emitted_frames
                .saturating_add(prepared.len() as u64);
            Ok(self.report(true, prepared))
        } else {
            self.state = LiveDecodeState::Priming;
            Ok(self.report(false, Vec::new()))
        }
    }

    fn validate_frame(&mut self, audio: &AudioBlock, objects: &[AudioObject]) -> Result<(), LiveDecodeError> {
        if audio.validate().is_err() {
            self.register_failure();
            return Err(LiveDecodeError::InvalidDecodedFrame(
                "PCM channel length does not match frame_count".to_owned(),
            ));
        }
        if audio.channels.len() != self.output_format.channel_count {
            self.register_failure();
            return Err(LiveDecodeError::InvalidDecodedFrame(format!(
                "expected {} PCM channels, got {}",
                self.output_format.channel_count,
                audio.channels.len()
            )));
        }
        if audio.frame_count == 0
            || audio
                .channels
                .iter()
                .flatten()
                .any(|sample| !sample.is_finite())
            || !audio.presentation_time_seconds.is_finite()
        {
            self.register_failure();
            return Err(LiveDecodeError::InvalidDecodedFrame(
                "PCM is empty or contains non-finite values".to_owned(),
            ));
        }
        if objects.len() > self.policy.maximum_objects {
            self.register_failure();
            return Err(LiveDecodeError::InvalidDecodedFrame(format!(
                "object count {} exceeds configured maximum {}",
                objects.len(),
                self.policy.maximum_objects
            )));
        }
        if self.policy.require_native_objects && objects.is_empty() {
            self.register_failure();
            return Err(LiveDecodeError::InvalidDecodedFrame(
                "object-scene decoder emitted a frame without object metadata".to_owned(),
            ));
        }
        if objects.iter().any(|object| {
            !object.position.x.is_finite()
                || !object.position.y.is_finite()
                || !object.position.z.is_finite()
                || !object.velocity.x.is_finite()
                || !object.velocity.y.is_finite()
                || !object.velocity.z.is_finite()
                || !object.gain_db.is_finite()
                || !object.spread.is_finite()
                || !(0.0..=1.0).contains(&object.spread)
        }) {
            self.register_failure();
            return Err(LiveDecodeError::InvalidDecodedFrame(
                "object metadata contains invalid numeric values".to_owned(),
            ));
        }
        Ok(())
    }

    fn register_failure(&mut self) {
        self.metrics.failures = self.metrics.failures.saturating_add(1);
        self.consecutive_errors = self.consecutive_errors.saturating_add(1);
        self.stable_packets = 0;
        self.consecutive_empty_packets = 0;
        self.decoder.reset_stream();
        self.renderer.reset();
        self.state = if self.consecutive_errors >= self.policy.maximum_consecutive_errors {
            LiveDecodeState::Faulted
        } else {
            LiveDecodeState::Muted
        };
    }

    fn report(&self, audible: bool, frames: Vec<LiveRenderedFrame>) -> LiveProcessReport {
        LiveProcessReport {
            state: self.state,
            audible,
            frames,
            metrics: self.metrics,
        }
    }
}

fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::{SampleType, Vector3};
    use aurora_decoder_api::{DecodedFrame, DecoderInfo, DecoderPacketTransport};
    use aurora_renderer_basic::{BasicRenderer, BasicRendererMode};

    #[derive(Debug)]
    struct MockDecoder {
        native_objects: bool,
        fail_packets: u32,
        configured: bool,
    }

    impl MockDecoder {
        fn object_frame() -> DecodedFrame {
            DecodedFrame {
                audio: AudioBlock {
                    channels: vec![vec![0.25; 256], vec![0.25; 256]],
                    frame_count: 256,
                    presentation_time_seconds: 0.0,
                    discontinuity: false,
                },
                objects: vec![AudioObject {
                    id: "object-1".to_owned(),
                    position: Vector3::new(0.0, 1.0, 1.0),
                    velocity: Vector3::ZERO,
                    gain_db: 0.0,
                    spread: 0.0,
                    start_time_seconds: None,
                    end_time_seconds: None,
                }],
            }
        }
    }

    impl StreamingDecoder for MockDecoder {
        fn info(&self) -> DecoderInfo {
            DecoderInfo {
                name: "mock-live-object-decoder",
                production_ready: false,
                maturity: "test",
                output_semantics: if self.native_objects {
                    DecoderOutputSemantics::ObjectScene
                } else {
                    DecoderOutputSemantics::ChannelPcm
                },
            }
        }

        fn configure(&mut self, _output_format: AudioFormat) -> Result<(), DecoderError> {
            self.configured = true;
            Ok(())
        }

        fn push_packet(&mut self, _packet: DecoderPacket<'_>) -> Result<DecodedBatch, DecoderError> {
            assert!(self.configured);
            if self.fail_packets > 0 {
                self.fail_packets -= 1;
                return Err(DecoderError::ExternalProcess("synthetic decoder fault".to_owned()));
            }
            Ok(DecodedBatch {
                frames: vec![Self::object_frame()],
                native_objects_present: self.native_objects,
            })
        }

        fn reset_stream(&mut self) {}
    }

    fn output_format() -> AudioFormat {
        AudioFormat {
            sample_rate: 48_000,
            channel_count: 2,
            sample_type: SampleType::F32,
            block_size: 256,
        }
    }

    fn listener() -> Listener {
        Listener {
            position: Vector3::ZERO,
            orientation: Vector3::new(0.0, 1.0, 0.0),
            ear_height: 1.2,
        }
    }

    fn layout() -> Vec<Speaker> {
        vec![
            Speaker {
                id: "left".to_owned(),
                label: "Left".to_owned(),
                channel_role: aurora_core::ChannelRole::FrontLeft,
                position: Vector3::new(-1.0, 1.0, 1.2),
                orientation: Vector3::new(0.0, -1.0, 0.0),
                gain_db: 0.0,
                delay_samples: 0.0,
                enabled: true,
            },
            Speaker {
                id: "right".to_owned(),
                label: "Right".to_owned(),
                channel_role: aurora_core::ChannelRole::FrontRight,
                position: Vector3::new(1.0, 1.0, 1.2),
                orientation: Vector3::new(0.0, -1.0, 0.0),
                gain_db: 0.0,
                delay_samples: 0.0,
                enabled: true,
            },
        ]
    }

    fn packet(discontinuity: bool) -> DecoderPacket<'static> {
        DecoderPacket {
            transport: DecoderPacketTransport::Iec61937,
            data_type: Some(IEC61937_EAC3_DATA_TYPE),
            payload: b"\x0b\x77\x00\x00",
            discontinuity,
        }
    }

    fn runtime(decoder: MockDecoder) -> LiveImmersiveRuntime<MockDecoder, BasicRenderer> {
        LiveImmersiveRuntime::new(
            decoder,
            BasicRenderer::new(BasicRendererMode::InverseDistance),
            output_format(),
            listener(),
            layout(),
            LiveDecodePolicy::default(),
        )
        .unwrap()
    }

    #[test]
    fn two_clean_object_packets_are_required_before_audio_is_released() {
        let mut runtime = runtime(MockDecoder {
            native_objects: true,
            fail_packets: 0,
            configured: false,
        });
        let first = runtime.push_packet(packet(false)).unwrap();
        assert_eq!(first.state, LiveDecodeState::Priming);
        assert!(!first.audible);
        assert!(first.frames.is_empty());
        let second = runtime.push_packet(packet(false)).unwrap();
        assert_eq!(second.state, LiveDecodeState::Running);
        assert!(second.audible);
        assert_eq!(second.frames.len(), 1);
        assert_eq!(second.frames[0].speaker_gains.len(), 2);
    }

    #[test]
    fn non_object_decoder_is_rejected_at_setup() {
        let result = LiveImmersiveRuntime::new(
            MockDecoder {
                native_objects: false,
                fail_packets: 0,
                configured: false,
            },
            BasicRenderer::new(BasicRendererMode::InverseDistance),
            output_format(),
            listener(),
            layout(),
            LiveDecodePolicy::default(),
        );
        assert!(matches!(result, Err(LiveDecodeError::DecoderSemantics { .. })));
    }

    #[test]
    fn wrong_iec_data_type_mutes_and_eventually_faults() {
        let mut runtime = runtime(MockDecoder {
            native_objects: true,
            fail_packets: 0,
            configured: false,
        });
        let bad = DecoderPacket {
            data_type: Some(0x01),
            ..packet(false)
        };
        for expected_state in [
            LiveDecodeState::Muted,
            LiveDecodeState::Muted,
            LiveDecodeState::Faulted,
        ] {
            assert!(runtime.push_packet(bad).is_err());
            assert_eq!(runtime.state(), expected_state);
        }
        assert!(matches!(runtime.push_packet(packet(false)), Err(LiveDecodeError::Faulted)));
        runtime.recover();
        assert_eq!(runtime.state(), LiveDecodeState::Searching);
    }

    #[test]
    fn discontinuity_discards_old_lock_and_requires_reprime() {
        let mut runtime = runtime(MockDecoder {
            native_objects: true,
            fail_packets: 0,
            configured: false,
        });
        runtime.push_packet(packet(false)).unwrap();
        assert!(runtime.push_packet(packet(false)).unwrap().audible);
        let after_gap = runtime.push_packet(packet(true)).unwrap();
        assert_eq!(after_gap.state, LiveDecodeState::Priming);
        assert!(!after_gap.audible);
        assert!(runtime.push_packet(packet(false)).unwrap().audible);
        assert_eq!(runtime.metrics().discontinuities, 1);
    }
}
