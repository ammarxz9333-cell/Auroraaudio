//! Codec-neutral Higher Order Ambisonics (HOA) coefficient contract.
//!
//! This crate deliberately distinguishes decoded HOA coefficient signals from
//! codec transport channels. A transport channel must never be labeled as a
//! coefficient until the codec's stateful spatial synthesis has run.

#![forbid(unsafe_code)]

use std::collections::HashSet;

use aurora_core::AudioBlock;
use thiserror::Error;

/// Coefficient indexing/normalization convention carried by a decoded HOA frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoaCoefficientConvention {
    /// Codec-native indexing whose public normalization/order has not yet been
    /// proven. Keep this explicit rather than silently assuming ACN.
    CodecNativeIndexed,
    /// ACN channel ordering with SN3D normalization.
    AcnSn3d,
    /// ACN channel ordering with N3D normalization.
    AcnN3d,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HoaCoefficientBinding {
    /// PCM lane containing this decoded HOA coefficient signal.
    pub pcm_channel_index: usize,
    /// Zero-based coefficient index in the declared convention.
    pub coefficient_index: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HoaCoefficientFrame {
    pub audio: AudioBlock,
    pub order: u16,
    pub convention: HoaCoefficientConvention,
    pub coefficients: Vec<HoaCoefficientBinding>,
}

impl HoaCoefficientFrame {
    pub fn expected_coefficient_count(&self) -> Result<usize, HoaIrError> {
        coefficient_count_for_order(self.order)
    }

    pub fn validate(&self) -> Result<(), HoaIrError> {
        self.audio
            .validate()
            .map_err(|_| HoaIrError::InvalidAudioGeometry)?;

        let expected = self.expected_coefficient_count()?;
        if self.coefficients.len() != expected {
            return Err(HoaIrError::CoefficientCountMismatch {
                expected,
                actual: self.coefficients.len(),
            });
        }
        if self.audio.channels.len() != expected {
            return Err(HoaIrError::AudioLaneCountMismatch {
                expected,
                actual: self.audio.channels.len(),
            });
        }

        let mut lanes = HashSet::with_capacity(expected);
        let mut indices = HashSet::with_capacity(expected);
        for binding in &self.coefficients {
            if binding.pcm_channel_index >= self.audio.channels.len() {
                return Err(HoaIrError::PcmLaneOutOfRange {
                    lane: binding.pcm_channel_index,
                    channels: self.audio.channels.len(),
                });
            }
            if binding.coefficient_index >= expected {
                return Err(HoaIrError::CoefficientIndexOutOfRange {
                    index: binding.coefficient_index,
                    coefficients: expected,
                });
            }
            if !lanes.insert(binding.pcm_channel_index) {
                return Err(HoaIrError::DuplicatePcmLane {
                    lane: binding.pcm_channel_index,
                });
            }
            if !indices.insert(binding.coefficient_index) {
                return Err(HoaIrError::DuplicateCoefficientIndex {
                    index: binding.coefficient_index,
                });
            }
        }

        if lanes.len() != expected || indices.len() != expected {
            return Err(HoaIrError::IncompleteCoefficientOwnership);
        }
        Ok(())
    }
}

pub fn coefficient_count_for_order(order: u16) -> Result<usize, HoaIrError> {
    let side = usize::from(order)
        .checked_add(1)
        .ok_or(HoaIrError::NumericOverflow)?;
    side.checked_mul(side).ok_or(HoaIrError::NumericOverflow)
}

/// Ambisonic Channel Number (ACN): `index = n(n+1) + m`.
pub fn acn_index(order: u16, degree: i16) -> Result<usize, HoaIrError> {
    let n = i32::from(order);
    let m = i32::from(degree);
    if m < -n || m > n {
        return Err(HoaIrError::InvalidAcnDegree { order, degree });
    }
    let index = n
        .checked_mul(n + 1)
        .and_then(|value| value.checked_add(m))
        .ok_or(HoaIrError::NumericOverflow)?;
    usize::try_from(index).map_err(|_| HoaIrError::NumericOverflow)
}

/// Inverse ACN mapping. ACN indices for order `n` occupy `n^2..(n+1)^2`.
pub fn acn_order_degree(index: usize) -> Result<(u16, i16), HoaIrError> {
    let mut order = 0usize;
    loop {
        let next = order
            .checked_add(1)
            .ok_or(HoaIrError::NumericOverflow)?;
        let next_square = next
            .checked_mul(next)
            .ok_or(HoaIrError::NumericOverflow)?;
        if next_square > index {
            break;
        }
        order = next;
    }
    let n = i64::try_from(order).map_err(|_| HoaIrError::NumericOverflow)?;
    let idx = i64::try_from(index).map_err(|_| HoaIrError::NumericOverflow)?;
    let degree = idx
        .checked_sub(n.checked_mul(n + 1).ok_or(HoaIrError::NumericOverflow)?)
        .ok_or(HoaIrError::NumericOverflow)?;
    Ok((
        u16::try_from(order).map_err(|_| HoaIrError::NumericOverflow)?,
        i16::try_from(degree).map_err(|_| HoaIrError::NumericOverflow)?,
    ))
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum HoaIrError {
    #[error("HOA coefficient audio block is internally inconsistent")]
    InvalidAudioGeometry,
    #[error("HOA order arithmetic overflow")]
    NumericOverflow,
    #[error("ACN degree {degree} is invalid for order {order}")]
    InvalidAcnDegree { order: u16, degree: i16 },
    #[error("HOA order requires {expected} coefficient bindings but frame contains {actual}")]
    CoefficientCountMismatch { expected: usize, actual: usize },
    #[error("HOA order requires {expected} PCM lanes but frame contains {actual}")]
    AudioLaneCountMismatch { expected: usize, actual: usize },
    #[error("HOA PCM lane {lane} is outside {channels}-lane coefficient audio")]
    PcmLaneOutOfRange { lane: usize, channels: usize },
    #[error("HOA coefficient index {index} is outside {coefficients} coefficients")]
    CoefficientIndexOutOfRange { index: usize, coefficients: usize },
    #[error("HOA PCM lane {lane} is bound more than once")]
    DuplicatePcmLane { lane: usize },
    #[error("HOA coefficient index {index} is bound more than once")]
    DuplicateCoefficientIndex { index: usize },
    #[error("HOA coefficient ownership is incomplete")]
    IncompleteCoefficientOwnership,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_three_has_sixteen_coefficients() {
        assert_eq!(coefficient_count_for_order(3).unwrap(), 16);
    }

    #[test]
    fn acn_round_trip_matches_n_n_plus_one_plus_m() {
        for n in 0..=6u16 {
            for m in -(n as i16)..=(n as i16) {
                let index = acn_index(n, m).unwrap();
                assert_eq!(acn_order_degree(index).unwrap(), (n, m));
            }
        }
    }

    #[test]
    fn acn_n3d_frame_requires_complete_unique_ownership() {
        let frame = HoaCoefficientFrame {
            audio: AudioBlock {
                channels: vec![vec![0.0; 40]; 4],
                frame_count: 40,
                presentation_time_seconds: 0.0,
                discontinuity: false,
            },
            order: 1,
            convention: HoaCoefficientConvention::AcnN3d,
            coefficients: (0..4)
                .map(|index| HoaCoefficientBinding {
                    pcm_channel_index: index,
                    coefficient_index: index,
                })
                .collect(),
        };
        frame.validate().unwrap();
    }

    #[test]
    fn transport_channel_subset_cannot_masquerade_as_coefficients() {
        let frame = HoaCoefficientFrame {
            audio: AudioBlock {
                channels: vec![vec![0.0; 40]; 2],
                frame_count: 40,
                presentation_time_seconds: 0.0,
                discontinuity: false,
            },
            order: 1,
            convention: HoaCoefficientConvention::AcnN3d,
            coefficients: vec![
                HoaCoefficientBinding { pcm_channel_index: 0, coefficient_index: 0 },
                HoaCoefficientBinding { pcm_channel_index: 1, coefficient_index: 1 },
            ],
        };
        assert!(matches!(
            frame.validate(),
            Err(HoaIrError::CoefficientCountMismatch { expected: 4, actual: 2 })
        ));
    }
}
