#![cfg(feature = "native-mpegh")]

use aurora_decoder_api::DecoderError;
use aurora_decoder_mpegh::{
    evaluate_native_mpegh_immersive_evidence, pair_mpegh_external_evidence_with_hoa,
    MpeghConformancePolicy, NativeMpeghDecoder, NativeMpeghImmersiveEvaluation,
};

use crate::AuroraSuperDecoder;

impl AuroraSuperDecoder {
    /// Decode and evidence-gate one MPEG-H immersive access unit without
    /// decoding the compressed payload twice.
    ///
    /// The same successful libmpegh execute contributes all three evidence
    /// planes: Transport V2 scene material, post-spatial ACN/N3D HOA
    /// coefficients, and final speaker-rendered reference PCM. Aurora then
    /// dispatches to the strongest admitted candidate renderer for the scene
    /// domain and falls back to the paired reference whenever that candidate is
    /// unsupported or fails its numerical evidence gate.
    pub fn decode_and_evaluate_mpegh_immersive_chunk(
        &mut self,
        input: &[u8],
        regularization: f64,
        policy: MpeghConformancePolicy,
    ) -> Result<Option<NativeMpeghImmersiveEvaluation>, DecoderError> {
        let Some(external) = self.decode_mpegh_external_chunk(input)? else {
            return Ok(None);
        };

        // Take both companions immediately after the same native execute so no
        // later call can associate stale evidence with a different access unit.
        let reference = self.take_mpegh_reference_pcm();
        let hoa_coefficients = self
            .mpegh
            .as_mut()
            .and_then(NativeMpeghDecoder::take_hoa_coefficients);

        let sample_rate = u32::try_from(external.sample_rate)
            .ok()
            .filter(|rate| *rate > 0)
            .ok_or_else(|| {
                self.mpegh_discontinuity = true;
                DecoderError::Decode(format!(
                    "MPEG-H external frame reported invalid sample rate {}",
                    external.sample_rate
                ))
            })?;
        let presentation_time_seconds =
            self.mpegh_sample_cursor as f64 / f64::from(sample_rate);
        let fallback_frame_count = reference
            .as_ref()
            .map(|pcm| pcm.frame_count as u64)
            .unwrap_or(1024);

        let evidence = match pair_mpegh_external_evidence_with_hoa(
            &external,
            hoa_coefficients,
            reference,
            presentation_time_seconds,
            self.mpegh_discontinuity,
        ) {
            Ok(evidence) => evidence,
            Err(error) => {
                self.mpegh_sample_cursor = self
                    .mpegh_sample_cursor
                    .saturating_add(fallback_frame_count);
                self.mpegh_discontinuity = true;
                return Err(DecoderError::Decode(format!(
                    "MPEG-H paired immersive evidence construction failed: {error}"
                )));
            }
        };

        let consumed_frames = evidence.scene.frame.decoded.audio.frame_count as u64;
        self.mpegh_sample_cursor = self.mpegh_sample_cursor.saturating_add(consumed_frames);

        match evaluate_native_mpegh_immersive_evidence(&evidence, regularization, policy) {
            Ok(decision) => {
                self.mpegh_discontinuity = false;
                Ok(Some(NativeMpeghImmersiveEvaluation { evidence, decision }))
            }
            Err(error) => {
                // The access unit was consumed even though no safe evaluation
                // artifact could be produced. Mark the next successful frame as
                // a discontinuity, but keep the monotonic sample cursor.
                self.mpegh_discontinuity = true;
                Err(DecoderError::Decode(format!(
                    "MPEG-H immersive candidate evaluation failed: {error}"
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use aurora_decoder_engine::EngineConfig;

    use super::*;
    use crate::ActiveRoute;

    #[test]
    fn empty_input_initializes_native_route_without_emitting_fake_evidence() {
        let mut decoder = AuroraSuperDecoder::new(EngineConfig::default());
        let result = decoder
            .decode_and_evaluate_mpegh_immersive_chunk(
                &[],
                1.0e-6,
                MpeghConformancePolicy::near_reference(),
            )
            .unwrap();
        assert!(result.is_none());
        assert_eq!(decoder.active_route(), ActiveRoute::MpegHNative);
        assert_eq!(decoder.mpegh_sample_cursor, 0);
        assert!(decoder.mpegh_discontinuity);
    }
}
