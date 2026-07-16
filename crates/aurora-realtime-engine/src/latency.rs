use thiserror::Error;

/// Physically captured or synthetic repeated-sequence latency estimate.
#[derive(Debug, Clone, PartialEq)]
pub struct LatencyMeasurementReport {
    /// Median detected round-trip offset in samples.
    pub median_samples: f64,
    /// Minimum valid offset in samples.
    pub minimum_samples: usize,
    /// Maximum valid offset in samples.
    pub maximum_samples: usize,
    /// Population standard deviation of valid offsets in samples.
    pub jitter_samples: f64,
    /// Rounded median sample offset.
    pub detected_sample_offset: usize,
    /// Average normalized correlation confidence.
    pub confidence: f32,
    /// Number of valid repeated measurements.
    pub valid_measurements: usize,
    /// Sample rate used for time conversion.
    pub sample_rate: u32,
}

impl LatencyMeasurementReport {
    /// Returns median round-trip latency in milliseconds.
    pub fn median_milliseconds(&self) -> f64 {
        self.median_samples / f64::from(self.sample_rate) * 1000.0
    }

    /// Returns minimum round-trip latency in milliseconds.
    pub fn minimum_milliseconds(&self) -> f64 {
        self.minimum_samples as f64 / f64::from(self.sample_rate) * 1000.0
    }

    /// Returns maximum round-trip latency in milliseconds.
    pub fn maximum_milliseconds(&self) -> f64 {
        self.maximum_samples as f64 / f64::from(self.sample_rate) * 1000.0
    }

    /// Returns jitter in milliseconds.
    pub fn jitter_milliseconds(&self) -> f64 {
        self.jitter_samples / f64::from(self.sample_rate) * 1000.0
    }
}

/// Latency estimator rejection reasons.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum LatencyEstimateError {
    /// No non-silent captured samples were supplied.
    #[error("latency measurement requires real non-silent captured input")]
    NoCapturedInput,
    /// No repeated correlation exceeded the confidence threshold.
    #[error("latency correlation confidence is too low: {confidence:.3}")]
    LowConfidence { confidence: f32 },
    /// Sequence, offsets, sample rate, or search window is invalid.
    #[error("invalid latency estimator configuration")]
    InvalidConfiguration,
}

/// Builds a deterministic bipolar pseudo-random measurement sequence.
pub fn generate_measurement_sequence(length: usize) -> Vec<f32> {
    let mut state = 0xACE1_u16;
    let mut sequence = Vec::with_capacity(length);
    for _ in 0..length {
        let bit = (state ^ (state >> 2) ^ (state >> 3) ^ (state >> 5)) & 1;
        state = (state >> 1) | (bit << 15);
        sequence.push(if state & 1 == 0 { -0.5 } else { 0.5 });
    }
    sequence
}

