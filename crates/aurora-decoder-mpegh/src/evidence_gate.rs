use aurora_core::{AudioBlock, ChannelRole};

use crate::{
    compare_mpegh_render_to_reference_by_role, MpeghConformancePolicy,
    MpeghConformanceReport, MpeghRenderedPcm, MpeghRenderedPcmError, MpeghSpeakerLayout,
};

/// Deterministic playback decision for one MPEG-H access unit after Aurora has
/// produced a candidate speaker render and libmpegh has produced its reference
/// render from the same compressed execute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpeghPlaybackChoice {
    AuroraCandidate,
    LibmpeghReference,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MpeghEvidenceGateOutcome {
    pub choice: MpeghPlaybackChoice,
    /// Present when a role-aligned numerical comparison could be completed.
    pub report: Option<MpeghConformanceReport>,
    /// Human-readable fail-closed reason when the reference path wins before
    /// or after the numerical gate.
    pub rejection_reason: Option<String>,
}

/// Evaluate one Aurora speaker render against the libmpegh reference.
///
/// The reference is the safety anchor. Any inability to prove semantic channel
/// alignment, any geometry mismatch, or any failed numerical threshold selects
/// the reference render. An invalid reference itself is returned as an error
/// because there is then no safe fallback artifact to play.
pub fn evaluate_mpegh_render_evidence(
    candidate: &AudioBlock,
    candidate_sample_rate: u32,
    candidate_roles: &[ChannelRole],
    reference: &MpeghRenderedPcm,
    reference_layout: &MpeghSpeakerLayout,
    policy: MpeghConformancePolicy,
) -> Result<MpeghEvidenceGateOutcome, MpeghRenderedPcmError> {
    reference.validate()?;

    if candidate_sample_rate != reference.sample_rate {
        return Ok(reference_wins(format!(
            "sample-rate mismatch: Aurora={} Hz, libmpegh={} Hz",
            candidate_sample_rate, reference.sample_rate
        )));
    }
    if candidate.frame_count != reference.frame_count {
        return Ok(reference_wins(format!(
            "frame-count mismatch: Aurora={}, libmpegh={}",
            candidate.frame_count, reference.frame_count
        )));
    }
    if candidate.channels.len() != candidate_roles.len() {
        return Ok(reference_wins(format!(
            "Aurora output has {} channels but {} semantic roles",
            candidate.channels.len(),
            candidate_roles.len()
        )));
    }

    match compare_mpegh_render_to_reference_by_role(
        &candidate.channels,
        candidate_roles,
        reference,
        reference_layout,
        policy,
    ) {
        Ok(report) if report.passed => Ok(MpeghEvidenceGateOutcome {
            choice: MpeghPlaybackChoice::AuroraCandidate,
            report: Some(report),
            rejection_reason: None,
        }),
        Ok(report) => Ok(MpeghEvidenceGateOutcome {
            choice: MpeghPlaybackChoice::LibmpeghReference,
            report: Some(report),
            rejection_reason: Some(
                "Aurora render failed the configured role-aligned numerical evidence gate".into(),
            ),
        }),
        Err(error) => Ok(reference_wins(format!(
            "role-aligned comparison unavailable: {error}"
        ))),
    }
}

/// Convert the validated libmpegh reference render into Aurora's planar block
/// representation for immediate fail-closed playback.
pub fn mpegh_reference_audio_block(
    reference: &MpeghRenderedPcm,
    presentation_time_seconds: f64,
    discontinuity: bool,
) -> Result<AudioBlock, MpeghRenderedPcmError> {
    let channels = reference.decode_planar_f32()?;
    Ok(AudioBlock {
        channels,
        frame_count: reference.frame_count,
        presentation_time_seconds,
        discontinuity,
    })
}

