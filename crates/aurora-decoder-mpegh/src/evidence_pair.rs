use thiserror::Error;

use aurora_spatial_transport_v2::SpatialTransportFrame;

use crate::{
    MpeghExternalFrame, MpeghRenderedPcm, MpeghRenderedPcmError, MpeghSpatialTransportError,
    MpeghSpeakerLayout,
};

/// One MPEG-H access unit represented as Aurora pre-render transport plus the
/// libmpegh speaker-rendered reference and its exact output layout.
///
/// This value exists specifically so the speaker layout cannot be lost between
/// native decode and role-aware evidence comparison.
#[derive(Debug)]
pub struct MpeghPairedEvidence {
    pub scene: SpatialTransportFrame,
    pub reference: Option<MpeghRenderedPcm>,
    pub reference_layout: MpeghSpeakerLayout,
}

/// Build a paired evidence value without decoding the compressed access unit a
/// second time. `external` and `reference` are expected to originate from the
/// same successful libmpegh execute call.
pub fn pair_mpegh_external_evidence(
    external: &MpeghExternalFrame,
    reference: Option<MpeghRenderedPcm>,
    presentation_time_seconds: f64,
    discontinuity: bool,
) -> Result<MpeghPairedEvidence, MpeghEvidencePairError> {
    if let Some(reference) = &reference {
        reference.validate()?;
        let external_rate = u32::try_from(external.sample_rate)
            .ok()
            .filter(|value| *value > 0)
            .ok_or(MpeghEvidencePairError::InvalidExternalSampleRate(
                external.sample_rate,
            ))?;
        if reference.sample_rate != external_rate {
            return Err(MpeghEvidencePairError::ReferenceSampleRateMismatch {
                external: external_rate,
                reference: reference.sample_rate,
            });
        }
        if !external.speaker_layout.speakers.is_empty()
            && external.speaker_layout.speakers.len() != reference.channel_count
        {
            return Err(MpeghEvidencePairError::ReferenceLayoutChannelMismatch {
                speakers: external.speaker_layout.speakers.len(),
                channels: reference.channel_count,
            });
        }
    }

    let scene = external.to_spatial_transport_v2(presentation_time_seconds, discontinuity)?;
    Ok(MpeghPairedEvidence {
        scene,
        reference,
        reference_layout: external.speaker_layout.clone(),
    })
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum MpeghEvidencePairError {
    #[error(transparent)]
    SpatialTransport(#[from] MpeghSpatialTransportError),
    #[error(transparent)]
    Reference(#[from] MpeghRenderedPcmError),
    #[error("MPEG-H external frame reports invalid sample rate {0}")]
    InvalidExternalSampleRate(i32),
    #[error("MPEG-H reference sample rate {reference} Hz does not match external scene rate {external} Hz")]
    ReferenceSampleRateMismatch { external: u32, reference: u32 },
    #[error("MPEG-H reference layout has {speakers} speakers for {channels} rendered PCM channels")]
    ReferenceLayoutChannelMismatch { speakers: usize, channels: usize },
}
