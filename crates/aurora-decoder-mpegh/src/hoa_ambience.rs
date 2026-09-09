use thiserror::Error;

/// MPEG-H HOA ambience synthesis for the LC-profile path where
/// `use_phase_shift_decorr == 0` (the reference decoder rejects value 1).
///
/// `reassignment_by_coefficient[c]` contains the zero-based transport-channel
/// index assigned to HOA coefficient `c`, or `None` for an unassigned
/// coefficient. `low_order_matrix` is row-major with
/// `low_order_coefficient_count^2` entries.
pub fn synthesize_hoa_ambience(
    corrected_transport_pcm: &[Vec<f32>],
    reassignment_by_coefficient: &[Option<usize>],
    low_order_coefficient_count: usize,
    low_order_matrix: &[f32],
) -> Result<Vec<Vec<f32>>, MpeghHoaAmbienceError> {
    let coefficient_count = reassignment_by_coefficient.len();
    if coefficient_count == 0 {
        return Err(MpeghHoaAmbienceError::ZeroCoefficients);
    }
    if low_order_coefficient_count > coefficient_count {
        return Err(MpeghHoaAmbienceError::LowOrderCountOutOfRange {
            low_order: low_order_coefficient_count,
            total: coefficient_count,
        });
    }
    let expected_matrix = low_order_coefficient_count
        .checked_mul(low_order_coefficient_count)
        .ok_or(MpeghHoaAmbienceError::NumericOverflow)?;
    if low_order_matrix.len() != expected_matrix {
        return Err(MpeghHoaAmbienceError::MatrixSizeMismatch {
            expected: expected_matrix,
            actual: low_order_matrix.len(),
        });
    }
    if low_order_matrix.iter().any(|value| !value.is_finite()) {
        return Err(MpeghHoaAmbienceError::NonFiniteMatrix);
    }

    let frame_length = corrected_transport_pcm
        .first()
        .map(Vec::len)
        .ok_or(MpeghHoaAmbienceError::ZeroTransportChannels)?;
    if frame_length == 0 {
        return Err(MpeghHoaAmbienceError::EmptyFrame);
    }
    if corrected_transport_pcm
        .iter()
        .any(|channel| channel.len() != frame_length)
    {
        return Err(MpeghHoaAmbienceError::InconsistentFrameLength);
    }
    if corrected_transport_pcm
        .iter()
        .flatten()
        .any(|sample| !sample.is_finite())
    {
        return Err(MpeghHoaAmbienceError::NonFiniteInput);
    }

    for (coefficient, assignment) in reassignment_by_coefficient.iter().enumerate() {
        if let Some(channel) = assignment {
            if *channel >= corrected_transport_pcm.len() {
                return Err(MpeghHoaAmbienceError::TransportIndexOutOfRange {
                    coefficient,
                    transport_index: *channel,
                    transport_channels: corrected_transport_pcm.len(),
                });
            }
        }
    }

    let mut output = vec![vec![0.0; frame_length]; coefficient_count];

    // Higher-order ambience coefficients are direct assigned transport signals
    // (or zero when unassigned) in the reference decoder.
    for coefficient in low_order_coefficient_count..coefficient_count {
        if let Some(channel) = reassignment_by_coefficient[coefficient] {
            output[coefficient].copy_from_slice(&corrected_transport_pcm[channel]);
        }
    }

    // Low-order ambience is synthesized through the square order matrix.
    for output_coefficient in 0..low_order_coefficient_count {
        let matrix_row = &low_order_matrix
            [output_coefficient * low_order_coefficient_count..(output_coefficient + 1) * low_order_coefficient_count];
        for sample in 0..frame_length {
            let mut accumulator = 0.0_f32;
            for (input_coefficient, matrix_gain) in matrix_row.iter().copied().enumerate() {
                if let Some(channel) = reassignment_by_coefficient[input_coefficient] {
                    accumulator += matrix_gain * corrected_transport_pcm[channel][sample];
                }
            }
            if !accumulator.is_finite() {
                return Err(MpeghHoaAmbienceError::NonFiniteOutput {
                    coefficient: output_coefficient,
                    sample,
                });
            }
            output[output_coefficient][sample] = accumulator;
        }
    }

    Ok(output)
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MpeghHoaAmbienceError {
    #[error("HOA ambience synthesis requires at least one coefficient")]
    ZeroCoefficients,
    #[error("HOA ambience synthesis requires at least one transport channel")]
    ZeroTransportChannels,
    #[error("HOA ambience frame is empty")]
    EmptyFrame,
    #[error("HOA ambience transport channels have inconsistent frame lengths")]
    InconsistentFrameLength,
    #[error("HOA ambience input contains a non-finite sample")]
    NonFiniteInput,
    #[error("HOA ambience low-order coefficient count {low_order} exceeds total {total}")]
    LowOrderCountOutOfRange { low_order: usize, total: usize },
    #[error("HOA ambience matrix requires {expected} values but contains {actual}")]
    MatrixSizeMismatch { expected: usize, actual: usize },
    #[error("HOA ambience matrix contains a non-finite value")]
    NonFiniteMatrix,
    #[error("coefficient {coefficient} references transport channel {transport_index} but only {transport_channels} channels exist")]
    TransportIndexOutOfRange {
        coefficient: usize,
        transport_index: usize,
        transport_channels: usize,
    },
    #[error("HOA ambience synthesis produced non-finite output at coefficient {coefficient}, sample {sample}")]
    NonFiniteOutput { coefficient: usize, sample: usize },
    #[error("HOA ambience synthesis arithmetic overflow")]
    NumericOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_low_order_matrix_preserves_assigned_coefficients() {
        let corrected = vec![vec![1.0, 2.0], vec![3.0, 4.0]];
        let output = synthesize_hoa_ambience(
            &corrected,
            &[Some(0), Some(1)],
            2,
            &[1.0, 0.0, 0.0, 1.0],
        )
        .unwrap();
        assert_eq!(output, corrected);
    }

    #[test]
    fn unassigned_higher_order_coefficient_is_silent() {
        let corrected = vec![vec![1.0, 2.0]];
        let output = synthesize_hoa_ambience(
            &corrected,
            &[Some(0), None],
            1,
            &[1.0],
        )
        .unwrap();
        assert_eq!(output[0], vec![1.0, 2.0]);
        assert_eq!(output[1], vec![0.0, 0.0]);
    }
}
