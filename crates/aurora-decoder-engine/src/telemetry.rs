use aurora_decoder_api::{DecodedFrame, DecoderError};

use crate::catalog::{BackendId, CodecId};
use crate::spatial_ir::SpatialDecodedFrame;
use crate::AuroraDecoderEngine;

/// Cumulative health counters for one Aurora decoder-engine instance.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EngineTelemetry {
    pub input_chunks: u64,
    pub poll_calls: u64,
    pub input_bytes: u64,
    pub frames_emitted: u64,
    pub samples_emitted: u64,
    pub objects_emitted: u64,
    pub spatial_updates_emitted: u64,
    pub discontinuities: u64,
    pub backend_selections: u64,
    pub backend_switches: u64,
    pub unavailable_errors: u64,
    pub unsupported_errors: u64,
    pub native_decode_errors: u64,
    pub external_worker_errors: u64,
    pub ac4_dropped_bytes: u64,
    pub dts_dropped_bytes: u64,
    pub active_backend: Option<BackendId>,
    pub active_codec: Option<CodecId>,
}

/// Allocation-free JOC state for the live health path. `speaker_render_active`
/// is live-only; the remaining render details are the latest successful
/// OpenJOC observation in the current decoder epoch and survive EOF retirement.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JocDecoderHealth {
    pub codec_classified_joc: bool,
    pub speaker_render_active: bool,
    pub channel_count: Option<usize>,
    pub latency_samples: Option<usize>,
    pub object_count: Option<u16>,
    pub complexity_index: Option<u8>,
    pub fallback_present: bool,
    pub last_decode_time_us: Option<u64>,
    pub last_render_time_us: Option<u64>,
    pub last_total_time_us: Option<u64>,
    pub max_total_time_us: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JocTimingHealth {
    pub last_decode_time_us: Option<u64>,
    pub last_render_time_us: Option<u64>,
    pub last_total_time_us: Option<u64>,
    pub max_total_time_us: Option<u64>,
}

impl AuroraDecoderEngine {
    pub fn joc_health(&self) -> JocDecoderHealth {
        let active_render = self.open.joc_render_info();
        let observed_render = active_render.or_else(|| self.open.last_joc_render_info());
        JocDecoderHealth {
            codec_classified_joc: self.open.detected_codec().map(CodecId::from)
                == Some(CodecId::Eac3Joc),
            speaker_render_active: active_render.is_some(),
            channel_count: observed_render.map(|info| info.channel_count),
            latency_samples: observed_render.map(|info| info.latency_samples),
            object_count: observed_render.and_then(|info| info.object_count),
            complexity_index: observed_render.and_then(|info| info.complexity_index),
            fallback_present: self.open.last_joc_error().is_some(),
            last_decode_time_us: observed_render.and_then(|info| info.last_decode_time_us),
            last_render_time_us: observed_render.and_then(|info| info.last_render_time_us),
            last_total_time_us: observed_render.and_then(|info| info.last_total_time_us),
            max_total_time_us: observed_render.and_then(|info| info.max_total_time_us),
        }
    }

    pub fn joc_timing_health(&self) -> JocTimingHealth {
        let render = self.open.last_joc_render_info();
        JocTimingHealth {
            last_decode_time_us: render.and_then(|info| info.last_decode_time_us),
            last_render_time_us: render.and_then(|info| info.last_render_time_us),
            last_total_time_us: render.and_then(|info| info.last_total_time_us),
            max_total_time_us: render.and_then(|info| info.max_total_time_us),
        }
    }
}

impl EngineTelemetry {
    pub(crate) fn observe_input(&mut self, input: &[u8]) {
        if input.is_empty() {
            self.poll_calls = self.poll_calls.saturating_add(1);
        } else {
            self.input_chunks = self.input_chunks.saturating_add(1);
            self.input_bytes = self.input_bytes.saturating_add(input.len() as u64);
        }
    }

    pub(crate) fn observe_frame(&mut self, frame: &DecodedFrame) {
        self.observe_audio(frame.audio.frame_count, frame.audio.discontinuity);
        self.objects_emitted = self
            .objects_emitted
            .saturating_add(frame.objects.len() as u64);
    }

    pub(crate) fn observe_spatial_frame(&mut self, frame: &SpatialDecodedFrame) {
        self.observe_audio(
            frame.decoded.audio.frame_count,
            frame.decoded.audio.discontinuity,
        );
        self.objects_emitted = self
            .objects_emitted
            .saturating_add(frame.spatial.object_signals.len() as u64);
        self.spatial_updates_emitted = self
            .spatial_updates_emitted
            .saturating_add(frame.spatial.object_updates.len() as u64);
    }

    fn observe_audio(&mut self, frame_count: usize, discontinuity: bool) {
        self.frames_emitted = self.frames_emitted.saturating_add(1);
        self.samples_emitted = self.samples_emitted.saturating_add(frame_count as u64);
        if discontinuity {
            self.discontinuities = self.discontinuities.saturating_add(1);
        }
    }

