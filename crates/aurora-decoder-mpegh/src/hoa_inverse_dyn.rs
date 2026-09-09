use thiserror::Error;

/// Decode MPEG-H HOA coded gain-correction exponent to the signed exponent
/// consumed by inverse dynamic correction. Mirrors the reference decoder:
/// code 1 -> 0, code 2 -> -1, otherwise code - 2.
pub fn decode_gain_correction_exponent(code: i32) -> i32 {
    match code {
        1 => 0,
        2 => -1,
        other => other - 2,
    }
}

/// Stateful inverse dynamic correction for MPEG-H HOA transport channels.
///
/// This is the first transport->coefficient synthesis stage. It intentionally
/// uses the reference equation (`window.powi(exp)`) instead of the reference
/// decoder's ROM power-table optimization, keeping the math explicit and easy
/// to verify. A later optimized implementation must prove equivalence against
/// this path before replacing it.
#[derive(Debug, Clone)]
pub struct MpeghHoaInverseDynCorrection {
    previous_gain: Vec<f32>,
}

impl MpeghHoaInverseDynCorrection {
    pub fn new(transport_channels: usize) -> Result<Self, MpeghHoaInverseDynError> {
        if transport_channels == 0 {
            return Err(MpeghHoaInverseDynError::ZeroTransportChannels);
        }
        Ok(Self {
            previous_gain: vec![1.0; transport_channels],
        })
    }

    pub fn reset(&mut self) {
        self.previous_gain.fill(1.0);
    }

    pub fn previous_gain(&self) -> &[f32] {
        &self.previous_gain
    }

    /// Apply HOA independency-frame initial gains before processing.
    pub fn set_previous_gain_from_exponents(
        &mut self,
        previous_amplitude_exponents: &[i32],
    ) -> Result<(), MpeghHoaInverseDynError> {
        if previous_amplitude_exponents.len() != self.previous_gain.len() {
            return Err(MpeghHoaInverseDynError::ChannelMetadataCountMismatch {
                channels: self.previous_gain.len(),
                metadata: previous_amplitude_exponents.len(),
            });
        }
        for (destination, exponent) in self
            .previous_gain
            .iter_mut()
            .zip(previous_amplitude_exponents.iter().copied())
        {
            *destination = 2.0_f32.powi(exponent);
            if !destination.is_finite() || *destination <= 0.0 {
                return Err(MpeghHoaInverseDynError::InvalidPreviousGain);
            }
        }
        Ok(())
    }

