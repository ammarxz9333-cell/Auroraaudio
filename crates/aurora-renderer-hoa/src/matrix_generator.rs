use std::f64::consts::PI;

use crate::{HoaDecodeMatrix, HoaRendererError};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeakerDirection {
    pub azimuth_degrees: f64,
    pub elevation_degrees: f64,
}

/// Generate a regularized mode-matching decode matrix for ACN/N3D HOA.
///
/// This is an Aurora candidate matrix, not an MPEG-H signaled matrix. It must
/// pass the external libmpegh reference gate before being admitted for playback.
pub fn generate_regularized_mode_matching_matrix(
    order: u16,
    speakers: &[SpeakerDirection],
    regularization: f64,
) -> Result<HoaDecodeMatrix, HoaMatrixGenerationError> {
    if speakers.is_empty() {
        return Err(HoaMatrixGenerationError::NoSpeakers);
    }
    if !regularization.is_finite() || regularization <= 0.0 {
        return Err(HoaMatrixGenerationError::InvalidRegularization);
    }
    if speakers.iter().any(|speaker| {
        !speaker.azimuth_degrees.is_finite()
            || !speaker.elevation_degrees.is_finite()
            || !(-90.0..=90.0).contains(&speaker.elevation_degrees)
    }) {
        return Err(HoaMatrixGenerationError::InvalidSpeakerDirection);
    }

    let coefficient_count = (usize::from(order) + 1)
        .checked_mul(usize::from(order) + 1)
        .ok_or(HoaMatrixGenerationError::GeometryOverflow)?;
    let speaker_count = speakers.len();

    let mut basis = vec![0.0_f64; speaker_count * coefficient_count];
    for (speaker_index, speaker) in speakers.iter().enumerate() {
        let phi = speaker.azimuth_degrees.to_radians();
        let elevation = speaker.elevation_degrees.to_radians();
        let cos_theta = elevation.sin();
        let sin_theta = elevation.cos();
        for n in 0..=i32::from(order) {
            for m in -n..=n {
                let index = acn_index(n, m)?;
                basis[speaker_index * coefficient_count + index] =
                    real_n3d_spherical_harmonic(n, m, cos_theta, sin_theta, phi)?;
            }
        }
    }

    let decoder = if speaker_count >= coefficient_count {
        // D = Y (Y^T Y + λI)^-1
        let gram = transpose_times_self(&basis, speaker_count, coefficient_count)?;
        let regularized = add_diagonal(gram, coefficient_count, regularization)?;
        let inverse = invert_square(regularized, coefficient_count)?;
        matmul(
            &basis,
            speaker_count,
            coefficient_count,
            &inverse,
            coefficient_count,
            coefficient_count,
        )?
    } else {
        // D = (Y Y^T + λI)^-1 Y
        let gram = self_times_transpose(&basis, speaker_count, coefficient_count)?;
        let regularized = add_diagonal(gram, speaker_count, regularization)?;
        let inverse = invert_square(regularized, speaker_count)?;
        matmul(
            &inverse,
            speaker_count,
            speaker_count,
            &basis,
            speaker_count,
            coefficient_count,
        )?
    };

    let gains = decoder.into_iter().map(|value| value as f32).collect();
    let matrix = HoaDecodeMatrix {
        speaker_count,
        coefficient_count,
        gains,
    };
    matrix
        .validate()
        .map_err(HoaMatrixGenerationError::Renderer)?;
    Ok(matrix)
}

pub fn real_n3d_spherical_harmonic(
    n: i32,
    m: i32,
    cos_theta: f64,
    sin_theta: f64,
    phi: f64,
) -> Result<f64, HoaMatrixGenerationError> {
    if n < 0 || m.abs() > n || !cos_theta.is_finite() || !sin_theta.is_finite() || !phi.is_finite()
    {
        return Err(HoaMatrixGenerationError::InvalidHarmonicArguments);
    }
    let abs_m = m.abs();
    let legendre = associated_legendre_no_condon_shortley(n, abs_m, cos_theta, sin_theta);
    let denominator = factorial_ratio_product(n - abs_m + 1, n + abs_m)?;
    let scale = if abs_m == 0 {
        ((2 * n + 1) as f64 / denominator).sqrt()
    } else {
        (2.0 * (2 * n + 1) as f64 / denominator).sqrt()
    };
    let sh = scale * legendre;
    Ok(if m < 0 {
        sh * (f64::from(abs_m) * phi).sin()
    } else {
        sh * (f64::from(m) * phi).cos()
    })
}

