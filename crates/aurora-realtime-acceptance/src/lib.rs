//! Control-plane acceptance policy for Aurora realtime metrics.
//!
//! This crate deliberately sits outside the audio callback path. It turns an
//! already-collected [`RealTimeMetrics`] snapshot into an explicit PASS/FAIL
//! report and may allocate while constructing diagnostics.

use aurora_realtime_engine::{RealTimeFault, RealTimeMetrics};

/// Thresholds used to decide whether a realtime software run is acceptable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RealTimeAcceptancePolicy {
    /// Maximum accepted input underruns.
    pub max_input_underruns: u64,
    /// Maximum accepted output underruns.
    pub max_output_underruns: u64,
    /// Maximum accepted dropped blocks.
    pub max_dropped_blocks: u64,
    /// Maximum accepted p95 callback use of the configured block budget.
    pub max_p95_budget_usage_percent: f64,
    /// Optional upper bound for estimated software end-to-end latency.
    pub max_estimated_end_to_end_latency_frames: Option<usize>,
    /// Require at least one callback so an idle engine cannot pass a soak gate.
    pub require_callbacks: bool,
    /// Require the engine's persistent fault status to remain `None`.
    pub require_fault_free: bool,
}

impl Default for RealTimeAcceptancePolicy {
    fn default() -> Self {
        Self {
            max_input_underruns: 0,
            max_output_underruns: 0,
            max_dropped_blocks: 0,
            max_p95_budget_usage_percent: 100.0,
            max_estimated_end_to_end_latency_frames: None,
            require_callbacks: true,
            require_fault_free: true,
        }
    }
}

/// One reason a metrics snapshot failed its acceptance policy.
#[derive(Debug, Clone, PartialEq)]
pub enum RealTimeAcceptanceViolation {
    /// No callback was observed while callbacks were required.
    NoCallbacks,
    /// A persistent engine fault was present.
    Fault(RealTimeFault),
    /// Input underruns exceeded the configured maximum.
    InputUnderruns { actual: u64, maximum: u64 },
    /// Output underruns exceeded the configured maximum.
    OutputUnderruns { actual: u64, maximum: u64 },
    /// Dropped blocks exceeded the configured maximum.
    DroppedBlocks { actual: u64, maximum: u64 },
    /// The block duration was zero, so p95 budget usage could not be evaluated.
    MissingBlockBudget,
    /// p95 callback duration exceeded the configured share of the block budget.
    P95BudgetExceeded {
        actual_percent: f64,
        maximum_percent: f64,
    },
    /// Estimated software end-to-end latency exceeded the configured maximum.
    EstimatedLatencyExceeded {
        actual_frames: usize,
        maximum_frames: usize,
    },
    /// The policy contained an invalid p95 budget limit.
    InvalidP95BudgetLimit(f64),
}

/// Result of evaluating one realtime metrics snapshot against a policy.
#[derive(Debug, Clone, PartialEq)]
pub struct RealTimeAcceptanceReport {
    /// Whether all configured acceptance conditions passed.
    pub accepted: bool,
    /// Observed p95 callback usage of the configured block budget.
    pub p95_budget_usage_percent: Option<f64>,
    /// Every acceptance violation found in the snapshot.
    pub violations: Vec<RealTimeAcceptanceViolation>,
}

/// Evaluate collected realtime metrics against an explicit software policy.
///
/// This function is intended for control/validation code after or between
/// audio runs, not for execution inside an audio callback.
pub fn evaluate_realtime_acceptance(
    metrics: &RealTimeMetrics,
    policy: RealTimeAcceptancePolicy,
) -> RealTimeAcceptanceReport {
    let mut violations = Vec::new();

    if !policy.max_p95_budget_usage_percent.is_finite()
        || policy.max_p95_budget_usage_percent <= 0.0
    {
        violations.push(RealTimeAcceptanceViolation::InvalidP95BudgetLimit(
            policy.max_p95_budget_usage_percent,
        ));
    }

    if policy.require_callbacks && metrics.callback_count == 0 {
        violations.push(RealTimeAcceptanceViolation::NoCallbacks);
    }
    if policy.require_fault_free && metrics.fault != RealTimeFault::None {
        violations.push(RealTimeAcceptanceViolation::Fault(metrics.fault));
    }
    if metrics.input_underruns > policy.max_input_underruns {
        violations.push(RealTimeAcceptanceViolation::InputUnderruns {
            actual: metrics.input_underruns,
            maximum: policy.max_input_underruns,
        });
    }
    if metrics.output_underruns > policy.max_output_underruns {
        violations.push(RealTimeAcceptanceViolation::OutputUnderruns {
            actual: metrics.output_underruns,
            maximum: policy.max_output_underruns,
        });
    }
    if metrics.dropped_blocks > policy.max_dropped_blocks {
        violations.push(RealTimeAcceptanceViolation::DroppedBlocks {
            actual: metrics.dropped_blocks,
            maximum: policy.max_dropped_blocks,
        });
    }

    let p95_budget_usage_percent = budget_usage_percent(
        metrics.p95_callback_duration,
        metrics.block_duration_budget,
    );
    if metrics.callback_count > 0 && p95_budget_usage_percent.is_none() {
        violations.push(RealTimeAcceptanceViolation::MissingBlockBudget);
    }
    if let Some(actual_percent) = p95_budget_usage_percent {
        if policy.max_p95_budget_usage_percent.is_finite()
            && policy.max_p95_budget_usage_percent > 0.0
            && actual_percent > policy.max_p95_budget_usage_percent
        {
            violations.push(RealTimeAcceptanceViolation::P95BudgetExceeded {
                actual_percent,
                maximum_percent: policy.max_p95_budget_usage_percent,
            });
        }
    }

    if let Some(maximum_frames) = policy.max_estimated_end_to_end_latency_frames {
        if metrics.estimated_end_to_end_latency_frames > maximum_frames {
            violations.push(RealTimeAcceptanceViolation::EstimatedLatencyExceeded {
                actual_frames: metrics.estimated_end_to_end_latency_frames,
                maximum_frames,
            });
        }
    }

    RealTimeAcceptanceReport {
        accepted: violations.is_empty(),
        p95_budget_usage_percent,
        violations,
    }
}

