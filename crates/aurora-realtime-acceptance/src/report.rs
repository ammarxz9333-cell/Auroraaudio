use std::time::Duration;

use aurora_realtime_engine::RealTimeMetrics;
use serde::Serialize;

use crate::{RealTimeAcceptancePolicy, RealTimeAcceptanceReport};

/// Stable schema version emitted by [`RealTimeHealthReportV1`].
pub const REALTIME_HEALTH_REPORT_SCHEMA_VERSION: u32 = 1;

/// Serialized acceptance policy used for one realtime run.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RealTimeHealthPolicyV1 {
    pub max_input_underruns: u64,
    pub max_output_underruns: u64,
    pub max_dropped_blocks: u64,
    pub max_p95_budget_usage_percent: f64,
    pub max_estimated_end_to_end_latency_frames: Option<usize>,
    pub require_callbacks: bool,
    pub require_fault_free: bool,
}

impl From<RealTimeAcceptancePolicy> for RealTimeHealthPolicyV1 {
    fn from(policy: RealTimeAcceptancePolicy) -> Self {
        Self {
            max_input_underruns: policy.max_input_underruns,
            max_output_underruns: policy.max_output_underruns,
            max_dropped_blocks: policy.max_dropped_blocks,
            max_p95_budget_usage_percent: policy.max_p95_budget_usage_percent,
            max_estimated_end_to_end_latency_frames: policy.max_estimated_end_to_end_latency_frames,
            require_callbacks: policy.require_callbacks,
            require_fault_free: policy.require_fault_free,
        }
    }
}

/// Serialized realtime software metrics used by the acceptance decision.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RealTimeHealthMetricsV1 {
    pub callback_count: u64,
    pub processed_blocks: u64,
    pub input_underruns: u64,
    pub output_underruns: u64,
    pub dropped_blocks: u64,
    pub max_callback_ms: f64,
    pub average_callback_ms: f64,
    pub p95_callback_ms: f64,
    pub block_duration_budget_ms: f64,
    pub renderer_latency_frames: usize,
    pub dsp_latency_frames: usize,
    pub estimated_device_latency_frames: usize,
    pub estimated_end_to_end_latency_frames: usize,
    pub fault_code: u64,
}

impl From<&RealTimeMetrics> for RealTimeHealthMetricsV1 {
    fn from(metrics: &RealTimeMetrics) -> Self {
        Self {
            callback_count: metrics.callback_count,
            processed_blocks: metrics.processed_blocks,
            input_underruns: metrics.input_underruns,
            output_underruns: metrics.output_underruns,
            dropped_blocks: metrics.dropped_blocks,
            max_callback_ms: duration_milliseconds(metrics.max_callback_duration),
            average_callback_ms: duration_milliseconds(metrics.average_callback_duration),
            p95_callback_ms: duration_milliseconds(metrics.p95_callback_duration),
            block_duration_budget_ms: duration_milliseconds(metrics.block_duration_budget),
            renderer_latency_frames: metrics.renderer_latency_frames,
            dsp_latency_frames: metrics.dsp_latency_frames,
            estimated_device_latency_frames: metrics.estimated_device_latency_frames,
            estimated_end_to_end_latency_frames: metrics.estimated_end_to_end_latency_frames,
            fault_code: metrics.fault as u64,
        }
    }
}

/// Stable machine-readable realtime health report emitted by Aurora control-plane code.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RealTimeHealthReportV1 {
    pub schema_version: u32,
    pub accepted: bool,
    pub p95_budget_usage_percent: Option<f64>,
    pub policy: RealTimeHealthPolicyV1,
    pub metrics: RealTimeHealthMetricsV1,
    pub violations: Vec<String>,
}

impl RealTimeHealthReportV1 {
    /// Build a schema-v1 document from the exact metrics and policy already evaluated.
    pub fn from_evaluation(
        metrics: &RealTimeMetrics,
        policy: RealTimeAcceptancePolicy,
        health: &RealTimeAcceptanceReport,
    ) -> Self {
        Self {
            schema_version: REALTIME_HEALTH_REPORT_SCHEMA_VERSION,
            accepted: health.accepted,
            p95_budget_usage_percent: health.p95_budget_usage_percent,
            policy: policy.into(),
            metrics: metrics.into(),
            violations: health
                .violations
                .iter()
                .map(|violation| format!("{violation:?}"))
                .collect(),
        }
    }

    /// Serialize this stable report as pretty JSON bytes.
    pub fn to_pretty_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec_pretty(self)
    }
}

fn duration_milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{evaluate_realtime_acceptance, RealTimeAcceptanceViolation};
    use aurora_realtime_engine::{RealTimeFault, RealTimeMetrics};

    fn passing_metrics() -> RealTimeMetrics {
        let mut metrics = RealTimeMetrics::default();
        metrics.callback_count = 20;
        metrics.processed_blocks = 20;
        metrics.block_duration_budget = Duration::from_millis(10);
        metrics.max_callback_duration = Duration::from_millis(5);
        metrics.average_callback_duration = Duration::from_millis(2);
        metrics.p95_callback_duration = Duration::from_millis(4);
        metrics.renderer_latency_frames = 16;
        metrics.dsp_latency_frames = 32;
        metrics.estimated_device_latency_frames = 64;
        metrics.estimated_end_to_end_latency_frames = 112;
        metrics
    }

    #[test]
    fn report_v1_serializes_stable_top_level_contract() {
        let metrics = passing_metrics();
        let policy = RealTimeAcceptancePolicy::default();
        let health = evaluate_realtime_acceptance(&metrics, policy);
        let document = RealTimeHealthReportV1::from_evaluation(&metrics, policy, &health);
        let value: serde_json::Value =
            serde_json::from_slice(&document.to_pretty_json().unwrap()).unwrap();

        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["accepted"], true);
        assert_eq!(value["p95_budget_usage_percent"], 40.0);
        assert_eq!(value["metrics"]["callback_count"], 20);
        assert_eq!(value["metrics"]["estimated_end_to_end_latency_frames"], 112);
        assert_eq!(value["policy"]["max_input_underruns"], 0);
        assert_eq!(value["violations"], serde_json::json!([]));
    }

    #[test]
    fn report_v1_preserves_failed_acceptance_evidence() {
        let mut metrics = passing_metrics();
        metrics.output_underruns = 1;
        metrics.dropped_blocks = 1;
        metrics.fault = RealTimeFault::OutputBuffer;
        let policy = RealTimeAcceptancePolicy::default();
        let health = evaluate_realtime_acceptance(&metrics, policy);
        let document = RealTimeHealthReportV1::from_evaluation(&metrics, policy, &health);

        assert!(!document.accepted);
        assert_eq!(document.metrics.output_underruns, 1);
        assert_eq!(document.metrics.dropped_blocks, 1);
        assert_eq!(
            document.metrics.fault_code,
            RealTimeFault::OutputBuffer as u64
        );
        assert!(document
            .violations
            .iter()
            .any(|entry| entry.contains("OutputUnderruns")));
        assert!(health
            .violations
            .contains(&RealTimeAcceptanceViolation::DroppedBlocks {
                actual: 1,
                maximum: 0,
            }));
    }
}
