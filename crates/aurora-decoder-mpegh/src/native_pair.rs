#![cfg(feature = "native-mpegh")]

use thiserror::Error;

use crate::{
    pair_mpegh_external_evidence_with_hoa, MpeghEvidencePairError, MpeghNativeError,
    MpeghPairedEvidence, NativeMpeghDecoder,
};

/// Decode one compressed MPEG-H chunk through the native backend and, when a
/// complete access unit is produced, bind all products from that same execute:
/// external-render transport scene, post-spatial ACN/N3D HOA coefficients,
/// and libmpegh's final speaker-rendered reference PCM.
///
/// This function never decodes the compressed access unit twice. It also does
/// not claim that Aurora has rendered the HOA coefficients; they remain an
/// independent evidence plane for a future Aurora HOA renderer.
pub fn decode_native_paired_chunk(
    decoder: &mut NativeMpeghDecoder,
    input: &[u8],
    presentation_time_seconds: f64,
    discontinuity: bool,
) -> Result<Option<MpeghPairedEvidence>, MpeghNativePairError> {
    let Some(external) = decoder.push(input)? else {
        return Ok(None);
    };
    let hoa_coefficients = decoder.take_hoa_coefficients();
    let reference = decoder.take_rendered_pcm();
    let pair = pair_mpegh_external_evidence_with_hoa(
        &external,
        hoa_coefficients,
        reference,
        presentation_time_seconds,
        discontinuity,
    )?;
    Ok(Some(pair))
}

#[derive(Debug, Error)]
pub enum MpeghNativePairError {
    #[error(transparent)]
    Native(#[from] MpeghNativeError),
    #[error(transparent)]
    Pair(#[from] MpeghEvidencePairError),
}
