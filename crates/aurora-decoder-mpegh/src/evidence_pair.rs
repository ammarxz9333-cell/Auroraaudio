use thiserror::Error;

use aurora_hoa_ir::{HoaCoefficientConvention, HoaCoefficientFrame};
use aurora_spatial_transport_v2::SpatialTransportFrame;

use crate::{
    MpeghExternalFrame, MpeghHoaDecodeContract, MpeghRenderedPcm, MpeghRenderedPcmError,
    MpeghSpatialTransportError, MpeghSpeakerLayout,
};

/// One MPEG-H access unit represented as Aurora pre-render transport, optional
/// post-spatial/pre-speaker HOA coefficients, and the libmpegh speaker render.
///
/// All products are expected to originate from the same successful native
/// execute call. The coefficient plane is kept separate from both transport
/// lanes and speaker-rendered PCM.
#[derive(Debug)]
pub struct MpeghPairedEvidence {
    pub scene: SpatialTransportFrame,
    pub hoa_coefficients: Option<HoaCoefficientFrame>,
    pub reference: Option<MpeghRenderedPcm>,
    pub reference_layout: MpeghSpeakerLayout,
}

/// Backward-compatible pairing path for callers that do not yet collect the
/// HOA observer plane.
pub fn pair_mpegh_external_evidence(
    external: &MpeghExternalFrame,
    reference: Option<MpeghRenderedPcm>,
    presentation_time_seconds: f64,
    discontinuity: bool,
) -> Result<MpeghPairedEvidence, MpeghEvidencePairError> {
    pair_mpegh_external_evidence_with_hoa(
        external,
        None,
        reference,
        presentation_time_seconds,
        discontinuity,
    )
}

/// Pair transport scene, decoded HOA coefficients, and reference render from
/// one libmpegh access unit without decoding the compressed payload twice.
pub fn pair_mpegh_external_evidence_with_hoa(
    external: &MpeghExternalFrame,
    hoa_coefficients: Option<HoaCoefficientFrame>,
    reference: Option<MpeghRenderedPcm>,
    presentation_time_seconds: f64,
    discontinuity: bool,
) -> Result<MpeghPairedEvidence, MpeghEvidencePairError> {
    validate_reference(external, reference.as_ref())?;

    let scene = external.to_spatial_transport_v2(presentation_time_seconds, discontinuity)?;
    if let Some(coefficients) = &hoa_coefficients {
        coefficients
            .validate()
            .map_err(|error| MpeghEvidencePairError::InvalidHoaCoefficients(error.to_string()))?;
        if coefficients.convention != HoaCoefficientConvention::AcnN3d {
            return Err(MpeghEvidencePairError::UnexpectedHoaConvention);
        }
        let contract = MpeghHoaDecodeContract::from_transport_scene(&scene)
            .map_err(|error| MpeghEvidencePairError::HoaContract(error.to_string()))?
            .ok_or(MpeghEvidencePairError::HoaCoefficientsWithoutTransport)?;
        if coefficients.order != contract.order {
            return Err(MpeghEvidencePairError::HoaOrderMismatch {
                transport: contract.order,
                coefficients: coefficients.order,
            });
        }
        if coefficients.coefficients.len() != contract.expected_coefficient_count {
            return Err(MpeghEvidencePairError::HoaCoefficientCountMismatch {
                expected: contract.expected_coefficient_count,
                actual: coefficients.coefficients.len(),
            });
        }
    }

    Ok(MpeghPairedEvidence {
        scene,
        hoa_coefficients,
        reference,
        reference_layout: external.speaker_layout.clone(),
    })
}

fn validate_reference(
    external: &MpeghExternalFrame,
    reference: Option<&MpeghRenderedPcm>,
) -> Result<(), MpeghEvidencePairError> {
    let Some(reference) = reference else {
        return Ok(());
    };
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
    Ok(())
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
    #[error("MPEG-H HOA observer frame failed validation: {0}")]
    InvalidHoaCoefficients(String),
    #[error("MPEG-H HOA observer emitted coefficients with a convention other than ACN/N3D")]
    UnexpectedHoaConvention,
    #[error("MPEG-H HOA observer emitted coefficients but the transport scene contains no HOA group")]
    HoaCoefficientsWithoutTransport,
    #[error("MPEG-H HOA transport contract failed: {0}")]
    HoaContract(String),
    #[error("MPEG-H HOA transport order {transport} does not match observed coefficient order {coefficients}")]
    HoaOrderMismatch { transport: u16, coefficients: u16 },
    #[error("MPEG-H HOA transport expects {expected} coefficients but observer emitted {actual}")]
    HoaCoefficientCountMismatch { expected: usize, actual: usize },
}