    pub(crate) fn observe_error(&mut self, error: &DecoderError) {
        match error {
            DecoderError::Unavailable(_) => {
                self.unavailable_errors = self.unavailable_errors.saturating_add(1)
            }
            DecoderError::UnsupportedInput(_) => {
                self.unsupported_errors = self.unsupported_errors.saturating_add(1)
            }
            DecoderError::Decode(_) => {
                self.native_decode_errors = self.native_decode_errors.saturating_add(1)
            }
            DecoderError::ExternalProcess(_) => {
                self.external_worker_errors = self.external_worker_errors.saturating_add(1)
            }
        }
    }

    pub(crate) fn observe_selection(
        &mut self,
        previous: Option<BackendId>,
        next: Option<BackendId>,
    ) {
        if previous == next {
            return;
        }
        if next.is_some() {
            self.backend_selections = self.backend_selections.saturating_add(1);
        }
        if previous.is_some() && next.is_some() {
            self.backend_switches = self.backend_switches.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spatial_ir::{
        CoordinateSpace, ObjectSignalBinding, SpatialDomain, SpatialFrameMetadata,
        SpatialObjectUpdate, SpatialPosition,
    };
    use crate::EngineConfig;
    use aurora_core::AudioBlock;

    #[test]
    fn errors_are_classified_without_collapsing_failure_domains() {
        let mut telemetry = EngineTelemetry::default();
        telemetry.observe_error(&DecoderError::Unavailable("off"));
        telemetry.observe_error(&DecoderError::UnsupportedInput("format"));
        telemetry.observe_error(&DecoderError::Decode("native".into()));
        telemetry.observe_error(&DecoderError::ExternalProcess("worker".into()));
        assert_eq!(telemetry.unavailable_errors, 1);
        assert_eq!(telemetry.unsupported_errors, 1);
        assert_eq!(telemetry.native_decode_errors, 1);
        assert_eq!(telemetry.external_worker_errors, 1);
    }

    #[test]
    fn first_backend_is_a_selection_not_a_switch() {
        let mut telemetry = EngineTelemetry::default();
        telemetry.observe_selection(None, Some(BackendId::OxideDtsCore));
        assert_eq!(telemetry.backend_selections, 1);
        assert_eq!(telemetry.backend_switches, 0);
        telemetry.observe_selection(Some(BackendId::OxideDtsCore), Some(BackendId::FfmpegWorker));
        assert_eq!(telemetry.backend_selections, 2);
        assert_eq!(telemetry.backend_switches, 1);
    }

    #[test]
    fn empty_engine_joc_health_is_fixed_size_and_has_no_claim() {
        let engine = AuroraDecoderEngine::new(EngineConfig::default());
        assert_eq!(engine.joc_health(), JocDecoderHealth::default());
        assert_eq!(engine.joc_timing_health(), JocTimingHealth::default());
        assert!(std::mem::size_of::<JocDecoderHealth>() <= 128);
        assert!(std::mem::size_of::<JocTimingHealth>() <= 64);
    }

    #[test]
    fn empty_engine_flush_is_a_noop() {
        let mut engine = AuroraDecoderEngine::new(EngineConfig::default());
        engine.flush_pending().unwrap();
    }

    #[test]
    fn spatial_telemetry_counts_signals_once_and_updates_separately() {
        let frame = SpatialDecodedFrame {
            decoded: DecodedFrame {
                audio: AudioBlock {
                    channels: vec![vec![0.0; 40]],
                    frame_count: 40,
                    presentation_time_seconds: 0.0,
                    discontinuity: true,
                },
                objects: Vec::new(),
            },
            spatial: SpatialFrameMetadata {
                domain: SpatialDomain::ObjectSignals,
                bed_signals: Vec::new(),
                object_signals: vec![ObjectSignalBinding {
                    id: "o0".into(),
                    pcm_channel_index: 0,
                }],
                object_updates: vec![
                    SpatialObjectUpdate {
                        object_id: "o0".into(),
                        active: true,
                        coordinate_space: CoordinateSpace::RoomNormalized,
                        position: SpatialPosition::Cartesian {
                            x: 0.5,
                            y: 0.5,
                            z: 0.0,
                        },
                        gain_db: 0.0,
                        spread: 0.0,
                        metadata_sample_offset: 0,
                        ramp_duration_samples: 0,
                        priority: Some(1.0),
                    },
                    SpatialObjectUpdate {
                        object_id: "o0".into(),
                        active: true,
                        coordinate_space: CoordinateSpace::RoomNormalized,
                        position: SpatialPosition::Cartesian {
                            x: 0.6,
                            y: 0.5,
                            z: 0.0,
                        },
                        gain_db: 0.0,
                        spread: 0.0,
                        metadata_sample_offset: 20,
                        ramp_duration_samples: 20,
                        priority: Some(1.0),
                    },
                ],
            },
        };
        let mut telemetry = EngineTelemetry::default();
        telemetry.observe_spatial_frame(&frame);
        assert_eq!(telemetry.frames_emitted, 1);
        assert_eq!(telemetry.samples_emitted, 40);
        assert_eq!(telemetry.objects_emitted, 1);
        assert_eq!(telemetry.spatial_updates_emitted, 2);
        assert_eq!(telemetry.discontinuities, 1);
    }
}