fn associated_legendre_no_condon_shortley(
    n: i32,
    abs_m: i32,
    cos_theta: f64,
    sin_theta: f64,
) -> f64 {
    let mut p_mm = 1.0_f64;
    let mut odd = 1.0_f64;
    for _ in 1..=abs_m {
        p_mm *= -odd * sin_theta;
        odd += 2.0;
    }
    if abs_m & 1 == 1 {
        p_mm = -p_mm;
    }
    if n == abs_m {
        return p_mm;
    }
    let mut p_m_plus_1_m = cos_theta * f64::from(2 * abs_m + 1) * p_mm;
    if n == abs_m + 1 {
        return p_m_plus_1_m;
    }
    let mut p_prev = p_mm;
    for j in (abs_m + 2)..=n {
        let p = (cos_theta * f64::from(2 * j - 1) * p_m_plus_1_m
            - f64::from(j + abs_m - 1) * p_prev)
            / f64::from(j - abs_m);
        p_prev = p_m_plus_1_m;
        p_m_plus_1_m = p;
    }
    p_m_plus_1_m
}

fn factorial_ratio_product(start: i32, end: i32) -> Result<f64, HoaMatrixGenerationError> {
    if start > end {
        return Ok(1.0);
    }
    let mut product = 1.0_f64;
    for value in start..=end {
        if value <= 0 {
            return Err(HoaMatrixGenerationError::InvalidHarmonicArguments);
        }
        product *= f64::from(value);
    }
    Ok(product)
}

fn acn_index(n: i32, m: i32) -> Result<usize, HoaMatrixGenerationError> {
    usize::try_from(n * (n + 1) + m).map_err(|_| HoaMatrixGenerationError::GeometryOverflow)
}

fn transpose_times_self(
    matrix: &[f64],
    rows: usize,
    cols: usize,
) -> Result<Vec<f64>, HoaMatrixGenerationError> {
    if matrix.len() != rows.checked_mul(cols).ok_or(HoaMatrixGenerationError::GeometryOverflow)? {
        return Err(HoaMatrixGenerationError::GeometryOverflow);
    }
    let mut out = vec![0.0; cols * cols];
    for r in 0..cols {
        for c in 0..cols {
            let mut acc = 0.0;
            for k in 0..rows {
                acc += matrix[k * cols + r] * matrix[k * cols + c];
            }
            out[r * cols + c] = acc;
        }
    }
    Ok(out)
}

fn self_times_transpose(
    matrix: &[f64],
    rows: usize,
    cols: usize,
) -> Result<Vec<f64>, HoaMatrixGenerationError> {
    if matrix.len() != rows.checked_mul(cols).ok_or(HoaMatrixGenerationError::GeometryOverflow)? {
        return Err(HoaMatrixGenerationError::GeometryOverflow);
    }
    let mut out = vec![0.0; rows * rows];
    for r in 0..rows {
        for c in 0..rows {
            let mut acc = 0.0;
            for k in 0..cols {
                acc += matrix[r * cols + k] * matrix[c * cols + k];
            }
            out[r * rows + c] = acc;
        }
    }
    Ok(out)
}

fn add_diagonal(
    mut matrix: Vec<f64>,
    size: usize,
    regularization: f64,
) -> Result<Vec<f64>, HoaMatrixGenerationError> {
    if matrix.len() != size.checked_mul(size).ok_or(HoaMatrixGenerationError::GeometryOverflow)? {
        return Err(HoaMatrixGenerationError::GeometryOverflow);
    }
    for index in 0..size {
        matrix[index * size + index] += regularization;
    }
    Ok(matrix)
}

fn matmul(
    left: &[f64],
    left_rows: usize,
    inner: usize,
    right: &[f64],
    right_rows: usize,
    right_cols: usize,
) -> Result<Vec<f64>, HoaMatrixGenerationError> {
    if inner != right_rows
        || left.len()
            != left_rows
                .checked_mul(inner)
                .ok_or(HoaMatrixGenerationError::GeometryOverflow)?
        || right.len()
            != right_rows
                .checked_mul(right_cols)
                .ok_or(HoaMatrixGenerationError::GeometryOverflow)?
    {
        return Err(HoaMatrixGenerationError::GeometryOverflow);
    }
    let mut out = vec![0.0; left_rows * right_cols];
    for row in 0..left_rows {
        for col in 0..right_cols {
            let mut acc = 0.0;
            for k in 0..inner {
                acc += left[row * inner + k] * right[k * right_cols + col];
            }
            out[row * right_cols + col] = acc;
        }
    }
    Ok(out)
}

