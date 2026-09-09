use thiserror::Error;

use crate::{MpeghRenderedPcm, MpeghRenderedPcmError};

/// Evidence-only acceptance policy for comparing Aurora's speaker render with
/// the libmpegh reference render from the same compressed access unit.
///
/// These thresholds are Aurora engineering gates, not MPEG-H certification
/// limits. Product readiness must still be backed by the official conformance
/// corpus and independent evidence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MpeghConformancePolicy {
    pub minimum_correlation: f64,
    pub maximum_rms_error: f64,
    pub maximum_peak_absolute_error: f64,
    pub silence_rms_threshold: f64,
}

impl MpeghConformancePolicy {
    /// A deliberately strict near-reference gate suitable for regression CI.
    pub const fn near_reference() -> Self {
        Self {
            minimum_correlation: 0.999,
            maximum_rms_error: 1.0e-3,
            maximum_peak_absolute_error: 1.0e-2,
            silence_rms_threshold: 1.0e-7,
        }
    }

    fn validate(self) -> Result<(), MpeghConformanceError> {
        if !self.minimum_correlation.is_finite()
            || !(-1.0..=1.0).contains(&self.minimum_correlation)
            || !self.maximum_rms_error.is_finite()
            || self.maximum_rms_error < 0.0
            || !self.maximum_peak_absolute_error.is_finite()
            || self.maximum_peak_absolute_error < 0.0
            || !self.silence_rms_threshold.is_finite()
            || self.silence_rms_threshold < 0.0
        {
            return Err(MpeghConformanceError::InvalidPolicy);
        }
        Ok(())
    }
}

