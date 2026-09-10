//! Fixed-capacity latency aggregation for the headless Aurora validation harness.
//!
//! Recording is allocation-free and constant-space. Reporting happens outside the
//! realtime path and returns only copyable numeric summaries.

use std::time::Duration;

const BUCKET_WIDTH_US: u64 = 100;
const BUCKETS: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationStage {
    Capture,
    Parser,
    Decode,
    JocRender,
    SpeakerPostProcessor,
    Output,
}

impl ValidationStage {
    pub const ALL: [Self; 6] = [
        Self::Capture,
        Self::Parser,
        Self::Decode,
        Self::JocRender,
        Self::SpeakerPostProcessor,
        Self::Output,
    ];

    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Capture => 0,
            Self::Parser => 1,
            Self::Decode => 2,
            Self::JocRender => 3,
            Self::SpeakerPostProcessor => 4,
            Self::Output => 5,
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Capture => "capture",
            Self::Parser => "parser",
            Self::Decode => "decode",
            Self::JocRender => "joc_render",
            Self::SpeakerPostProcessor => "speaker_postprocessor",
            Self::Output => "output",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LatencySummary {
    pub samples: u64,
    pub p50_us: u64,
    pub p99_us: u64,
    pub max_us: u64,
    pub overflow_samples: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedLatencyHistogram {
    buckets: [u64; BUCKETS],
    samples: u64,
    max_us: u64,
    overflow_samples: u64,
}

impl Default for FixedLatencyHistogram {
    fn default() -> Self {
        Self::new()
    }
}

impl FixedLatencyHistogram {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            buckets: [0; BUCKETS],
            samples: 0,
            max_us: 0,
            overflow_samples: 0,
        }
    }

    /// Records one duration without allocation, locks, formatting or system I/O.
    pub fn record(&mut self, duration: Duration) {
        let micros = duration.as_micros().min(u128::from(u64::MAX)) as u64;
        self.record_us(micros);
    }

    /// Records one microsecond value without allocation.
    pub fn record_us(&mut self, micros: u64) {
        self.samples = self.samples.saturating_add(1);
        self.max_us = self.max_us.max(micros);
        let bucket = micros / BUCKET_WIDTH_US;
        if let Ok(index) = usize::try_from(bucket) {
            if let Some(slot) = self.buckets.get_mut(index) {
                *slot = slot.saturating_add(1);
                return;
            }
        }
        self.overflow_samples = self.overflow_samples.saturating_add(1);
    }

    #[must_use]
    pub fn summary(&self) -> LatencySummary {
        if self.samples == 0 {
            return LatencySummary::default();
        }
        LatencySummary {
            samples: self.samples,
            p50_us: self.percentile_us(50),
            p99_us: self.percentile_us(99),
            max_us: self.max_us,
            overflow_samples: self.overflow_samples,
        }
    }

    fn percentile_us(&self, percentile: u64) -> u64 {
        let target = self
            .samples
            .saturating_mul(percentile)
            .saturating_add(99)
            / 100;
        let mut cumulative = 0_u64;
        for (index, count) in self.buckets.iter().copied().enumerate() {
            cumulative = cumulative.saturating_add(count);
            if cumulative >= target {
                return (index as u64 + 1).saturating_mul(BUCKET_WIDTH_US);
            }
        }
        self.max_us
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageLatencyBook {
    stages: [FixedLatencyHistogram; 6],
}

impl Default for StageLatencyBook {
    fn default() -> Self {
        Self::new()
    }
}

impl StageLatencyBook {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            stages: [
                FixedLatencyHistogram::new(),
                FixedLatencyHistogram::new(),
                FixedLatencyHistogram::new(),
                FixedLatencyHistogram::new(),
                FixedLatencyHistogram::new(),
                FixedLatencyHistogram::new(),
            ],
        }
    }

    pub fn record(&mut self, stage: ValidationStage, duration: Duration) {
        self.stages[stage.index()].record(duration);
    }

    pub fn record_us(&mut self, stage: ValidationStage, micros: u64) {
        self.stages[stage.index()].record_us(micros);
    }

    #[must_use]
    pub fn summary(&self, stage: ValidationStage) -> LatencySummary {
        self.stages[stage.index()].summary()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_and_max_are_bounded_without_sample_storage() {
        let mut histogram = FixedLatencyHistogram::new();
        for value in 0..100_u64 {
            histogram.record_us(value * 100);
        }
        let summary = histogram.summary();
        assert_eq!(summary.samples, 100);
        assert_eq!(summary.p50_us, 5_000);
        assert_eq!(summary.p99_us, 9_900);
        assert_eq!(summary.max_us, 9_900);
        assert_eq!(summary.overflow_samples, 0);
    }

    #[test]
    fn overflow_keeps_exact_max_and_fail_visible_count() {
        let mut histogram = FixedLatencyHistogram::new();
        histogram.record_us(100);
        histogram.record_us(75_000);
        let summary = histogram.summary();
        assert_eq!(summary.samples, 2);
        assert_eq!(summary.p50_us, 200);
        assert_eq!(summary.p99_us, 75_000);
        assert_eq!(summary.max_us, 75_000);
        assert_eq!(summary.overflow_samples, 1);
    }

    #[test]
    fn stage_book_does_not_mix_measurement_domains() {
        let mut book = StageLatencyBook::new();
        book.record_us(ValidationStage::Parser, 80);
        book.record_us(ValidationStage::Decode, 1_250);
        assert_eq!(book.summary(ValidationStage::Parser).samples, 1);
        assert_eq!(book.summary(ValidationStage::Decode).samples, 1);
        assert_eq!(book.summary(ValidationStage::Output).samples, 0);
    }
}
