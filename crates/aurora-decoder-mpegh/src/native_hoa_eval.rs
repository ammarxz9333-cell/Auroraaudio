#![cfg(feature = "native-mpegh")]

use aurora_core::AudioBlock;
use thiserror::Error;

use crate::{
    candidate_family_for_domain, decode_native_paired_chunk,
    evaluate_exact_mpegh_scene_candidate, evaluate_mpegh_hoa_candidate,
    mpegh_reference_audio_block, MpeghCandidateDispatchError, MpeghCandidateFamily,
    MpeghConformancePolicy, MpeghEvidenceGateOutcome, MpeghHoaCandidateDecision,
    MpeghHoaCandidateGateError, MpeghNativePairError, MpeghPairedEvidence,
    MpeghPairedPlaybackDecision, MpeghPlaybackChoice, MpeghRenderedPcmError,
    NativeMpeghDecoder,
};

/// Uniform Aurora candidate decision for one native MPEG-H immersive access
/// unit. `candidate` is absent when Aurora deliberately fails closed and the
/// paired libmpegh reference is selected instead.
#[derive(Debug)]
pub struct NativeMpeghImmersiveDecision {
    pub family: MpeghCandidateFamily,
    pub candidate: Option<AudioBlock>,
    pub playback: MpeghPairedPlaybackDecision,
}

#[derive(Debug)]
pub struct NativeMpeghImmersiveEvaluation {
    pub evidence: MpeghPairedEvidence,
    pub decision: NativeMpeghImmersiveDecision,
}

/// Evaluate already-paired MPEG-H evidence through the strongest currently
/// admitted Aurora candidate path for its exact transport domain.
///
/// * pure HOA / Bed+HOA -> HOA candidate renderer
/// * Objects+HOA / Bed+Objects+HOA -> exact-per-sample object + HOA renderer
///
/// Candidate-rendering failures are not playback failures when the same access
/// unit has a valid libmpegh reference render. They select that reference with a
/// diagnostic rejection reason. Non-HOA domains still fail dispatch because
/// this evaluator is specifically anchored to the HOA coefficient observer.
pub fn evaluate_native_mpegh_immersive_evidence(
    evidence: &MpeghPairedEvidence,
    regularization: f64,
    policy: MpeghConformancePolicy,
) -> Result<NativeMpeghImmersiveDecision, NativeMpeghImmersiveEvaluationError> {
    let family = candidate_family_for_domain(evidence.scene.domain)?;
    match family {
        MpeghCandidateFamily::Hoa => {
            match evaluate_mpegh_hoa_candidate(evidence, regularization, policy) {
                Ok(decision) => Ok(NativeMpeghImmersiveDecision {
                    family,
                    candidate: Some(decision.candidate),
                    playback: decision.playback,
                }),
                Err(error) => reference_fallback(evidence, family, error.to_string()),
            }
        }
        MpeghCandidateFamily::ExactScene => {
            match evaluate_exact_mpegh_scene_candidate(evidence, regularization, policy) {
                Ok(decision) => Ok(NativeMpeghImmersiveDecision {
                    family,
                    candidate: Some(decision.candidate),
                    playback: decision.playback,
                }),
                Err(error) => reference_fallback(evidence, family, error.to_string()),
            }
        }
    }
}

fn reference_fallback(
    evidence: &MpeghPairedEvidence,
    family: MpeghCandidateFamily,
    reason: String,
) -> Result<NativeMpeghImmersiveDecision, NativeMpeghImmersiveEvaluationError> {
    let reference = evidence
        .reference
        .as_ref()
        .ok_or(NativeMpeghImmersiveEvaluationError::MissingReferenceRender)?;
    let source = &evidence.scene.frame.decoded.audio;
    let fallback = mpegh_reference_audio_block(
        reference,
        source.presentation_time_seconds,
        source.discontinuity,
    )?;
    Ok(NativeMpeghImmersiveDecision {
        family,
        candidate: None,
        playback: MpeghPairedPlaybackDecision {
            evidence: MpeghEvidenceGateOutcome {
                choice: MpeghPlaybackChoice::LibmpeghReference,
                report: None,
                rejection_reason: Some(format!(
                    "Aurora {family:?} candidate was not admitted: {reason}"
                )),
            },
            reference_fallback: Some(fallback),
        },
    })
}

/// Decode and evaluate one admitted MPEG-H immersive access unit in one native
/// pass. Scene transport, observed ACN/N3D HOA coefficients, libmpegh reference
/// PCM, Aurora candidate rendering, and evidence-gated playback all refer to the
/// same successful decoder execute.
pub fn decode_and_evaluate_native_mpegh_immersive_chunk(
    decoder: &mut NativeMpeghDecoder,
    input: &[u8],
    presentation_time_seconds: f64,
    discontinuity: bool,
    regularization: f64,
    policy: MpeghConformancePolicy,
) -> Result<Option<NativeMpeghImmersiveEvaluation>, NativeMpeghImmersiveEvaluationError> {
    let Some(evidence) = decode_native_paired_chunk(
        decoder,
        input,
        presentation_time_seconds,
        discontinuity,
    )? else {
        return Ok(None);
    };

    let decision = evaluate_native_mpegh_immersive_evidence(&evidence, regularization, policy)?;
    Ok(Some(NativeMpeghImmersiveEvaluation { evidence, decision }))
}

/// Backward-compatible HOA-family evaluation artifact.
#[derive(Debug)]
pub struct NativeMpeghHoaEvaluation {
    pub evidence: MpeghPairedEvidence,
    pub decision: MpeghHoaCandidateDecision,
}

/// Backward-compatible pure-HOA / Bed+HOA native entry point. Object-bearing
/// immersive scenes are intentionally not silently upgraded through this old
/// API; callers that want domain dispatch should use
/// [`decode_and_evaluate_native_mpegh_immersive_chunk`].
pub fn decode_and_evaluate_native_mpegh_hoa_chunk(
    decoder: &mut NativeMpeghDecoder,
    input: &[u8],
    presentation_time_seconds: f64,
    discontinuity: bool,
    regularization: f64,
    policy: MpeghConformancePolicy,
) -> Result<Option<NativeMpeghHoaEvaluation>, NativeMpeghHoaEvaluationError> {
    let Some(evidence) = decode_native_paired_chunk(
        decoder,
        input,
        presentation_time_seconds,
        discontinuity,
    )? else {
        return Ok(None);
    };

    let decision = evaluate_mpegh_hoa_candidate(&evidence, regularization, policy)?;
    Ok(Some(NativeMpeghHoaEvaluation { evidence, decision }))
}

#[derive(Debug, Error)]
pub enum NativeMpeghImmersiveEvaluationError {
    #[error(transparent)]
    NativePair(#[from] MpeghNativePairError),
    #[error(transparent)]
    Dispatch(#[from] MpeghCandidateDispatchError),
    #[error("paired MPEG-H evidence has no libmpegh reference render for fail-closed playback")]
    MissingReferenceRender,
    #[error(transparent)]
    Reference(#[from] MpeghRenderedPcmError),
}

#[derive(Debug, Error)]
pub enum NativeMpeghHoaEvaluationError {
    #[error(transparent)]
    NativePair(#[from] MpeghNativePairError),
    #[error(transparent)]
    CandidateGate(#[from] MpeghHoaCandidateGateError),
}
