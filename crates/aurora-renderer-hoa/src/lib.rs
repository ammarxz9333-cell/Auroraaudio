//! Reference Higher Order Ambisonics renderer for Aurora.
//!
//! This crate deliberately separates coefficient decoding from decode-matrix
//! generation. It consumes already-decoded HOA coefficient signals and a
//! caller-supplied, validated speaker decode matrix. No transport channel is
//! ever treated as an HOA coefficient and no speaker matrix is guessed.

#![forbid(unsafe_code)]

mod matrix_generator;

use aurora_core::AudioBlock;
use aurora_hoa_ir::{HoaCoefficientConvention, HoaCoefficientFrame};
use thiserror::Error;

pub use matrix_generator::{
    generate_regularized_mode_matching_matrix, real_n3d_spherical_harmonic,
    HoaMatrixGenerationError, SpeakerDirection,
};

#[derive(Debug, Clone, PartialEq)]
pub struct HoaDecodeMatrix {
    pub speaker_count: usize,
    pub coefficient_count: usize,
    /// Row-major `[speaker][coefficient]` linear gains.
    pub gains: Vec<f32>,
}

impl HoaDecodeMatrix {
    pub fn validate(&self) -> Result<(), HoaRendererError> {
        if self.speaker_count == 0 || self.coefficient_count == 0 {
            return Err(HoaRendererError::EmptyMatrix);
        }
        let expected = self
            .speaker_count
            .checked_mul(self.coefficient_count)
            .ok_or(HoaRendererError::GeometryOverflow)?;
        if self.gains.len() != expected {
            return Err(HoaRendererError::MatrixLengthMismatch {
                expected,
                actual: self.gains.len(),
            });
        }
        if self.gains.iter().any(|gain| !gain.is_finite()) {
            return Err(HoaRendererError::NonFiniteMatrixGain);
        }
        Ok(())
    }

    pub fn row(&self, speaker: usize) -> Option<&[f32]> {
        if speaker >= self.speaker_count {
            return None;
        }
        let start = speaker * self.coefficient_count;
        Some(&self.gains[start..start + self.coefficient_count])
    }
}

