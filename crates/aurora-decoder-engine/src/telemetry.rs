use aurora_decoder_api::{DecodedFrame, DecoderError};

use crate::catalog::{BackendId, CodecId};

/// Cumulative health counters for one Aurora decoder-engine instance.
///
/// Counters are intentionally codec-agnostic and cheap enough to update on the
/// realtime control path. Native adapter-specific resynchronization counters are
/// copied into the snapshot by `AuroraDecoderEngine::telemetry()`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EngineTelemetry {
    pub input_chunks: u64,
    pub poll_calls: u64,
    pub input_bytes: u64,
    pub frames_emitted: u64,
    pub samples_emitted: u64,
    pub objects_emitted: u64,
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
        self.frames_emitted = self.frames_emitted.saturating_add(1);
        self.samples_emitted = self
            .samples_emitted
            .saturating_add(frame.audio.frame_count as u64);
        self.objects_emitted = self
            .objects_emitted
            .saturating_add(frame.objects.len() as u64);
        if frame.audio.discontinuity {
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
}
