use aurora_core::{AudioBlock, ChannelRole};
use thiserror::Error;

use crate::{
    evaluate_mpegh_render_evidence, mpegh_reference_audio_block, MpeghConformancePolicy,
    MpeghEvidenceGateOutcome, MpeghPairedEvidence, MpeghPlaybackChoice,
    MpeghRenderedPcmError,
};

/// Full decision artifact for one paired MPEG-H access unit.
///
/// When the evidence gate selects libmpegh, `reference_fallback` contains the
/// already-decoded planar block ready for fail-closed playback. No compressed
/// access unit is decoded again.
#[derive(Debug, Clone, PartialEq)]
pub struct MpeghPairedPlaybackDecision {
    pub evidence: MpeghEvidenceGateOutcome,
    pub reference_fallback: Option<AudioBlock>,
}

pub fn evaluate_paired_mpegh_candidate(
    pair: &MpeghPairedEvidence,
    candidate: &AudioBlock,
    candidate_sample_rate: u32,
    candidate_roles: &[ChannelRole],
    policy: MpeghConformancePolicy,
) -> Result<MpeghPairedPlaybackDecision, MpeghPairedGateError> {
    let reference = pair
        .reference
        .as_ref()
        .ok_or(MpeghPairedGateError::MissingReferenceRender)?;

    let evidence = evaluate_mpegh_render_evidence(
        candidate,
        candidate_sample_rate,
        candidate_roles,
        reference,
        &pair.reference_layout,
        policy,
    )?;

    let reference_fallback = if evidence.choice == MpeghPlaybackChoice::LibmpeghReference {
        let source_audio = &pair.scene.frame.decoded.audio;
        Some(mpegh_reference_audio_block(
            reference,
            source_audio.presentation_time_seconds,
            source_audio.discontinuity,
        )?)
    } else {
        None
    };

    Ok(MpeghPairedPlaybackDecision {
        evidence,
        reference_fallback,
    })
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum MpeghPairedGateError {
    #[error("paired MPEG-H evidence has no libmpegh reference render")]
    MissingReferenceRender,
    #[error(transparent)]
    Reference(#[from] MpeghRenderedPcmError),
}