/// Render ACN/N3D HOA coefficients through a precomputed decode matrix.
///
/// This is a reference-quality implementation: accumulation is performed in
/// `f64` to make conformance comparisons stable. A later realtime backend may
/// use SIMD/f32 after it is proven equivalent within policy tolerances.
pub fn render_hoa_coefficients(
    frame: &HoaCoefficientFrame,
    matrix: &HoaDecodeMatrix,
) -> Result<AudioBlock, HoaRendererError> {
    frame
        .validate()
        .map_err(|error| HoaRendererError::InvalidCoefficientFrame(error.to_string()))?;
    matrix.validate()?;

    if frame.convention != HoaCoefficientConvention::AcnN3d {
        return Err(HoaRendererError::UnsupportedConvention);
    }
    if frame.coefficients.len() != matrix.coefficient_count {
        return Err(HoaRendererError::CoefficientCountMismatch {
            frame: frame.coefficients.len(),
            matrix: matrix.coefficient_count,
        });
    }

    let frame_count = frame.audio.frame_count;
    let mut channels = vec![vec![0.0_f32; frame_count]; matrix.speaker_count];
    for (speaker_index, output) in channels.iter_mut().enumerate() {
        let row = matrix
            .row(speaker_index)
            .ok_or(HoaRendererError::GeometryOverflow)?;
        for sample_index in 0..frame_count {
            let mut acc = 0.0_f64;
            for (coefficient_index, gain) in row.iter().copied().enumerate() {
                let binding = &frame.coefficients[coefficient_index];
                let sample = frame.audio.channels[binding.pcm_channel_index][sample_index];
                acc += f64::from(sample) * f64::from(gain);
            }
            output[sample_index] = acc as f32;
        }
    }

    let rendered = AudioBlock {
        channels,
        frame_count,
        presentation_time_seconds: frame.audio.presentation_time_seconds,
        discontinuity: frame.audio.discontinuity,
    };
    rendered
        .validate()
        .map_err(|_| HoaRendererError::InvalidRenderedGeometry)?;
    Ok(rendered)
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum HoaRendererError {
    #[error("HOA decode matrix must contain at least one speaker and coefficient")]
    EmptyMatrix,
    #[error("HOA renderer geometry arithmetic overflow")]
    GeometryOverflow,
    #[error("HOA decode matrix requires {expected} gains but contains {actual}")]
    MatrixLengthMismatch { expected: usize, actual: usize },
    #[error("HOA decode matrix contains a non-finite gain")]
    NonFiniteMatrixGain,
    #[error("HOA coefficient frame failed validation: {0}")]
    InvalidCoefficientFrame(String),
    #[error("HOA renderer currently accepts only ACN/N3D coefficients")]
    UnsupportedConvention,
    #[error("HOA coefficient frame contains {frame} coefficients but decode matrix expects {matrix}")]
    CoefficientCountMismatch { frame: usize, matrix: usize },
    #[error("HOA renderer produced invalid output geometry")]
    InvalidRenderedGeometry,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_hoa_ir::{HoaCoefficientBinding, HoaCoefficientConvention};

    fn order_one_frame() -> HoaCoefficientFrame {
        HoaCoefficientFrame {
            audio: AudioBlock {
                channels: vec![
                    vec![1.0, 2.0],
                    vec![10.0, 20.0],
                    vec![100.0, 200.0],
                    vec![1000.0, 2000.0],
                ],
                frame_count: 2,
                presentation_time_seconds: 1.25,
                discontinuity: true,
            },
            order: 1,
            convention: HoaCoefficientConvention::AcnN3d,
            coefficients: (0..4)
                .map(|index| HoaCoefficientBinding {
                    pcm_channel_index: index,
                    coefficient_index: index,
                })
                .collect(),
        }
    }

    #[test]
    fn identity_decode_matrix_preserves_coefficients() {
        let matrix = HoaDecodeMatrix {
            speaker_count: 4,
            coefficient_count: 4,
            gains: vec![
                1.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 0.0, 0.0,
                0.0, 0.0, 1.0, 0.0,
                0.0, 0.0, 0.0, 1.0,
            ],
        };
        let rendered = render_hoa_coefficients(&order_one_frame(), &matrix).unwrap();
        assert_eq!(rendered.channels[2], vec![100.0, 200.0]);
        assert_eq!(rendered.presentation_time_seconds, 1.25);
        assert!(rendered.discontinuity);
    }

    #[test]
    fn dense_matrix_mixes_coefficients_per_speaker() {
        let matrix = HoaDecodeMatrix {
            speaker_count: 1,
            coefficient_count: 4,
            gains: vec![1.0, 0.1, 0.01, 0.001],
        };
        let rendered = render_hoa_coefficients(&order_one_frame(), &matrix).unwrap();
        assert_eq!(rendered.channels[0], vec![4.0, 8.0]);
    }

    #[test]
    fn matrix_geometry_fails_closed() {
        let matrix = HoaDecodeMatrix {
            speaker_count: 2,
            coefficient_count: 4,
            gains: vec![0.0; 7],
        };
        assert!(matches!(
            render_hoa_coefficients(&order_one_frame(), &matrix),
            Err(HoaRendererError::MatrixLengthMismatch { expected: 8, actual: 7 })
        ));
    }

    #[test]
    fn non_finite_matrix_gain_is_rejected() {
        let matrix = HoaDecodeMatrix {
            speaker_count: 1,
            coefficient_count: 4,
            gains: vec![1.0, f32::NAN, 0.0, 0.0],
        };
        assert_eq!(matrix.validate(), Err(HoaRendererError::NonFiniteMatrixGain));
    }
}