    pub fn process(
        &mut self,
        transport_pcm: &[Vec<f32>],
        exponents: &[i32],
        exceptions: &[bool],
        inverse_dynamic_window: &[f32],
    ) -> Result<Vec<Vec<f32>>, MpeghHoaInverseDynError> {
        let channels = self.previous_gain.len();
        if transport_pcm.len() != channels {
            return Err(MpeghHoaInverseDynError::ChannelCountMismatch {
                expected: channels,
                actual: transport_pcm.len(),
            });
        }
        if exponents.len() != channels || exceptions.len() != channels {
            return Err(MpeghHoaInverseDynError::ChannelMetadataCountMismatch {
                channels,
                metadata: exponents.len().min(exceptions.len()),
            });
        }
        let frame_size = transport_pcm
            .first()
            .map(Vec::len)
            .ok_or(MpeghHoaInverseDynError::ZeroTransportChannels)?;
        if frame_size == 0 {
            return Err(MpeghHoaInverseDynError::EmptyFrame);
        }
        if inverse_dynamic_window.len() < frame_size {
            return Err(MpeghHoaInverseDynError::WindowTooShort {
                required: frame_size,
                actual: inverse_dynamic_window.len(),
            });
        }
        if transport_pcm.iter().any(|channel| channel.len() != frame_size) {
            return Err(MpeghHoaInverseDynError::InconsistentFrameLength);
        }
        if inverse_dynamic_window[..frame_size]
            .iter()
            .any(|value| !value.is_finite() || *value <= 0.0)
        {
            return Err(MpeghHoaInverseDynError::InvalidWindow);
        }

        let mut output = Vec::with_capacity(channels);
        for channel in 0..channels {
            let last_gain = self.previous_gain[channel];
            if !last_gain.is_finite() || last_gain <= 0.0 {
                return Err(MpeghHoaInverseDynError::InvalidPreviousGain);
            }
            let reciprocal_last_gain = 1.0 / last_gain;
            let exponent = exponents[channel];
            let input = &transport_pcm[channel];
            let mut corrected = Vec::with_capacity(frame_size);

            if exponent == 0 {
                corrected.extend(input.iter().map(|sample| *sample * reciprocal_last_gain));
            } else if exceptions[channel] {
                let factor = 2.0_f32.powi(exponent) * reciprocal_last_gain;
                corrected.extend(input.iter().map(|sample| *sample * factor));
                self.previous_gain[channel] = last_gain * 2.0_f32.powi(-exponent);
            } else {
                for (sample, window) in input
                    .iter()
                    .zip(inverse_dynamic_window[..frame_size].iter())
                {
                    corrected.push(*sample * reciprocal_last_gain * window.powi(exponent));
                }
                self.previous_gain[channel] =
                    last_gain * inverse_dynamic_window[frame_size - 1].powi(exponent);
            }

            if corrected.iter().any(|sample| !sample.is_finite())
                || !self.previous_gain[channel].is_finite()
                || self.previous_gain[channel] <= 0.0
            {
                return Err(MpeghHoaInverseDynError::NonFiniteResult { channel });
            }
            output.push(corrected);
        }
        Ok(output)
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MpeghHoaInverseDynError {
    #[error("HOA inverse dynamic correction requires at least one transport channel")]
    ZeroTransportChannels,
    #[error("HOA inverse dynamic correction received an empty frame")]
    EmptyFrame,
    #[error("expected {expected} HOA transport channels but received {actual}")]
    ChannelCountMismatch { expected: usize, actual: usize },
    #[error("HOA channel metadata count does not match {channels} channels (received {metadata})")]
    ChannelMetadataCountMismatch { channels: usize, metadata: usize },
    #[error("HOA transport channels have inconsistent frame lengths")]
    InconsistentFrameLength,
    #[error("HOA inverse dynamic window needs {required} samples but has {actual}")]
    WindowTooShort { required: usize, actual: usize },
    #[error("HOA inverse dynamic window contains a non-finite or non-positive value")]
    InvalidWindow,
    #[error("HOA inverse dynamic previous gain is invalid")]
    InvalidPreviousGain,
    #[error("HOA inverse dynamic correction produced a non-finite result on channel {channel}")]
    NonFiniteResult { channel: usize },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coded_exponent_mapping_matches_reference_decoder() {
        assert_eq!(decode_gain_correction_exponent(1), 0);
        assert_eq!(decode_gain_correction_exponent(2), -1);
        assert_eq!(decode_gain_correction_exponent(3), 1);
        assert_eq!(decode_gain_correction_exponent(8), 6);
    }

    #[test]
    fn exponent_zero_only_removes_previous_gain() {
        let mut stage = MpeghHoaInverseDynCorrection::new(1).unwrap();
        stage.set_previous_gain_from_exponents(&[1]).unwrap();
        let output = stage
            .process(&[vec![2.0, 4.0]], &[0], &[false], &[0.5, 1.0])
            .unwrap();
        assert_eq!(output[0], vec![1.0, 2.0]);
        assert_eq!(stage.previous_gain(), &[2.0]);
    }

    #[test]
    fn exception_uses_constant_power_of_two_factor_and_updates_state() {
        let mut stage = MpeghHoaInverseDynCorrection::new(1).unwrap();
        let output = stage
            .process(&[vec![1.0, -1.0]], &[2], &[true], &[0.5, 1.0])
            .unwrap();
        assert_eq!(output[0], vec![4.0, -4.0]);
        assert!((stage.previous_gain()[0] - 0.25).abs() < 1.0e-7);
    }

    #[test]
    fn ordinary_path_applies_window_power_and_carries_terminal_gain() {
        let mut stage = MpeghHoaInverseDynCorrection::new(1).unwrap();
        let output = stage
            .process(&[vec![1.0, 1.0]], &[1], &[false], &[0.5, 1.0])
            .unwrap();
        assert_eq!(output[0], vec![0.5, 1.0]);
        assert!((stage.previous_gain()[0] - 1.0).abs() < 1.0e-7);
    }
}
