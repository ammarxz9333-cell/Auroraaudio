//! Source Manager to decoder/renderer handoff without realtime-engine coupling.
//!
//! This crate runs on the control/media side. It resolves one typed media
//! reference, decodes a validation chunk through `aurora-decoder-api`, and
//! preflights object rendering through `aurora-renderer-api`. A prepared chunk
//! can cross the activation boundary only when the same Source Manager session
//! and generation are active.

pub mod live;

use aurora_core::{AudioBlock, AudioFormat, AudioObject, Listener, Speaker};
use aurora_decoder_api::{DecodedFrame, Decoder, DecoderError};
use aurora_plugin_api::source_manager::{PlayableMediaRef, SourceManager, SourceSessionId};
use aurora_renderer_api::{RenderObject, Renderer, RendererError, RendererScratch, SpeakerGain};
use thiserror::Error;

/// Failure returned by a source-media loader adapter.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum MediaLoadError {
    /// The adapter does not support this typed media reference.
    #[error("unsupported media reference: {0}")]
    UnsupportedReference(&'static str),
    /// The adapter failed while resolving or reading the source.
    #[error("media load failed: {0}")]
    LoadFailed(String),
}

/// Adapter that resolves a Source Manager media reference into one decoder
/// input chunk.
///
/// Implementations run outside the audio callback. A future streaming adapter
/// can replace this first-chunk contract without exposing provider SDK types to
/// Aurora's decoder, renderer, DSP, or realtime layers.
pub trait MediaLoader {
    /// Loads one decoder input chunk.
    fn load_chunk(&mut self, media: &PlayableMediaRef) -> Result<Vec<u8>, MediaLoadError>;
}

/// A source chunk that passed media loading, decoding, and renderer preflight.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedSourceChunk {
    session: SourceSessionId,
    audio: AudioBlock,
    objects: Vec<AudioObject>,
    speaker_gains: Vec<SpeakerGain>,
}

impl PreparedSourceChunk {
    /// Source Manager session this preparation belongs to.
    pub fn session(&self) -> &SourceSessionId {
        &self.session
    }

    /// Decoded PCM validation block.
    pub fn audio(&self) -> &AudioBlock {
        &self.audio
    }

    /// Decoded object metadata accompanying the PCM block.
    pub fn objects(&self) -> &[AudioObject] {
        &self.objects
    }

    /// Object-major, speaker-minor renderer gains. Channel-only material has an
    /// empty gain list rather than being silently converted into object audio.
    pub fn speaker_gains(&self) -> &[SpeakerGain] {
        &self.speaker_gains
    }
}

/// A prepared chunk released across the Source Manager activation boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveSourceChunk {
    /// Source session proven active at commit time.
    pub session: SourceSessionId,
    /// Decoded PCM block ready for the downstream Aurora audio path.
    pub audio: AudioBlock,
    /// Decoded object metadata ready for scene/render processing.
    pub objects: Vec<AudioObject>,
    /// Preflight renderer gains for object-bearing material.
    pub speaker_gains: Vec<SpeakerGain>,
}

/// Control-thread bridge between Source Manager and replaceable media adapters.
pub struct SourceMediaRuntime<L, D, R> {
    loader: L,
    decoder: D,
    renderer: R,
}

