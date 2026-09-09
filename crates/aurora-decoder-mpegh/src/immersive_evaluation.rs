use aurora_core::AudioBlock;
use thiserror::Error;

use crate::{
    candidate_family_for_domain, evaluate_exact_mpegh_scene_candidate,
    evaluate_mpegh_hoa_candidate, evaluate_mpegh_non_hoa_candidate,
    mpegh_reference_audio_block, MpeghCandidateDispatchError, MpeghCandidateFamily,
    MpeghConformancePolicy, MpeghEvidenceGateOutcome, MpeghPairedEvidence,
    MpeghPairedPlaybackDecision, MpeghPlaybackChoice, MpeghRenderedPcmError,
};

/// Evidence-gated Aurora candidate decision for one MPEG-H access unit.
///
/// This type is intentionally available in baseline Rust builds: candidate
/// dispatch, rendering and fallback policy can therefore be tested without
/// loading the native libmpegh FFI boundary.
#[derive(Debug)]
pub struct MpeghImmersiveDecision {
    pub family: MpeghCandidateFamily,
    /// Diagnostic Aurora candidate. Absent when candidate construction itself
    /// failed and the paired libmpegh reference was selected fail-closed.
    pub candidate: Option<AudioBlock>,
    pub playback: MpeghPairedPlaybackDecision,
}

impl MpeghImmersiveDecision {
    /// Borrow exactly the block authorized by the evidence decision.
    pub fn selected_audio(&self) -> Result<&AudioBlock, MpeghPlaybackSelectionError> {
        match self.playback.evidence.choice {
            MpeghPlaybackChoice::AuroraCandidate => self
                .candidate
                .as_ref()
                .ok_or(MpeghPlaybackSelectionError::MissingAuroraCandidate),
            MpeghPlaybackChoice::LibmpeghReference => self
                .playback
                .reference_fallback
                .as_ref()
                .ok_or(MpeghPlaybackSelectionError::MissingReferenceFallback),
        }
    }

    /// Consume the decision and return exactly the block authorized for
    /// playback, preventing a caller from bypassing the evidence choice.
    pub fn into_selected_audio(self) -> Result<AudioBlock, MpeghPlaybackSelectionError> {
        match self.playback.evidence.choice {
            MpeghPlaybackChoice::AuroraCandidate => self
                .candidate
                .ok_or(MpeghPlaybackSelectionError::MissingAuroraCandidate),
            MpeghPlaybackChoice::LibmpeghReference => self
                .playback
                .reference_fallback
                .ok_or(MpeghPlaybackSelectionError::MissingReferenceFallback),
        }
    }
}

