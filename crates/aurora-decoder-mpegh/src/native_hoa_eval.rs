#![cfg(feature = "native-mpegh")]

use thiserror::Error;

use crate::{
    decode_native_paired_chunk, evaluate_pure_mpegh_hoa_candidate, MpeghConformancePolicy,
    MpeghHoaCandidateDecision, MpeghHoaCandidateGateError, MpeghNativePairError,
    MpeghPairedEvidence, NativeMpeghDecoder,
};

#[derive(Debug)]
pub struct NativeMpeghHoaEvaluation {
    pub evidence: MpeghPairedEvidence,
    pub decision: MpeghHoaCandidateDecision,
}

/// Decode and evaluate a pure-HOA MPEG-H access unit in one native decode pass.
///
/// The compressed input is decoded once. The same access unit yields the
/// transport scene, observed ACN/N3D coefficients, libmpegh reference PCM,
/// Aurora candidate HOA render, and the conformance-gated playback decision.
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

    let decision = evaluate_pure_mpegh_hoa_candidate(&evidence, regularization, policy)?;
    Ok(Some(NativeMpeghHoaEvaluation { evidence, decision }))
}

#[derive(Debug, Error)]
pub enum NativeMpeghHoaEvaluationError {
    #[error(transparent)]
    NativePair(#[from] MpeghNativePairError),
    #[error(transparent)]
    CandidateGate(#[from] MpeghHoaCandidateGateError),
}
