#![cfg(feature = "native-mpegh")]

use aurora_core::AudioBlock;
use thiserror::Error;

use crate::{
    candidate_family_for_domain, decode_native_paired_chunk,
    evaluate_exact_mpegh_scene_candidate, evaluate_mpegh_hoa_candidate,
    MpeghCandidateDispatchError, MpeghCandidateFamily, MpeghConformancePolicy,
    MpeghExactSceneError, MpeghHoaCandidateDecision, MpeghHoaCandidateGateError,
    MpeghNativePairError, MpeghPairedEvidence, MpeghPairedPlaybackDecision,
    NativeMpeghDecoder,
};

/// Uniform Aurora candidate decision for one native MPEG-H immersive access
/// unit. The compressed input has been decoded exactly once; `playback` still
/// decides whether the Aurora candidate is admitted or the paired libmpegh
/// reference must be used.
#[derive(Debug)]
pub struct NativeMpeghImmersiveDecision {
    pub family: MpeghCandidateFamily,
    pub candidate: AudioBlock,
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
/// Non-HOA domains fail closed because this evidence path is anchored to the
/// post-spatial ACN/N3D coefficient observer and same-access-unit libmpegh
/// speaker reference.
pub fn evaluate_native_mpegh_immersive_evidence(
    evidence: &MpeghPairedEvidence,
    regularization: f64,
    policy: MpeghConformancePolicy,
) -> Result<NativeMpeghImmersiveDecision, NativeMpeghImmersiveEvaluationError> {
    let family = candidate_family_for_domain(evidence.scene.domain)?;
    match family {
        MpeghCandidateFamily::Hoa => {
            let decision = evaluate_mpegh_hoa_candidate(evidence, regularization, policy)?;
            Ok(NativeMpeghImmersiveDecision {
                family,
                candidate: decision.candidate,
                playback: decision.playback,
            })
        }
        MpeghCandidateFamily::ExactScene => {
            let decision = evaluate_exact_mpegh_scene_candidate(evidence, regularization, policy)?;
            Ok(NativeMpeghImmersiveDecision {
                family,
                candidate: decision.candidate,
                playback: decision.playback,
            })
        }
    }
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
    #[error(transparent)]
    HoaCandidate(#[from] MpeghHoaCandidateGateError),
    #[error(transparent)]
    ExactScene(#[from] MpeghExactSceneError),
}

#[derive(Debug, Error)]
pub enum NativeMpeghHoaEvaluationError {
    #[error(transparent)]
    NativePair(#[from] MpeghNativePairError),
    #[error(transparent)]
    CandidateGate(#[from] MpeghHoaCandidateGateError),
}