fn budget_usage_percent(
    callback: std::time::Duration,
    budget: std::time::Duration,
) -> Option<f64> {
    if budget.is_zero() {
        return None;
    }
    Some(callback.as_secs_f64() / budget.as_secs_f64() * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn clean_metrics() -> RealTimeMetrics {
        let mut metrics = RealTimeMetrics::default();
        metrics.callback_count = 1_000;
        metrics.processed_blocks = 1_000;
        metrics.block_duration_budget = Duration::from_millis(10);
        metrics.p95_callback_duration = Duration::from_millis(4);
        metrics.estimated_end_to_end_latency_frames = 512;
        metrics
    }

    #[test]
    fn strict_default_accepts_clean_metrics() {
        let report = evaluate_realtime_acceptance(
            &clean_metrics(),
            RealTimeAcceptancePolicy::default(),
        );
        assert!(report.accepted);
        assert!(report.violations.is_empty());
        assert_eq!(report.p95_budget_usage_percent, Some(40.0));
    }

    #[test]
    fn strict_default_rejects_faults_underruns_and_drops() {
        let mut metrics = clean_metrics();
        metrics.input_underruns = 2;
        metrics.output_underruns = 3;
        metrics.dropped_blocks = 4;
        metrics.fault = RealTimeFault::Renderer;
        let report = evaluate_realtime_acceptance(
            &metrics,
            RealTimeAcceptancePolicy::default(),
        );
        assert!(!report.accepted);
        assert!(report
            .violations
            .contains(&RealTimeAcceptanceViolation::Fault(RealTimeFault::Renderer)));
        assert!(report
            .violations
            .contains(&RealTimeAcceptanceViolation::InputUnderruns {
                actual: 2,
                maximum: 0,
            }));
        assert!(report
            .violations
            .contains(&RealTimeAcceptanceViolation::OutputUnderruns {
                actual: 3,
                maximum: 0,
            }));
        assert!(report
            .violations
            .contains(&RealTimeAcceptanceViolation::DroppedBlocks {
                actual: 4,
                maximum: 0,
            }));
    }

    #[test]
    fn rejects_p95_over_budget_limit() {
        let mut metrics = clean_metrics();
        metrics.p95_callback_duration = Duration::from_millis(9);
        let policy = RealTimeAcceptancePolicy {
            max_p95_budget_usage_percent: 75.0,
            ..RealTimeAcceptancePolicy::default()
        };
        let report = evaluate_realtime_acceptance(&metrics, policy);
        assert!(!report.accepted);
        assert!(matches!(
            report.violations.as_slice(),
            [RealTimeAcceptanceViolation::P95BudgetExceeded {
                actual_percent,
                maximum_percent: 75.0,
            }] if (*actual_percent - 90.0).abs() < f64::EPSILON
        ));
    }

    #[test]
    fn optional_latency_limit_is_enforced() {
        let metrics = clean_metrics();
        let policy = RealTimeAcceptancePolicy {
            max_estimated_end_to_end_latency_frames: Some(256),
            ..RealTimeAcceptancePolicy::default()
        };
        let report = evaluate_realtime_acceptance(&metrics, policy);
        assert_eq!(
            report.violations,
            vec![RealTimeAcceptanceViolation::EstimatedLatencyExceeded {
                actual_frames: 512,
                maximum_frames: 256,
            }]
        );
    }

    #[test]
    fn idle_metrics_do_not_pass_default_policy() {
        let report = evaluate_realtime_acceptance(
            &RealTimeMetrics::default(),
            RealTimeAcceptancePolicy::default(),
        );
        assert!(!report.accepted);
        assert!(report
            .violations
            .contains(&RealTimeAcceptanceViolation::NoCallbacks));
    }

    #[test]
    fn missing_block_budget_is_rejected_after_callbacks() {
        let mut metrics = clean_metrics();
        metrics.block_duration_budget = Duration::ZERO;
        let report = evaluate_realtime_acceptance(
            &metrics,
            RealTimeAcceptancePolicy::default(),
        );
        assert!(report
            .violations
            .contains(&RealTimeAcceptanceViolation::MissingBlockBudget));
    }

    #[test]
    fn invalid_p95_limit_cannot_pass() {
        let report = evaluate_realtime_acceptance(
            &clean_metrics(),
            RealTimeAcceptancePolicy {
                max_p95_budget_usage_percent: f64::NAN,
                ..RealTimeAcceptancePolicy::default()
            },
        );
        assert!(matches!(
            report.violations.as_slice(),
            [RealTimeAcceptanceViolation::InvalidP95BudgetLimit(value)] if value.is_nan()
        ));
    }
}