fn invert_square(
    matrix: Vec<f64>,
    size: usize,
) -> Result<Vec<f64>, HoaMatrixGenerationError> {
    if matrix.len() != size.checked_mul(size).ok_or(HoaMatrixGenerationError::GeometryOverflow)? {
        return Err(HoaMatrixGenerationError::GeometryOverflow);
    }
    let augmented_cols = size
        .checked_mul(2)
        .ok_or(HoaMatrixGenerationError::GeometryOverflow)?;
    let mut augmented = vec![0.0_f64; size * augmented_cols];
    for row in 0..size {
        for col in 0..size {
            augmented[row * augmented_cols + col] = matrix[row * size + col];
        }
        augmented[row * augmented_cols + size + row] = 1.0;
    }

    for pivot_col in 0..size {
        let mut pivot_row = pivot_col;
        let mut pivot_abs = augmented[pivot_row * augmented_cols + pivot_col].abs();
        for row in (pivot_col + 1)..size {
            let value = augmented[row * augmented_cols + pivot_col].abs();
            if value > pivot_abs {
                pivot_abs = value;
                pivot_row = row;
            }
        }
        if !pivot_abs.is_finite() || pivot_abs < 1.0e-12 {
            return Err(HoaMatrixGenerationError::SingularGeometry);
        }
        if pivot_row != pivot_col {
            for col in 0..augmented_cols {
                augmented.swap(
                    pivot_row * augmented_cols + col,
                    pivot_col * augmented_cols + col,
                );
            }
        }

        let pivot = augmented[pivot_col * augmented_cols + pivot_col];
        for col in 0..augmented_cols {
            augmented[pivot_col * augmented_cols + col] /= pivot;
        }
        for row in 0..size {
            if row == pivot_col {
                continue;
            }
            let factor = augmented[row * augmented_cols + pivot_col];
            if factor == 0.0 {
                continue;
            }
            for col in 0..augmented_cols {
                let pivot_value = augmented[pivot_col * augmented_cols + col];
                augmented[row * augmented_cols + col] -= factor * pivot_value;
            }
        }
    }

    let mut inverse = vec![0.0; size * size];
    for row in 0..size {
        for col in 0..size {
            inverse[row * size + col] = augmented[row * augmented_cols + size + col];
        }
    }
    Ok(inverse)
}

#[derive(Debug, thiserror::Error, Clone, PartialEq)]
pub enum HoaMatrixGenerationError {
    #[error("HOA matrix generation requires at least one speaker")]
    NoSpeakers,
    #[error("HOA matrix regularization must be finite and greater than zero")]
    InvalidRegularization,
    #[error("HOA speaker direction is invalid")]
    InvalidSpeakerDirection,
    #[error("HOA spherical-harmonic arguments are invalid")]
    InvalidHarmonicArguments,
    #[error("HOA matrix geometry arithmetic overflow")]
    GeometryOverflow,
    #[error("HOA speaker geometry is numerically singular")]
    SingularGeometry,
    #[error(transparent)]
    Renderer(#[from] HoaRendererError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_order_front_basis_matches_real_n3d_signs() {
        let phi = 0.0;
        let cos_theta = 0.0;
        let sin_theta = 1.0;
        let y00 = real_n3d_spherical_harmonic(0, 0, cos_theta, sin_theta, phi).unwrap();
        let y1m1 = real_n3d_spherical_harmonic(1, -1, cos_theta, sin_theta, phi).unwrap();
        let y10 = real_n3d_spherical_harmonic(1, 0, cos_theta, sin_theta, phi).unwrap();
        let y11 = real_n3d_spherical_harmonic(1, 1, cos_theta, sin_theta, phi).unwrap();
        assert!((y00 - 1.0).abs() < 1.0e-12);
        assert!(y1m1.abs() < 1.0e-12);
        assert!(y10.abs() < 1.0e-12);
        assert!((y11 - 3.0_f64.sqrt()).abs() < 1.0e-12);
    }

    #[test]
    fn tetrahedral_geometry_generates_first_order_matrix() {
        let speakers = [
            SpeakerDirection { azimuth_degrees: 45.0, elevation_degrees: 35.26438968 },
            SpeakerDirection { azimuth_degrees: -135.0, elevation_degrees: 35.26438968 },
            SpeakerDirection { azimuth_degrees: 135.0, elevation_degrees: -35.26438968 },
            SpeakerDirection { azimuth_degrees: -45.0, elevation_degrees: -35.26438968 },
        ];
        let matrix = generate_regularized_mode_matching_matrix(1, &speakers, 1.0e-8).unwrap();
        assert_eq!(matrix.speaker_count, 4);
        assert_eq!(matrix.coefficient_count, 4);
        assert_eq!(matrix.gains.len(), 16);
        matrix.validate().unwrap();
    }

    #[test]
    fn invalid_elevation_fails_closed() {
        let speakers = [SpeakerDirection { azimuth_degrees: 0.0, elevation_degrees: 91.0 }];
        assert_eq!(
            generate_regularized_mode_matching_matrix(0, &speakers, 1.0e-6),
            Err(HoaMatrixGenerationError::InvalidSpeakerDirection)
        );
    }
}
