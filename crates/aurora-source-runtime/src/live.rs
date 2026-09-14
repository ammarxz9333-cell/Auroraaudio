//! Hardware-agnostic live encoded-audio decode, object rendering, and speaker mixing.
//!
//! This module runs on the media/control side, not inside the hard realtime audio callback. It
//! consumes validated encoded packets through [`aurora_decoder_api::StreamingDecoder`], preserves
//! source PCM channel semantics and object/channel bindings, renders object gains, routes bed
//! channels, and emits final speaker PCM only after a bounded stable-acquisition gate.

use aurora_core::{AudioBlock, AudioFormat, AudioObject, ChannelRole, Listener, Speaker};
use aurora_decoder_api::{
    DecodedBatch, DecodedChannelKind, DecoderError, DecoderOutputSemantics, DecoderPacket,
    DecoderPacketTransport, ObjectChannelBinding, StreamingDecodedFrame, StreamingDecoder,
    StreamingDecoderConfig,
};
use aurora_renderer_api::{RenderObject, Renderer, RendererError, RendererScratch, SpeakerGain};
use thiserror::Error;

/// IEC61937 data type for E-AC-3.
pub const IEC61937_EAC3_DATA_TYPE: u8 = 0x15;

/// Live decoder lifecycle visible to diagnostics and callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LiveDecodeState {
    /// No stable decoder output has been observed yet.
    #[default]
    Searching,
    /// Valid packets are arriving but the stability/object threshold has not yet been met.
    Priming,
    /// Stable validated output may be released downstream.
    Running,
    /// Output is muted while a fresh stable sequence is reacquired after an error.
    Muted,
    /// Error/probe budget was exhausted. Explicit recovery is required.
    Faulted,
}

/// Fail-closed policy for a live immersive decoder path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveDecodePolicy {
    /// Consecutive valid object-bearing packet results required before output becomes audible.
    pub required_stable_packets: u32,
    /// Consecutive decoder/validation failures allowed before entering `Faulted`.
    pub maximum_consecutive_errors: u32,
    /// Consecutive accepted packets that emit no frame before entering `Faulted`.
    pub maximum_consecutive_empty_packets: u32,
    /// Consecutive decoded packets allowed while waiting for native-object evidence.
    pub maximum_object_probe_packets: u32,
    /// Maximum decoded PCM channels accepted from a backend.
    pub maximum_pcm_channels: usize,
    /// Maximum native object count accepted from one decoded frame.
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
            maximum_object_probe_packets: 8,
            maximum_pcm_channels: 32,
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
    /// Highest observed source PCM channel count.
    pub maximum_observed_pcm_channels: usize,
    /// Highest observed native object count in one frame.
    pub maximum_observed_objects: usize,
}

/// One frame after source decode, object rendering, bed routing, and speaker mixing.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveRenderedFrame {
    /// Decoder PCM before object/bed mixing, retained for diagnostics and differential validation.
    pub source_audio: AudioBlock,
    /// Native object metadata accompanying the source block.
    pub objects: Vec<AudioObject>,
    /// Complete active object-to-source-channel bindings.
    pub object_channels: Vec<ObjectChannelBinding>,
    /// Object-major, speaker-minor gains for the configured output layout.
    pub speaker_gains: Vec<SpeakerGain>,
    /// Final planar speaker PCM in configured layout order.
    pub speaker_audio: AudioBlock,
}

/// Result of one packet submission.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveProcessReport {
    /// Runtime state after processing the packet.
    pub state: LiveDecodeState,
    /// True only when decoded/mixed frames were released downstream.
    pub audible: bool,
    /// Valid frames released after stable acquisition. Priming output is intentionally discarded.
    pub frames: Vec<LiveRenderedFrame>,
    /// Aggregate counters after this packet.
    pub metrics: LiveDecodeMetrics,
}

