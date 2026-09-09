use aurora_core::AudioBlock;
use thiserror::Error;

use crate::{
    evaluate_paired_mpegh_candidate, reference_roles, render_mpegh_hoa_candidate,
    render_pure_mpegh_hoa_candidate, MpeghConformancePolicy, MpeghHoaCandidateError,
    MpeghPairedEvidence, MpeghPairedGateError, MpeghPairedPlaybackDecision,
    MpeghRoleConformanceError,
};

/// Candidate render plus the evidence-gated playback decision for one admitted
/// MPEG-H HOA access unit.
///
/// `candidate` is always retained for diagnostics. The caller must obey
/// `playback.evidence.choice`; a failed comparison does not authorize Aurora
/// candidate playback.
#[derive(Debug, Clone, PartialEq)]
pub struct MpeghHoaCandidateDecision {
    pub candidate: AudioBlock,
    pub playback: MpeghPairedPlaybackDecision,
}

/// Backward-compatible pure-HOA evidence gate.
pub fn evaluate_pure_mpegh_hoa_candidate(
    pair: &MpeghPairedEvidence,
    regularization: f64,
    policy: MpeghConformancePolicy,
) -> Result<MpeghHoaCandidateDecision, MpeghHoaCandidateGateError> {
    let reference = pair
        .reference
        .as_ref()
        .ok_or(MpeghHoaCandidateGateError::MissingReferenceRender)?;
    let candidate = render_pure_mpegh_hoa_candidate(pair, regularization)?;
    evaluate_candidate(pair, candidate, reference.sample_rate, policy)
}

/// Evidence gate for the currently admitted HOA scene classes: pure HOA and
/// Bed+HOA. The candidate is already produced in libmpegh's exact reference
/// speaker order; semantic roles are still resolved and checked before the
/// comparison so ambiguous layouts fail closed.
pub fn evaluate_mpegh_hoa_candidate(
    pair: &MpeghPairedEvidence,
    regularization: f64,
    policy: MpeghConformancePolicy,
) -> Result<MpeghHoaCandidateDecision, MpeghHoaCandidateGateError> {
    let reference = pair
        .reference
        .as_ref()
        .ok_or(MpeghHoaCandidateGateError::MissingReferenceRender)?;
    let candidate = render_mpegh_hoa_candidate(pair, regularization)?;
    evaluate_candidate(pair, candidate, reference.sample_rate, policy)
}

fn evaluate_candidate(
    pair: &MpeghPairedEvidence,
    candidate: AudioBlock,
    sample_rate: u32,
    policy: MpeghConformancePolicy,
) -> Result<MpeghHoaCandidateDecision, MpeghHoaCandidateGateError> {
    let roles = reference_roles(&pair.reference_layout, candidate.channels.len())?;
    let playback = evaluate_paired_mpegh_candidate(
        pair,
        &candidate,
        sample_rate,
        &roles,
        policy,
    )?;
    Ok(MpeghHoaCandidateDecision { candidate, playback })
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum MpeghHoaCandidateGateError {
    #[error("paired MPEG-H evidence has no libmpegh reference render")]
    MissingReferenceRender,
    #[error(transparent)]
    Candidate(#[from] MpeghHoaCandidateError),
    #[error(transparent)]
    Roles(#[from] MpeghRoleConformanceError),
    #[error(transparent)]
    Playback(#[from] MpeghPairedGateError),
}