/// Evaluate already-paired MPEG-H evidence through the strongest admitted
/// Aurora renderer for its exact Transport V2 domain.
///
/// Candidate-rendering failures select the valid same-access-unit libmpegh
/// reference rather than becoming playback failures. Only failure to obtain a
/// trustworthy reference/dispatch artifact is returned as an error.
pub fn evaluate_mpegh_immersive_evidence(
    evidence: &MpeghPairedEvidence,
    regularization: f64,
    policy: MpeghConformancePolicy,
) -> Result<MpeghImmersiveDecision, MpeghImmersiveEvaluationError> {
    let family = candidate_family_for_domain(evidence.scene.domain)?;
    match family {
        MpeghCandidateFamily::NonHoa => match evaluate_mpegh_non_hoa_candidate(evidence, policy) {
            Ok(decision) => Ok(MpeghImmersiveDecision {
                family,
                candidate: Some(decision.candidate),
                playback: decision.playback,
            }),
            Err(error) => reference_fallback(evidence, family, error.to_string()),
        },
        MpeghCandidateFamily::Hoa => {
            match evaluate_mpegh_hoa_candidate(evidence, regularization, policy) {
                Ok(decision) => Ok(MpeghImmersiveDecision {
                    family,
                    candidate: Some(decision.candidate),
                    playback: decision.playback,
                }),
                Err(error) => reference_fallback(evidence, family, error.to_string()),
            }
        }
        MpeghCandidateFamily::ExactScene => {
            match evaluate_exact_mpegh_scene_candidate(evidence, regularization, policy) {
                Ok(decision) => Ok(MpeghImmersiveDecision {
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
) -> Result<MpeghImmersiveDecision, MpeghImmersiveEvaluationError> {
    let reference = evidence
        .reference
        .as_ref()
        .ok_or(MpeghImmersiveEvaluationError::MissingReferenceRender)?;
    let source = &evidence.scene.frame.decoded.audio;
    let fallback = mpegh_reference_audio_block(
        reference,
        source.presentation_time_seconds,
        source.discontinuity,
    )?;
    Ok(MpeghImmersiveDecision {
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

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MpeghPlaybackSelectionError {
    #[error("MPEG-H evidence selected the Aurora candidate but no candidate block is present")]
    MissingAuroraCandidate,
    #[error("MPEG-H evidence selected the libmpegh reference but no reference fallback block is present")]
    MissingReferenceFallback,
}

#[derive(Debug, Error)]
pub enum MpeghImmersiveEvaluationError {
    #[error(transparent)]
    Dispatch(#[from] MpeghCandidateDispatchError),
    #[error("paired MPEG-H evidence has no libmpegh reference render for fail-closed playback")]
    MissingReferenceRender,
    #[error(transparent)]
    Reference(#[from] MpeghRenderedPcmError),
}

#[cfg(test)]
mod tests {
    use aurora_decoder_api::DecodedFrame;
    use aurora_spatial_ir_v2::{SpatialDecodedFrame, SpatialDomain, SpatialFrameMetadata};
    use aurora_spatial_transport_v2::{
        HoaGroupBinding, HoaSignalBinding, SpatialTransportFrame, TransportSceneDomain,
    };

    use crate::{MpeghRenderedPcm, MpeghSpeaker, MpeghSpeakerLayout};

    use super::*;

    fn block(value: f32) -> AudioBlock {
        AudioBlock {
            channels: vec![vec![value; 2]],
            frame_count: 2,
            presentation_time_seconds: 0.5,
            discontinuity: true,
        }
    }

    fn outcome(choice: MpeghPlaybackChoice) -> MpeghEvidenceGateOutcome {
        MpeghEvidenceGateOutcome {
            choice,
            report: None,
            rejection_reason: None,
        }
    }

    fn hoa_pair_without_coefficients() -> MpeghPairedEvidence {
        MpeghPairedEvidence {
            scene: SpatialTransportFrame {
                frame: SpatialDecodedFrame {
                    decoded: DecodedFrame {
                        audio: block(0.0),
                        objects: Vec::new(),
                    },
                    spatial: SpatialFrameMetadata {
                        domain: SpatialDomain::DiscreteBed,
                        bed_signals: Vec::new(),
                        object_signals: Vec::new(),
                        object_updates: Vec::new(),
                    },
                },
                domain: TransportSceneDomain::HoaTransport,
                bed_signals: Vec::new(),
                hoa_signals: vec![HoaSignalBinding {
                    pcm_channel_index: 0,
                    transport_index: 0,
                }],
                hoa_groups: vec![HoaGroupBinding {
                    group_index: 0,
                    transport_indices: vec![0],
                    order: 0,
                    fixed_position: false,
                    priority: 0,
                    uses_nfc: false,
                    nfc_reference_distance_raw: None,
                    matrix: None,
                    screen_relative: false,
                }],
                codec_metadata: Vec::new(),
            },
            hoa_coefficients: None,
            reference: Some(MpeghRenderedPcm {
                bytes: vec![0_u8; 4],
                bit_depth: 16,
                channel_count: 1,
                frame_count: 2,
                sample_rate: 48_000,
            }),
            reference_layout: MpeghSpeakerLayout {
                cicp_index: 0,
                layout_code: 0,
                speakers: vec![MpeghSpeaker {
                    is_lfe: false,
                    azimuth_degrees: 0,
                    elevation_degrees: 0,
                }],
            },
        }
    }

    #[test]
    fn selected_audio_obeys_aurora_candidate_choice() {
        let decision = MpeghImmersiveDecision {
            family: MpeghCandidateFamily::NonHoa,
            candidate: Some(block(0.25)),
            playback: MpeghPairedPlaybackDecision {
                evidence: outcome(MpeghPlaybackChoice::AuroraCandidate),
                reference_fallback: Some(block(0.75)),
            },
        };
        assert_eq!(decision.selected_audio().unwrap().channels[0][0], 0.25);
    }

    #[test]
    fn selected_audio_obeys_reference_choice() {
        let decision = MpeghImmersiveDecision {
            family: MpeghCandidateFamily::ExactScene,
            candidate: Some(block(0.25)),
            playback: MpeghPairedPlaybackDecision {
                evidence: outcome(MpeghPlaybackChoice::LibmpeghReference),
                reference_fallback: Some(block(0.75)),
            },
        };
        assert_eq!(decision.selected_audio().unwrap().channels[0][0], 0.75);
    }

    #[test]
    fn unsupported_hoa_candidate_uses_same_access_unit_reference() {
        let decision = evaluate_mpegh_immersive_evidence(
            &hoa_pair_without_coefficients(),
            1.0e-6,
            MpeghConformancePolicy::near_reference(),
        )
        .unwrap();
        assert_eq!(decision.family, MpeghCandidateFamily::Hoa);
        assert!(decision.candidate.is_none());
        assert_eq!(
            decision.playback.evidence.choice,
            MpeghPlaybackChoice::LibmpeghReference
        );
        assert!(decision.playback.reference_fallback.is_some());
    }
}
