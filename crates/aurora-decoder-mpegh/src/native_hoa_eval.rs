#![cfg(feature = "native-mpegh")]

use thiserror::Error;

use crate::{
    decode_native_paired_chunk, evaluate_mpegh_hoa_candidate,
    evaluate_mpegh_immersive_evidence, MpeghConformancePolicy, MpeghHoaCandidateDecision,
    MpeghHoaCandidateGateError, MpeghImmersiveDecision, MpeghImmersiveEvaluationError,
    MpeghNativePairError, MpeghPairedEvidence, MpeghPlaybackSelectionError,
    NativeMpeghDecoder,
};

/// Backward-compatible name for the now baseline-testable immersive decision.
pub type NativeMpeghImmersiveDecision = MpeghImmersiveDecision;
/// Backward-compatible name for evidence-safe playback selection failures.
pub type NativeMpeghPlaybackSelectionError = MpeghPlaybackSelectionError;

#[derive(Debug)]
pub struct NativeMpeghImmersiveEvaluation {
    pub evidence: MpeghPairedEvidence,
    pub decision: MpeghImmersiveDecision,
}

/// Evaluate already-paired evidence through Aurora's baseline Rust dispatcher.
/// The native feature is not required for the policy/render decision itself;
/// this wrapper preserves the previous native API surface for callers already
/// using it.
pub fn evaluate_native_mpegh_immersive_evidence(
    evidence: &MpeghPairedEvidence,
    regularization: f64,
    policy: MpeghConformancePolicy,
) -> Result<MpeghImmersiveDecision, NativeMpeghImmersiveEvaluationError> {
    evaluate_mpegh_immersive_evidence(evidence, regularization, policy)
        .map_err(NativeMpeghImmersiveEvaluationError::Evaluation)
}

/// Decode and evaluate one admitted MPEG-H immersive access unit in one native
/// pass. The native layer only produces the paired evidence; domain dispatch,
/// candidate rendering, numerical gating and reference fallback are all shared
/// with baseline Rust builds.
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

    let decision = evaluate_mpegh_immersive_evidence(&evidence, regularization, policy)?;
    Ok(Some(NativeMpeghImmersiveEvaluation { evidence, decision }))
}

/// Backward-compatible HOA-family evaluation artifact.
#[derive(Debug)]
pub struct NativeMpeghHoaEvaluation {
    pub evidence: MpeghPairedEvidence,
    pub decision: MpeghHoaCandidateDecision,
}

/// Backward-compatible pure-HOA / Bed+HOA native entry point. Object-bearing
/// or non-HOA scenes are intentionally not silently upgraded through this old
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
    Evaluation(#[from] MpeghImmersiveEvaluationError),
}

#[derive(Debug, Error)]
pub enum NativeMpeghHoaEvaluationError {
    #[error(transparent)]
    NativePair(#[from] MpeghNativePairError),
    #[error(transparent)]
    CandidateGate(#[from] MpeghHoaCandidateGateError),
}