/// Estimates repeated round-trip offsets using normalized cross-correlation.
///
/// `emission_offsets` are positions in the captured clock domain where each
/// emitted sequence is expected before physical latency is added.
pub fn estimate_repeated_latency(
    sequence: &[f32],
    captured: &[f32],
    emission_offsets: &[usize],
    maximum_latency_samples: usize,
    minimum_confidence: f32,
    sample_rate: u32,
) -> Result<LatencyMeasurementReport, LatencyEstimateError> {
    if sequence.is_empty()
        || emission_offsets.is_empty()
        || maximum_latency_samples == 0
        || sample_rate == 0
        || !(0.0..=1.0).contains(&minimum_confidence)
    {
        return Err(LatencyEstimateError::InvalidConfiguration);
    }
    if !captured.iter().any(|sample| sample.abs() > 1.0e-6) {
        return Err(LatencyEstimateError::NoCapturedInput);
    }
    let reference_energy = sequence.iter().map(|sample| sample * sample).sum::<f32>();
    if reference_energy <= f32::EPSILON {
        return Err(LatencyEstimateError::InvalidConfiguration);
    }

    let mut offsets = Vec::with_capacity(emission_offsets.len());
    let mut confidences = Vec::with_capacity(emission_offsets.len());
    let mut best_rejected_confidence = 0.0_f32;
    for emission in emission_offsets {
        let mut best_confidence = 0.0_f32;
        let mut best_offset = 0_usize;
        for delay in 0..=maximum_latency_samples {
            let Some(start) = emission.checked_add(delay) else {
                continue;
            };
            let Some(end) = start.checked_add(sequence.len()) else {
                continue;
            };
            let Some(window) = captured.get(start..end) else {
                continue;
            };
            let mut dot = 0.0_f32;
            let mut captured_energy = 0.0_f32;
            for (reference, sample) in sequence.iter().zip(window) {
                dot += reference * sample;
                captured_energy += sample * sample;
            }
            let denominator = (reference_energy * captured_energy).sqrt();
            let confidence = if denominator > f32::EPSILON {
                (dot / denominator).abs()
            } else {
                0.0
            };
            if confidence > best_confidence {
                best_confidence = confidence;
                best_offset = delay;
            }
        }
        best_rejected_confidence = best_rejected_confidence.max(best_confidence);
        if best_confidence >= minimum_confidence {
            offsets.push(best_offset);
            confidences.push(best_confidence);
        }
    }
    if offsets.is_empty() {
        return Err(LatencyEstimateError::LowConfidence {
            confidence: best_rejected_confidence,
        });
    }
    offsets.sort_unstable();
    let median_samples = median(&offsets);
    let mean = offsets.iter().map(|value| *value as f64).sum::<f64>() / offsets.len() as f64;
    let jitter_samples = (offsets
        .iter()
        .map(|value| {
            let difference = *value as f64 - mean;
            difference * difference
        })
        .sum::<f64>()
        / offsets.len() as f64)
        .sqrt();
    let confidence = confidences.iter().sum::<f32>() / confidences.len() as f32;
    Ok(LatencyMeasurementReport {
        median_samples,
        minimum_samples: offsets.first().copied().unwrap_or(0),
        maximum_samples: offsets.last().copied().unwrap_or(0),
        jitter_samples,
        detected_sample_offset: median_samples.round() as usize,
        confidence,
        valid_measurements: offsets.len(),
        sample_rate,
    })
}

fn median(values: &[usize]) -> f64 {
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[middle - 1] as f64 + values[middle] as f64) * 0.5
    } else {
        values[middle] as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_delayed_sequences_report_known_offset() {
        let sequence = generate_measurement_sequence(127);
        let emissions = [100, 1_100, 2_100, 3_100];
        let delay = 237;
        let mut captured = vec![0.0_f32; 4_000];
        for emission in emissions {
            let start = emission + delay;
            for (target, sample) in captured[start..start + sequence.len()]
                .iter_mut()
                .zip(&sequence)
            {
                *target += *sample;
            }
        }
        let report =
            estimate_repeated_latency(&sequence, &captured, &emissions, 480, 0.8, 48_000).unwrap();
        assert_eq!(report.detected_sample_offset, delay);
        assert_eq!(report.valid_measurements, emissions.len());
        assert!(report.confidence > 0.99);
        assert_eq!(report.jitter_samples, 0.0);
    }

    #[test]
    fn low_confidence_capture_is_rejected() {
        let sequence = generate_measurement_sequence(127);
        let mut captured = vec![0.0_f32; 1_000];
        captured[200] = 0.01;
        let error =
            estimate_repeated_latency(&sequence, &captured, &[100], 400, 0.8, 48_000).unwrap_err();
        assert!(matches!(error, LatencyEstimateError::LowConfidence { .. }));
    }

    #[test]
    fn silent_capture_is_not_presented_as_measurement() {
        let sequence = generate_measurement_sequence(127);
        let error = estimate_repeated_latency(&sequence, &[0.0; 1_000], &[100], 400, 0.8, 48_000)
            .unwrap_err();
        assert_eq!(error, LatencyEstimateError::NoCapturedInput);
    }
}