impl<L, D, R> SourceMediaRuntime<L, D, R>
where
    L: MediaLoader,
    D: Decoder,
    R: Renderer,
{
    /// Creates a source-media runtime from independently replaceable adapters.
    pub fn new(loader: L, decoder: D, renderer: R) -> Self {
        Self {
            loader,
            decoder,
            renderer,
        }
    }

    /// Prepares one Source Manager candidate without changing the active source.
    ///
    /// The source must already have passed `SourceManager::prepare`. Media
    /// resolution, decoder configuration, first-chunk decoding, and object
    /// renderer preflight happen here. Only a successful candidate should be
    /// activated by the caller.
    pub fn prepare_source_chunk(
        &mut self,
        manager: &SourceManager,
        session: &SourceSessionId,
        output_format: AudioFormat,
        listener: &Listener,
        layout: &[Speaker],
    ) -> Result<PreparedSourceChunk, SourceRuntimeError> {
        let record = manager
            .record(&session.source_id)
            .ok_or_else(|| SourceRuntimeError::UnknownSource(session.source_id.clone()))?;
        if record.generation() != session.generation {
            return Err(SourceRuntimeError::StaleSession {
                source_id: session.source_id.clone(),
                actual: session.generation,
                current: record.generation(),
            });
        }

        let prepared = record
            .prepared()
            .ok_or_else(|| SourceRuntimeError::SourceNotPrepared(session.source_id.clone()))?;
        if prepared.session != *session {
            return Err(SourceRuntimeError::StaleSession {
                source_id: session.source_id.clone(),
                actual: prepared.session.generation,
                current: session.generation,
            });
        }

        let input = self.loader.load_chunk(&prepared.media)?;
        self.decoder.reset();
        self.decoder.configure(output_format)?;
        let decoded = self
            .decoder
            .decode_chunk(&input)?
            .ok_or(SourceRuntimeError::DecoderProducedNoFrame)?;
        validate_decoded_frame(&decoded, output_format)?;

        let speaker_gains = if decoded.objects.is_empty() {
            Vec::new()
        } else {
            if layout.is_empty() {
                return Err(SourceRuntimeError::EmptyRenderLayout);
            }
            self.renderer.reset();
            self.renderer.configure(
                layout.to_vec(),
                output_format.sample_rate,
                output_format.block_size,
                decoded.objects.len(),
            )?;
            let render_objects = decoded
                .objects
                .iter()
                .map(|object| RenderObject {
                    position: object.position,
                    gain: db_to_linear(object.gain_db),
                })
                .collect::<Vec<_>>();
            let mut gains = vec![
                SpeakerGain::default();
                render_objects.len() * self.renderer.output_channel_count()
            ];
            let mut scratch = RendererScratch::new(self.renderer.required_scratch_size()?);
            self.renderer
                .render_gains(listener, &render_objects, &mut gains, &mut scratch)?;
            gains
        };

        Ok(PreparedSourceChunk {
            session: session.clone(),
            audio: decoded.audio,
            objects: decoded.objects,
            speaker_gains,
        })
    }

    /// Releases a prepared chunk only when its exact session generation is now
    /// active in Source Manager.
    ///
    /// This is the transaction boundary: preflight may happen before source
    /// switching, but stale or merely prepared candidates cannot enter the
    /// downstream playback path.
    pub fn commit_active(
        &self,
        manager: &SourceManager,
        prepared: PreparedSourceChunk,
    ) -> Result<ActiveSourceChunk, SourceRuntimeError> {
        let active = manager
            .active_session()
            .ok_or(SourceRuntimeError::NoActiveSource)?;
        if active != prepared.session() {
            return Err(SourceRuntimeError::SessionNotActive {
                source_id: prepared.session.source_id.clone(),
                generation: prepared.session.generation,
            });
        }

        let record = manager
            .record(&prepared.session.source_id)
            .ok_or_else(|| SourceRuntimeError::UnknownSource(prepared.session.source_id.clone()))?;
        if record.generation() != prepared.session.generation {
            return Err(SourceRuntimeError::StaleSession {
                source_id: prepared.session.source_id.clone(),
                actual: prepared.session.generation,
                current: record.generation(),
            });
        }

        Ok(ActiveSourceChunk {
            session: prepared.session,
            audio: prepared.audio,
            objects: prepared.objects,
            speaker_gains: prepared.speaker_gains,
        })
    }
}

/// Fail-closed source-media handoff errors.
#[derive(Debug, Error, PartialEq)]
pub enum SourceRuntimeError {
    /// Source ID is not currently registered.
    #[error("unknown source `{0}`")]
    UnknownSource(String),
    /// Source Manager generation changed after the caller obtained its session.
    #[error("stale source session `{source_id}`: got {actual}, current {current}")]
    StaleSession {
        source_id: String,
        actual: u64,
        current: u64,
    },
    /// The source exists but has not passed Source Manager preparation.
    #[error("source `{0}` is not prepared")]
    SourceNotPrepared(String),
    /// No source is currently active.
    #[error("no active source")]
    NoActiveSource,
    /// A prepared chunk does not belong to the exact active session.
    #[error("source session `{source_id}` generation {generation} is not active")]
    SessionNotActive { source_id: String, generation: u64 },
    /// The selected media adapter could not produce decoder input.
    #[error(transparent)]
    MediaLoad(#[from] MediaLoadError),
    /// Decoder adapter rejected or failed the input.
    #[error(transparent)]
    Decoder(#[from] DecoderError),
    /// Decoder accepted input but emitted no validation frame.
    #[error("decoder produced no frame")]
    DecoderProducedNoFrame,
    /// Decoded PCM violates the configured output-format contract.
    #[error("decoded frame format mismatch: {0}")]
    DecodedFormatMismatch(String),
    /// Object rendering requires a nonempty speaker layout.
    #[error("object-bearing decoded material requires a nonempty speaker layout")]
    EmptyRenderLayout,
    /// Renderer adapter rejected configuration or processing.
    #[error(transparent)]
    Renderer(#[from] RendererError),
}

fn validate_decoded_frame(
    decoded: &DecodedFrame,
    format: AudioFormat,
) -> Result<(), SourceRuntimeError> {
    if decoded.audio.channels.len() != format.channel_count {
        return Err(SourceRuntimeError::DecodedFormatMismatch(format!(
            "expected {} channels, got {}",
            format.channel_count,
            decoded.audio.channels.len()
        )));
    }
    if decoded
        .audio
        .channels
        .iter()
        .any(|channel| channel.len() != decoded.audio.frame_count)
    {
        return Err(SourceRuntimeError::DecodedFormatMismatch(
            "channel length does not match frame_count".to_owned(),
        ));
    }
    if decoded
        .audio
        .channels
        .iter()
        .flatten()
        .any(|sample| !sample.is_finite())
    {
        return Err(SourceRuntimeError::DecodedFormatMismatch(
            "decoded PCM contains non-finite samples".to_owned(),
        ));
    }
    Ok(())
}

fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}