impl Default for MpeghConformancePolicy {
    fn default() -> Self {
        Self::near_reference()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MpeghChannelConformance {
    pub channel_index: usize,
    pub sample_count: usize,
    pub candidate_rms: f64,
    pub reference_rms: f64,
    pub rms_error: f64,
    pub peak_absolute_error: f64,
    /// Pearson correlation. `None` is used when either signal has effectively
    /// zero variance; in that case the absolute error gates decide the result.
    pub correlation: Option<f64>,
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MpeghConformanceReport {
    pub sample_rate: u32,
    pub channel_count: usize,
    pub frame_count: usize,
    pub channels: Vec<MpeghChannelConformance>,
    pub passed: bool,
}

/// Compare planar F32 speaker output from Aurora against libmpegh's rendered
/// PCM companion captured from the exact same decoder execute call.
pub fn compare_mpegh_render_to_reference(
    candidate: &[Vec<f32>],
    reference: &MpeghRenderedPcm,
    policy: MpeghConformancePolicy,
) -> Result<MpeghConformanceReport, MpeghConformanceError> {
    policy.validate()?;
    let reference_channels = reference.decode_planar_f32()?;

    if candidate.len() != reference.channel_count {
        return Err(MpeghConformanceError::ChannelCountMismatch {
            candidate: candidate.len(),
            reference: reference.channel_count,
        });
    }

    for (channel_index, channel) in candidate.iter().enumerate() {
        if channel.len() != reference.frame_count {
            return Err(MpeghConformanceError::FrameCountMismatch {
                channel: channel_index,
                candidate: channel.len(),
                reference: reference.frame_count,
            });
        }
    }

    let mut channel_reports = Vec::with_capacity(candidate.len());
    for (channel_index, (candidate_channel, reference_channel)) in candidate
        .iter()
        .zip(reference_channels.iter())
        .enumerate()
    {
        channel_reports.push(compare_channel(
            channel_index,
            candidate_channel,
            reference_channel,
            policy,
        )?);
    }

    let passed = channel_reports.iter().all(|channel| channel.passed);
    Ok(MpeghConformanceReport {
        sample_rate: reference.sample_rate,
        channel_count: reference.channel_count,
        frame_count: reference.frame_count,
        channels: channel_reports,
        passed,
    })
}

fn compare_channel(
    channel_index: usize,
    candidate: &[f32],
    reference: &[f32],
    policy: MpeghConformancePolicy,
) -> Result<MpeghChannelConformance, MpeghConformanceError> {
    let sample_count = candidate.len();
    if sample_count == 0 {
        return Err(MpeghConformanceError::EmptyFrame);
    }

    let mut sum_candidate = 0.0_f64;
    let mut sum_reference = 0.0_f64;
    let mut sum_candidate_sq = 0.0_f64;
    let mut sum_reference_sq = 0.0_f64;
    let mut sum_cross = 0.0_f64;
    let mut sum_error_sq = 0.0_f64;
    let mut peak_absolute_error = 0.0_f64;

    for (sample_index, (&candidate_sample, &reference_sample)) in
        candidate.iter().zip(reference.iter()).enumerate()
    {
        if !candidate_sample.is_finite() {
            return Err(MpeghConformanceError::NonFiniteCandidate {
                channel: channel_index,
                sample: sample_index,
            });
        }
        if !reference_sample.is_finite() {
            return Err(MpeghConformanceError::NonFiniteReference {
                channel: channel_index,
                sample: sample_index,
            });
        }

        let x = f64::from(candidate_sample);
        let y = f64::from(reference_sample);
        let error = x - y;
        sum_candidate += x;
        sum_reference += y;
        sum_candidate_sq += x * x;
        sum_reference_sq += y * y;
        sum_cross += x * y;
        sum_error_sq += error * error;
        peak_absolute_error = peak_absolute_error.max(error.abs());
    }

    let count = sample_count as f64;
    let candidate_rms = (sum_candidate_sq / count).sqrt();
    let reference_rms = (sum_reference_sq / count).sqrt();
    let rms_error = (sum_error_sq / count).sqrt();

    let candidate_variance =
        (sum_candidate_sq - (sum_candidate * sum_candidate / count)).max(0.0);
    let reference_variance =
        (sum_reference_sq - (sum_reference * sum_reference / count)).max(0.0);
    let covariance = sum_cross - (sum_candidate * sum_reference / count);
    let variance_floor = 1.0e-24_f64;
    let correlation = if candidate_variance <= variance_floor || reference_variance <= variance_floor
    {
        None
    } else {
        Some(
            (covariance / (candidate_variance * reference_variance).sqrt()).clamp(-1.0, 1.0),
        )
    };

    let error_pass = rms_error <= policy.maximum_rms_error
        && peak_absolute_error <= policy.maximum_peak_absolute_error;
    let both_effectively_silent = candidate_rms <= policy.silence_rms_threshold
        && reference_rms <= policy.silence_rms_threshold;
    let correlation_pass = if both_effectively_silent {
        true
    } else {
        correlation
            .map(|value| value >= policy.minimum_correlation)
            // Constant/non-silent channels have undefined Pearson correlation;
            // require the absolute error gates instead of manufacturing 1.0.
            .unwrap_or(error_pass)
    };

    Ok(MpeghChannelConformance {
        channel_index,
        sample_count,
        candidate_rms,
        reference_rms,
        rms_error,
        peak_absolute_error,
        correlation,
        passed: error_pass && correlation_pass,
    })
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum MpeghConformanceError {
    #[error(transparent)]
    Reference(#[from] MpeghRenderedPcmError),
    #[error("invalid MPEG-H conformance threshold policy")]
    InvalidPolicy,
    #[error("MPEG-H conformance frame contains zero samples")]
    EmptyFrame,
    #[error("candidate has {candidate} channels but reference has {reference}")]
    ChannelCountMismatch { candidate: usize, reference: usize },
    #[error("candidate channel {channel} has {candidate} frames but reference has {reference}")]
    FrameCountMismatch {
        channel: usize,
        candidate: usize,
        reference: usize,
    },
    #[error("candidate channel {channel} sample {sample} is non-finite")]
    NonFiniteCandidate { channel: usize, sample: usize },
    #[error("reference channel {channel} sample {sample} is non-finite")]
    NonFiniteReference { channel: usize, sample: usize },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference_s16(samples: &[[i16; 2]]) -> MpeghRenderedPcm {
        let mut bytes = Vec::with_capacity(samples.len() * 4);
        for frame in samples {
            bytes.extend_from_slice(&frame[0].to_le_bytes());
            bytes.extend_from_slice(&frame[1].to_le_bytes());
        }
        MpeghRenderedPcm {
            bytes,
            bit_depth: 16,
            channel_count: 2,
            frame_count: samples.len(),
            sample_rate: 48_000,
        }
    }

    #[test]
    fn identical_render_passes_near_reference_gate() {
        let reference = reference_s16(&[[16_384, -16_384], [8_192, -8_192]]);
        let candidate = reference.decode_planar_f32().unwrap();
        let report = compare_mpegh_render_to_reference(
            &candidate,
            &reference,
            MpeghConformancePolicy::near_reference(),
        )
        .unwrap();
        assert!(report.passed);
        assert!(report
            .channels
            .iter()
            .all(|channel| channel.rms_error <= f64::EPSILON));
    }

    #[test]
    fn inverted_channel_fails_correlation_gate() {
        let reference = reference_s16(&[[16_384, 0], [-16_384, 0], [8_192, 0], [-8_192, 0]]);
        let mut candidate = reference.decode_planar_f32().unwrap();
        for sample in &mut candidate[0] {
            *sample = -*sample;
        }
        let report = compare_mpegh_render_to_reference(
            &candidate,
            &reference,
            MpeghConformancePolicy::near_reference(),
        )
        .unwrap();
        assert!(!report.passed);
        assert!(report.channels[0].correlation.unwrap() < -0.99);
    }

    #[test]
    fn rejects_channel_count_mismatch() {
        let reference = reference_s16(&[[0, 0]]);
        let candidate = vec![vec![0.0]];
        assert!(matches!(
            compare_mpegh_render_to_reference(
                &candidate,
                &reference,
                MpeghConformancePolicy::near_reference(),
            ),
            Err(MpeghConformanceError::ChannelCountMismatch { .. })
        ));
    }
}