/// Live runtime setup or processing failure.
#[derive(Debug, Error)]
pub enum LiveDecodeError {
    /// Policy/layout values cannot produce a bounded runtime.
    #[error("invalid live decode policy or output layout")]
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
    /// Decoded frame violates Aurora's PCM/object/channel contract.
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
    listener: Listener,
    layout: Vec<Speaker>,
    policy: LiveDecodePolicy,
    renderer_scratch: RendererScratch,
    output_channels: usize,
    state: LiveDecodeState,
    stable_packets: u32,
    consecutive_errors: u32,
    consecutive_empty_packets: u32,
    consecutive_non_object_packets: u32,
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
            || policy.maximum_object_probe_packets == 0
            || policy.maximum_pcm_channels == 0
            || policy.maximum_objects == 0
            || output_format.sample_rate == 0
            || output_format.channel_count == 0
            || output_format.block_size == 0
            || layout.is_empty()
            || layout.iter().any(|speaker| !speaker.enabled)
        {
            return Err(LiveDecodeError::InvalidPolicy);
        }
        if layout.len() != output_format.channel_count || has_duplicate_roles(&layout) {
            return Err(LiveDecodeError::InvalidPolicy);
        }

        let info = decoder.info();
        if policy.require_native_objects
            && info.output_semantics != DecoderOutputSemantics::ObjectScene
        {
            return Err(LiveDecodeError::DecoderSemantics {
                name: info.name,
                actual: info.output_semantics,
            });
        }
        decoder.configure_stream(StreamingDecoderConfig {
            sample_rate: output_format.sample_rate,
            block_size: output_format.block_size,
            maximum_pcm_channels: policy.maximum_pcm_channels,
        })?;
        renderer.configure(
            layout.clone(),
            output_format.sample_rate,
            output_format.block_size,
            policy.maximum_objects,
        )?;
        let output_channels = renderer.output_channel_count();
        if output_channels != output_format.channel_count {
            return Err(LiveDecodeError::InvalidPolicy);
        }
        let renderer_scratch = RendererScratch::new(renderer.required_scratch_size()?);
        Ok(Self {
            decoder,
            renderer,
            listener,
            layout,
            policy,
            renderer_scratch,
            output_channels,
            state: LiveDecodeState::Searching,
            stable_packets: 0,
            consecutive_errors: 0,
            consecutive_empty_packets: 0,
            consecutive_non_object_packets: 0,
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

    /// Pushes one validated ingress packet through decoder, renderer, and speaker mixer.
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
            if packet.transport != DecoderPacketTransport::Iec61937 {
                self.register_failure();
                return Err(LiveDecodeError::PacketContract(
                    "IEC61937 transport is required for the configured data type",
                ));
            }
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
    pub fn mark_discontinuity(&mut self) {
        self.decoder.reset_stream();
        self.renderer.reset();
        self.metrics.discontinuities = self.metrics.discontinuities.saturating_add(1);
        self.state = LiveDecodeState::Muted;
        self.stable_packets = 0;
        self.consecutive_empty_packets = 0;
        self.consecutive_non_object_packets = 0;
    }

    /// Explicitly re-arms the decoder and starts a fresh muted acquisition epoch.
    ///
    /// A panic-isolated decoder must successfully clear its crash latch here. Failure leaves the
    /// runtime `Faulted` and is returned instead of presenting a false recovery.
    pub fn try_recover(&mut self) -> Result<(), LiveDecodeError> {
        if let Err(error) = self.decoder.recover_stream() {
            self.renderer.reset();
            self.state = LiveDecodeState::Faulted;
            self.stable_packets = 0;
            self.consecutive_empty_packets = 0;
            self.consecutive_non_object_packets = 0;
            self.metrics.failures = self.metrics.failures.saturating_add(1);
            return Err(LiveDecodeError::Decoder(error));
        }
        self.renderer.reset();
        self.state = LiveDecodeState::Searching;
        self.stable_packets = 0;
        self.consecutive_errors = 0;
        self.consecutive_empty_packets = 0;
        self.consecutive_non_object_packets = 0;
        self.metrics.recoveries = self.metrics.recoveries.saturating_add(1);
        Ok(())
    }

    /// Backward-compatible explicit recovery request.
    ///
    /// Call [`Self::try_recover`] when the caller needs the backend recovery error. A failed
    /// recovery still leaves this runtime visibly `Faulted`.
    pub fn recover(&mut self) {
        let _ = self.try_recover();
    }

    fn accept_batch(&mut self, batch: DecodedBatch) -> Result<LiveProcessReport, LiveDecodeError> {
        if batch.frames.is_empty() {
            self.consecutive_empty_packets = self.consecutive_empty_packets.saturating_add(1);
            self.stable_packets = 0;
            self.state = LiveDecodeState::Priming;
            if self.consecutive_empty_packets >= self.policy.maximum_consecutive_empty_packets {
                self.latch_fault();
                return Err(LiveDecodeError::InvalidDecodedFrame(
                    "decoder emitted no frames for too many consecutive packets".to_owned(),
                ));
            }
            return Ok(self.report(false, Vec::new()));
        }

        if self.policy.require_native_objects && !batch.native_objects_present {
            self.consecutive_non_object_packets =
                self.consecutive_non_object_packets.saturating_add(1);
            self.stable_packets = 0;
            self.state = LiveDecodeState::Priming;
            if self.consecutive_non_object_packets >= self.policy.maximum_object_probe_packets {
                self.latch_fault();
                return Err(LiveDecodeError::InvalidDecodedFrame(
                    "native object-scene evidence did not appear within the probe budget"
                        .to_owned(),
                ));
            }
            return Ok(self.report(false, Vec::new()));
        }

        let mut prepared = Vec::with_capacity(batch.frames.len());
        for frame in batch.frames {
            match self.prepare_frame(frame) {
                Ok(frame) => prepared.push(frame),
                Err(error) => {
                    self.register_failure();
                    return Err(error);
                }
            }
        }

        self.metrics.packets_with_frames = self.metrics.packets_with_frames.saturating_add(1);
        self.consecutive_errors = 0;
        self.consecutive_empty_packets = 0;
        self.consecutive_non_object_packets = 0;
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

    fn prepare_frame(
        &mut self,
        frame: StreamingDecodedFrame,
    ) -> Result<LiveRenderedFrame, LiveDecodeError> {
        let source_audio = frame.decoded.audio;
        let objects = frame.decoded.objects;
        validate_source_audio(
            &source_audio,
            &frame.channel_kinds,
            self.policy.maximum_pcm_channels,
        )?;
        validate_objects(
            &objects,
            self.policy.maximum_objects,
            self.policy.require_native_objects,
        )?;
        validate_bindings(
            &objects,
            &frame.channel_kinds,
            &frame.object_channels,
            self.policy.require_native_objects,
        )?;

        let render_objects = objects
            .iter()
            .map(|object| RenderObject {
                position: object.position,
                gain: db_to_linear(object.gain_db),
            })
            .collect::<Vec<_>>();
        let mut speaker_gains =
            vec![SpeakerGain::default(); render_objects.len().saturating_mul(self.output_channels)];
        if !render_objects.is_empty() {
            self.renderer.render_gains(
                &self.listener,
                &render_objects,
                &mut speaker_gains,
                &mut self.renderer_scratch,
            )?;
        }

        let mut speaker_audio = AudioBlock {
            channels: vec![vec![0.0; source_audio.frame_count]; self.output_channels],
            frame_count: source_audio.frame_count,
            presentation_time_seconds: source_audio.presentation_time_seconds,
            discontinuity: source_audio.discontinuity,
        };
        route_bed_channels(
            &source_audio,
            &frame.channel_kinds,
            &self.layout,
            &mut speaker_audio,
        )?;
        mix_object_channels(
            &source_audio,
            &objects,
            &frame.object_channels,
            &speaker_gains,
            self.output_channels,
            &mut speaker_audio,
        )?;
        if speaker_audio
            .channels
            .iter()
            .flatten()
            .any(|sample| !sample.is_finite())
        {
            return Err(LiveDecodeError::InvalidDecodedFrame(
                "speaker mix produced non-finite samples".to_owned(),
            ));
        }

        self.metrics.maximum_observed_pcm_channels = self
            .metrics
            .maximum_observed_pcm_channels
            .max(source_audio.channels.len());
        self.metrics.maximum_observed_objects =
            self.metrics.maximum_observed_objects.max(objects.len());
        self.metrics.decoded_frames = self.metrics.decoded_frames.saturating_add(1);

        Ok(LiveRenderedFrame {
            source_audio,
            objects,
            object_channels: frame.object_channels,
            speaker_gains,
            speaker_audio,
        })
    }

    fn register_failure(&mut self) {
        self.metrics.failures = self.metrics.failures.saturating_add(1);
        self.consecutive_errors = self.consecutive_errors.saturating_add(1);
        self.stable_packets = 0;
        self.consecutive_empty_packets = 0;
        self.consecutive_non_object_packets = 0;
        self.decoder.reset_stream();
        self.renderer.reset();
        self.state = if self.consecutive_errors >= self.policy.maximum_consecutive_errors {
            LiveDecodeState::Faulted
        } else {
            LiveDecodeState::Muted
        };
    }

    fn latch_fault(&mut self) {
        self.metrics.failures = self.metrics.failures.saturating_add(1);
        self.decoder.reset_stream();
        self.renderer.reset();
        self.stable_packets = 0;
        self.state = LiveDecodeState::Faulted;
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

fn validate_source_audio(
    audio: &AudioBlock,
    channel_kinds: &[DecodedChannelKind],
    maximum_pcm_channels: usize,
) -> Result<(), LiveDecodeError> {
    if audio.validate().is_err() {
        return Err(LiveDecodeError::InvalidDecodedFrame(
            "PCM channel length does not match frame_count".to_owned(),
        ));
    }
    if audio.channels.is_empty()
        || audio.channels.len() > maximum_pcm_channels
        || audio.frame_count == 0
        || channel_kinds.len() != audio.channels.len()
        || audio
            .channels
            .iter()
            .flatten()
            .any(|sample| !sample.is_finite())
        || !audio.presentation_time_seconds.is_finite()
    {
        return Err(LiveDecodeError::InvalidDecodedFrame(
            "PCM/channel semantics are empty, oversized, mismatched, or non-finite".to_owned(),
        ));
    }
    if channel_kinds
        .iter()
        .any(|kind| matches!(kind, DecodedChannelKind::Unknown))
    {
        return Err(LiveDecodeError::InvalidDecodedFrame(
            "decoder exposed an unknown PCM channel role".to_owned(),
        ));
    }
    Ok(())
}

fn validate_objects(
    objects: &[AudioObject],
    maximum_objects: usize,
    require_native_objects: bool,
) -> Result<(), LiveDecodeError> {
    if objects.len() > maximum_objects || (require_native_objects && objects.is_empty()) {
        return Err(LiveDecodeError::InvalidDecodedFrame(
            "object count is empty or exceeds the configured maximum".to_owned(),
        ));
    }
    for (index, object) in objects.iter().enumerate() {
        if objects[..index]
            .iter()
            .any(|previous| previous.id == object.id)
            || object.id.is_empty()
            || !object.position.x.is_finite()
            || !object.position.y.is_finite()
            || !object.position.z.is_finite()
            || !object.velocity.x.is_finite()
            || !object.velocity.y.is_finite()
            || !object.velocity.z.is_finite()
            || !object.gain_db.is_finite()
            || !object.spread.is_finite()
            || !(0.0..=1.0).contains(&object.spread)
        {
            return Err(LiveDecodeError::InvalidDecodedFrame(
                "object metadata contains duplicate IDs or invalid numeric values".to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_bindings(
    objects: &[AudioObject],
    channel_kinds: &[DecodedChannelKind],
    bindings: &[ObjectChannelBinding],
    require_native_objects: bool,
) -> Result<(), LiveDecodeError> {
    if require_native_objects && bindings.len() != objects.len() {
        return Err(LiveDecodeError::InvalidDecodedFrame(
            "native object table is not fully bound to PCM channels".to_owned(),
        ));
    }
    for (index, binding) in bindings.iter().enumerate() {
        if binding.channel_index >= channel_kinds.len()
            || channel_kinds[binding.channel_index] != DecodedChannelKind::Object
            || !objects.iter().any(|object| object.id == binding.object_id)
            || bindings[..index].iter().any(|previous| {
                previous.object_id == binding.object_id
                    || previous.channel_index == binding.channel_index
            })
        {
            return Err(LiveDecodeError::InvalidDecodedFrame(
                "object/channel binding is stale, duplicate, or points to a non-object channel"
                    .to_owned(),
            ));
        }
    }
    for (channel_index, kind) in channel_kinds.iter().enumerate() {
        if *kind == DecodedChannelKind::Object
            && !bindings
                .iter()
                .any(|binding| binding.channel_index == channel_index)
        {
            return Err(LiveDecodeError::InvalidDecodedFrame(
                "object PCM channel has no active object binding".to_owned(),
            ));
        }
    }
    Ok(())
}

fn route_bed_channels(
    source: &AudioBlock,
    channel_kinds: &[DecodedChannelKind],
    layout: &[Speaker],
    output: &mut AudioBlock,
) -> Result<(), LiveDecodeError> {
    for (source_index, kind) in channel_kinds.iter().enumerate() {
        let DecodedChannelKind::Bed(role) = kind else {
            continue;
        };
        let output_index = unique_layout_role_index(layout, role).ok_or_else(|| {
            LiveDecodeError::InvalidDecodedFrame(format!(
                "decoded bed role `{role}` has no unique output speaker"
            ))
        })?;
        for frame_index in 0..source.frame_count {
            output.channels[output_index][frame_index] +=
                source.channels[source_index][frame_index];
        }
    }
    Ok(())
}

fn mix_object_channels(
    source: &AudioBlock,
    objects: &[AudioObject],
    bindings: &[ObjectChannelBinding],
    gains: &[SpeakerGain],
    output_channels: usize,
    output: &mut AudioBlock,
) -> Result<(), LiveDecodeError> {
    if gains.len() != objects.len().saturating_mul(output_channels) {
        return Err(LiveDecodeError::InvalidDecodedFrame(
            "renderer gain shape does not match object/output dimensions".to_owned(),
        ));
    }
    for (object_index, object) in objects.iter().enumerate() {
        let binding = bindings
            .iter()
            .find(|binding| binding.object_id == object.id)
            .ok_or_else(|| {
                LiveDecodeError::InvalidDecodedFrame(format!(
                    "object `{}` has no PCM channel binding",
                    object.id
                ))
            })?;
        let source_channel = &source.channels[binding.channel_index];
        for gain in &gains[object_index * output_channels..(object_index + 1) * output_channels] {
            if gain.speaker_index >= output_channels || !gain.gain.is_finite() {
                return Err(LiveDecodeError::InvalidDecodedFrame(
                    "renderer returned an invalid speaker index or gain".to_owned(),
                ));
            }
            let output_channel = &mut output.channels[gain.speaker_index];
            for frame_index in 0..source.frame_count {
                output_channel[frame_index] += source_channel[frame_index] * gain.gain;
            }
        }
    }
    Ok(())
}

fn unique_layout_role_index(layout: &[Speaker], role: &ChannelRole) -> Option<usize> {
    let mut matches = layout
        .iter()
        .enumerate()
        .filter(|(_, speaker)| &speaker.channel_role == role)
        .map(|(index, _)| index);
    let first = matches.next()?;
    if matches.next().is_some() {
        None
    } else {
        Some(first)
    }
}

fn has_duplicate_roles(layout: &[Speaker]) -> bool {
    layout.iter().enumerate().any(|(index, speaker)| {
        layout[..index]
            .iter()
            .any(|previous| previous.channel_role == speaker.channel_role)
    })
}

fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::{SampleType, Vector3};
    use aurora_decoder_api::{
        DecodedFrame, DecoderInfo, PanicIsolatedStreamingDecoder, StreamingDecodedFrame,
        StreamingDecoderConfig,
    };
    use aurora_renderer_basic::{BasicRenderer, BasicRendererMode};

    #[derive(Debug)]
    struct MockDecoder {
        native_objects: bool,
        fail_packets: u32,
        configured: bool,
    }

    impl MockDecoder {
        fn object_frame() -> StreamingDecodedFrame {
            StreamingDecodedFrame {
                decoded: DecodedFrame {
                    audio: AudioBlock {
                        channels: vec![vec![0.10; 256], vec![0.25; 256], vec![0.10; 256]],
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
                },
                channel_kinds: vec![
                    DecodedChannelKind::Bed(ChannelRole::FrontLeft),
                    DecodedChannelKind::Object,
                    DecodedChannelKind::Bed(ChannelRole::FrontRight),
                ],
                object_channels: vec![ObjectChannelBinding {
                    object_id: "object-1".to_owned(),
                    channel_index: 1,
                }],
            }
        }
    }

    #[derive(Debug)]
    struct PanicOnceDecoder {
        configured: bool,
        panic_on_push: bool,
    }

    impl StreamingDecoder for PanicOnceDecoder {
        fn info(&self) -> DecoderInfo {
            DecoderInfo {
                name: "panic-once-live-object-decoder",
                production_ready: false,
                maturity: "test",
                output_semantics: DecoderOutputSemantics::ObjectScene,
            }
        }

        fn configure_stream(
            &mut self,
            _config: StreamingDecoderConfig,
        ) -> Result<(), DecoderError> {
            self.configured = true;
            Ok(())
        }

        fn push_packet(
            &mut self,
            _packet: DecoderPacket<'_>,
        ) -> Result<DecodedBatch, DecoderError> {
            assert!(self.configured);
            if self.panic_on_push {
                self.panic_on_push = false;
                panic!("intentional live decoder panic");
            }
            Ok(DecodedBatch {
                frames: vec![MockDecoder::object_frame()],
                native_objects_present: true,
            })
        }

        fn reset_stream(&mut self) {}
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

        fn configure_stream(&mut self, config: StreamingDecoderConfig) -> Result<(), DecoderError> {
            assert_eq!(config.sample_rate, 48_000);
            assert!(config.maximum_pcm_channels >= 3);
            self.configured = true;
            Ok(())
        }

        fn push_packet(
            &mut self,
            _packet: DecoderPacket<'_>,
        ) -> Result<DecodedBatch, DecoderError> {
            assert!(self.configured);
            if self.fail_packets > 0 {
                self.fail_packets -= 1;
                return Err(DecoderError::ExternalProcess(
                    "synthetic decoder fault".to_owned(),
                ));
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
                channel_role: ChannelRole::FrontLeft,
                position: Vector3::new(-1.0, 1.0, 1.2),
                orientation: Vector3::new(0.0, -1.0, 0.0),
                gain_db: 0.0,
                delay_samples: 0.0,
                enabled: true,
            },
            Speaker {
                id: "right".to_owned(),
                label: "Right".to_owned(),
                channel_role: ChannelRole::FrontRight,
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
    fn two_clean_object_packets_are_required_before_speaker_pcm_is_released() {
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
        let frame = &second.frames[0];
        assert_eq!(frame.speaker_gains.len(), 2);
        assert_eq!(frame.speaker_audio.channels.len(), 2);
        assert!(frame
            .speaker_audio
            .channels
            .iter()
            .flatten()
            .all(|sample| sample.is_finite()));
        assert!(frame.speaker_audio.channels[0][0] > 0.10);
        assert!(frame.speaker_audio.channels[1][0] > 0.10);
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
        assert!(matches!(
            result,
            Err(LiveDecodeError::DecoderSemantics { .. })
        ));
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
        assert!(matches!(
            runtime.push_packet(packet(false)),
            Err(LiveDecodeError::Faulted)
        ));
        runtime.recover();
        assert_eq!(runtime.state(), LiveDecodeState::Searching);
    }

    #[test]
    fn panic_isolation_requires_explicit_runtime_recovery_before_audio_rearms() {
        let decoder = PanicIsolatedStreamingDecoder::try_new(PanicOnceDecoder {
            configured: false,
            panic_on_push: true,
        })
        .unwrap();
        let mut runtime = LiveImmersiveRuntime::new(
            decoder,
            BasicRenderer::new(BasicRendererMode::InverseDistance),
            output_format(),
            listener(),
            layout(),
            LiveDecodePolicy::default(),
        )
        .unwrap();

        assert!(matches!(
            runtime.push_packet(packet(false)),
            Err(LiveDecodeError::Decoder(DecoderError::BackendPanic(
                "push_packet"
            )))
        ));
        assert_eq!(runtime.state(), LiveDecodeState::Muted);

        runtime.try_recover().unwrap();
        assert_eq!(runtime.state(), LiveDecodeState::Searching);
        assert_eq!(runtime.metrics().recoveries, 1);

        let first = runtime.push_packet(packet(false)).unwrap();
        assert_eq!(first.state, LiveDecodeState::Priming);
        assert!(!first.audible);
        let second = runtime.push_packet(packet(false)).unwrap();
        assert_eq!(second.state, LiveDecodeState::Running);
        assert!(second.audible);
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

    #[test]
    fn stale_object_channel_mapping_is_rejected_fail_closed() {
        let mut frame = MockDecoder::object_frame();
        frame.object_channels[0].channel_index = 0;
        assert!(validate_bindings(
            &frame.decoded.objects,
            &frame.channel_kinds,
            &frame.object_channels,
            true,
        )
        .is_err());
    }
}
