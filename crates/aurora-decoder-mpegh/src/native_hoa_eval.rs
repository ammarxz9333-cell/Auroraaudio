#![cfg(feature = "native-mpegh")]

use thiserror::Error;

use crate::{
    decode_native_paired_chunk, evaluate_mpegh_hoa_candidate, MpeghConformancePolicy,
    MpeghHoaCandidateDecision, MpeghHoaCandidateGateError, MpeghNativePairError,
    MpeghPairedEvidence, NativeMpeghDecoder,
};

#[derive(Debug)]
pub struct NativeMpeghHoaEvaluation {
    pub evidence: MpeghPairedEvidence,
    pub decision: MpeghHoaCandidateDecision,
}

/// Decode and evaluate an admitted MPEG-H HOA access unit in one native pass.
///
/// The compressed input is decoded once. The same access unit yields the
/// transport scene, observed ACN/N3D coefficients, libmpegh reference PCM,
/// Aurora candidate render, and the conformance-gated playback decision.
/// Current admitted scene domains are pure HOA and Bed+HOA; object-bearing HOA
/// scenes remain fail-closed until an independent Aurora object renderer is
/// wired into the same candidate mix.
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
pub enum NativeMpeghHoaEvaluationError {
    #[error(transparent)]
    NativePair(#[from] MpeghNativePairError),
    #[error(transparent)]
    CandidateGate(#[from] MpeghHoaCandidateGateError),
}