fn reference_wins(reason: String) -> MpeghEvidenceGateOutcome {
    MpeghEvidenceGateOutcome {
        choice: MpeghPlaybackChoice::LibmpeghReference,
        report: None,
        rejection_reason: Some(reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MpeghSpeaker, MpeghSpeakerLayout};

    fn stereo_reference() -> (MpeghRenderedPcm, MpeghSpeakerLayout) {
        let samples = [
            [16_384_i16, -16_384_i16],
            [8_192, -8_192],
            [-4_096, 4_096],
        ];
        let mut bytes = Vec::new();
        for frame in samples {
            bytes.extend_from_slice(&frame[0].to_le_bytes());
            bytes.extend_from_slice(&frame[1].to_le_bytes());
        }
        (
            MpeghRenderedPcm {
                bytes,
                bit_depth: 16,
                channel_count: 2,
                frame_count: samples.len(),
                sample_rate: 48_000,
            },
            MpeghSpeakerLayout {
                cicp_index: 2,
                layout_code: 0,
                speakers: Vec::new(),
            },
        )
    }

    #[test]
    fn role_aligned_reference_quality_candidate_wins() {
        let (reference, layout) = stereo_reference();
        let planar = reference.decode_planar_f32().unwrap();
        let candidate = AudioBlock {
            channels: vec![planar[1].clone(), planar[0].clone()],
            frame_count: reference.frame_count,
            presentation_time_seconds: 0.0,
            discontinuity: false,
        };
        let roles = vec![ChannelRole::FrontRight, ChannelRole::FrontLeft];
        let outcome = evaluate_mpegh_render_evidence(
            &candidate,
            48_000,
            &roles,
            &reference,
            &layout,
            MpeghConformancePolicy::near_reference(),
        )
        .unwrap();
        assert_eq!(outcome.choice, MpeghPlaybackChoice::AuroraCandidate);
        assert!(outcome.report.unwrap().passed);
    }

    #[test]
    fn numerically_wrong_candidate_falls_back_to_reference() {
        let (reference, layout) = stereo_reference();
        let mut planar = reference.decode_planar_f32().unwrap();
        for sample in &mut planar[0] {
            *sample = -*sample;
        }
        let candidate = AudioBlock {
            channels: planar,
            frame_count: reference.frame_count,
            presentation_time_seconds: 0.0,
            discontinuity: false,
        };
        let roles = vec![ChannelRole::FrontLeft, ChannelRole::FrontRight];
        let outcome = evaluate_mpegh_render_evidence(
            &candidate,
            48_000,
            &roles,
            &reference,
            &layout,
            MpeghConformancePolicy::near_reference(),
        )
        .unwrap();
        assert_eq!(outcome.choice, MpeghPlaybackChoice::LibmpeghReference);
        assert!(!outcome.report.unwrap().passed);
    }

    #[test]
    fn unresolved_reference_layout_falls_back_without_fake_verdict() {
        let (reference, _) = stereo_reference();
        let candidate = AudioBlock {
            channels: reference.decode_planar_f32().unwrap(),
            frame_count: reference.frame_count,
            presentation_time_seconds: 0.0,
            discontinuity: false,
        };
        let layout = MpeghSpeakerLayout {
            cicp_index: 0,
            layout_code: 0,
            speakers: vec![
                MpeghSpeaker {
                    is_lfe: false,
                    azimuth_degrees: 60,
                    elevation_degrees: 0,
                },
                MpeghSpeaker {
                    is_lfe: false,
                    azimuth_degrees: -60,
                    elevation_degrees: 0,
                },
            ],
        };
        let roles = vec![ChannelRole::FrontLeft, ChannelRole::FrontRight];
        let outcome = evaluate_mpegh_render_evidence(
            &candidate,
            48_000,
            &roles,
            &reference,
            &layout,
            MpeghConformancePolicy::near_reference(),
        )
        .unwrap();
        assert_eq!(outcome.choice, MpeghPlaybackChoice::LibmpeghReference);
        assert!(outcome.report.is_none());
    }

    #[test]
    fn reference_pcm_converts_to_planar_audio_block() {
        let (reference, _) = stereo_reference();
        let block = mpegh_reference_audio_block(&reference, 1.25, true).unwrap();
        assert_eq!(block.channels.len(), 2);
        assert_eq!(block.frame_count, reference.frame_count);
        assert_eq!(block.presentation_time_seconds, 1.25);
        assert!(block.discontinuity);
    }
}
