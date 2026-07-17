use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::thread;

use aurora_diagnostics::{
    BuildInformation, DiagnosticEvent, DiagnosticLog, DiagnosticReport, DiagnosticSnapshot,
    DiagnosticValue, EventId, EventTimestamp, FailureCategory, LogDisposition, MemorySummary,
    MetricSnapshot, QueueStatus, RealtimeMetricCounters, ReportValidationError, Reproducibility,
    RoutingEdge, Severity, SharedDiagnosticLog, SnapshotValidationError, TruthSource,
};

fn event(sequence: u64, severity: Severity) -> DiagnosticEvent {
    DiagnosticEvent::new(
        EventTimestamp::Logical(sequence),
        "test.component",
        severity,
        EventId::Startup,
        TruthSource::UnitTest,
    )
}

#[test]
fn every_required_event_id_serializes() {
    let identifiers = [
        EventId::Startup,
        EventId::Shutdown,
        EventId::DeviceDiscovery,
        EventId::CapabilityReport,
        EventId::RoutingDecision,
        EventId::RendererSelection,
        EventId::StateTransition,
        EventId::Underrun,
        EventId::Overrun,
        EventId::Recovery,
        EventId::ConfigurationValidation,
        EventId::SimulationExecution,
        EventId::BenchmarkExecution,
    ];

    for (sequence, event_id) in identifiers.into_iter().enumerate() {
        let value = DiagnosticEvent::new(
            EventTimestamp::Logical(sequence as u64),
            "contracts",
            Severity::Info,
            event_id,
            TruthSource::UnitTest,
        );
        let json = value.to_json().expect("event must serialize");
        let decoded: DiagnosticEvent = serde_json::from_str(&json).expect("event must parse");
        assert_eq!(decoded, value);
    }
}

#[test]
fn event_serialization_and_human_output_are_deterministic() {
    let first = event(7, Severity::Info)
        .with_field("zeta", DiagnosticValue::Unsigned(2))
        .with_field("alpha", DiagnosticValue::Text("ready".into()));
    let second = event(7, Severity::Info)
        .with_field("alpha", DiagnosticValue::Text("ready".into()))
        .with_field("zeta", DiagnosticValue::Unsigned(2));

    assert_eq!(first.to_json().unwrap(), second.to_json().unwrap());
    assert_eq!(first.to_human_line(), second.to_human_line());
    assert!(
        first.to_json().unwrap().find("alpha").unwrap()
            < first.to_json().unwrap().find("zeta").unwrap()
    );
}

#[test]
fn bounded_log_filters_and_evicts_oldest_events() {
    let mut log = DiagnosticLog::new(2, Severity::Info).unwrap();
    assert_eq!(
        log.push(event(0, Severity::Debug)),
        LogDisposition::Filtered
    );
    assert_eq!(log.push(event(1, Severity::Info)), LogDisposition::Retained);
    assert_eq!(
        log.push(event(2, Severity::Warning)),
        LogDisposition::Retained
    );
    assert_eq!(
        log.push(event(3, Severity::Error)),
        LogDisposition::RetainedAfterEviction
    );

    let timestamps: Vec<_> = log.events().map(|value| value.timestamp).collect();
    assert_eq!(
        timestamps,
        vec![EventTimestamp::Logical(2), EventTimestamp::Logical(3)]
    );
    assert_eq!(log.capacity(), 2);
    assert_eq!(log.filtered_events(), 1);
    assert_eq!(log.dropped_events(), 1);
    assert_eq!(log.to_json_lines().unwrap().lines().count(), 2);
    assert_eq!(log.to_human_lines().lines().count(), 2);
}

#[test]
fn bounded_log_rejects_oversized_payloads_observably() {
    let mut log = DiagnosticLog::with_max_event_bytes(4, Severity::Trace, 256).unwrap();
    let oversized =
        event(1, Severity::Error).with_field("detail", DiagnosticValue::Text("x".repeat(512)));
    assert_eq!(log.push(oversized), LogDisposition::RejectedOversized);
    assert_eq!(log.oversized_events(), 1);
    assert_eq!(log.events().len(), 0);
    assert_eq!(log.max_event_bytes(), 256);
}

#[test]
fn concurrent_control_thread_producers_remain_bounded() {
    let log = SharedDiagnosticLog::new(32, Severity::Trace).unwrap();
    let mut workers = Vec::new();
    for producer in 0..4 {
        let producer_log = log.clone();
        workers.push(thread::spawn(move || {
            for offset in 0..100 {
                let _ = producer_log
                    .push(event(producer * 100 + offset, Severity::Info))
                    .unwrap();
            }
        }));
    }
    for worker in workers {
        worker.join().unwrap();
    }

    log.inspect(|retained| {
        assert_eq!(retained.events().len(), 32);
        assert_eq!(retained.dropped_events(), 368);
    })
    .unwrap();
}

#[test]
fn metric_snapshot_contains_all_accumulated_counters() {
    let metrics = RealtimeMetricCounters::new();
    metrics.record_callback_execution(40);
    metrics.record_callback_execution(60);
    metrics.record_renderer_execution(25);
    metrics.record_queue_occupancy(9);
    metrics.record_queue_occupancy(4);
    metrics.record_underrun();
    metrics.record_overrun();
    metrics.record_recovery();
    metrics.record_dropped_frames(12);
    metrics.record_simulation_progress(48_000, 500_000_000);
    metrics.record_benchmark_sample(100);
    metrics.record_benchmark_sample(200);

    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.callback_count, 2);
    assert_eq!(snapshot.callback_execution_ns_total, 100);
    assert_eq!(snapshot.callback_execution_ns_max, 60);
    assert_eq!(snapshot.renderer_execution_ns_max, 25);
    assert_eq!(snapshot.queue_occupancy, 4);
    assert_eq!(snapshot.queue_occupancy_max, 9);
    assert_eq!(snapshot.underrun_count, 1);
    assert_eq!(snapshot.overrun_count, 1);
    assert_eq!(snapshot.recovery_count, 1);
    assert_eq!(snapshot.dropped_frames, 12);
    assert_eq!(snapshot.simulation_frames_per_second(), Some(96_000.0));
    assert_eq!(snapshot.benchmark_mean_ns(), Some(150.0));
}

#[test]
fn snapshot_generation_is_valid_and_deterministic() {
    let snapshot = DiagnosticSnapshot {
        schema_version: 1,
        active_configuration: BTreeMap::from([
            ("sample_rate".into(), DiagnosticValue::Unsigned(48_000)),
            ("layout".into(), DiagnosticValue::Text("7.1".into())),
        ]),
        selected_renderer: "vbap".into(),
        routing_graph: BTreeSet::from([
            RoutingEdge {
                source: "input.1".into(),
                destination: "FL".into(),
            },
            RoutingEdge {
                source: "input.2".into(),
                destination: "FR".into(),
            },
        ]),
        queue_status: QueueStatus {
            occupancy_frames: 64,
            capacity_frames: 256,
        },
        memory: MemorySummary {
            realtime_reserved_bytes: 8_192,
            diagnostics_retained_bytes: 1_024,
            diagnostics_capacity_bytes: 4_096,
        },
        enabled_features: BTreeSet::from(["diagnostics".into(), "simulation".into()]),
        build: BuildInformation {
            platform: "test-platform".into(),
            version: "0.1.0".into(),
            git_commit: "0123456789abcdef".into(),
        },
        metrics: MetricSnapshot::default(),
        truth_source: TruthSource::UnitTest,
    };

    let first = snapshot.to_json_pretty().unwrap();
    let second = snapshot.to_json_pretty().unwrap();
    assert_eq!(first, second);
    assert!(first.find("layout").unwrap() < first.find("sample_rate").unwrap());
    let decoded: DiagnosticSnapshot = serde_json::from_str(&first).unwrap();
    assert_eq!(decoded, snapshot);
    assert_eq!(snapshot.validate(), Ok(()));

    let mut invalid = snapshot.clone();
    invalid.queue_status.occupancy_frames = 257;
    assert_eq!(
        invalid.validate(),
        Err(SnapshotValidationError::QueueExceedsCapacity)
    );
}

#[test]
fn report_contains_reproducibility_recommendation_and_truth_source() {
    let report = DiagnosticReport {
        schema_version: 1,
        failure_category: FailureCategory::Simulation,
        root_component: "simulation.scheduler".into(),
        reproducibility: Reproducibility {
            operation: "cargo test -p aurora-simulation-assurance".into(),
            scenario: "seeded-fault-campaign".into(),
            deterministic_seed: Some(42),
        },
        recommendation: "rerun with the recorded seed".into(),
        truth_source: TruthSource::DeterministicSimulation,
        context: BTreeMap::from([("callback".into(), DiagnosticValue::Unsigned(12))]),
    };

    let json = report.to_json_pretty().unwrap();
    let decoded: DiagnosticReport = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, report);
    assert!(json.contains("deterministic_seed"));
    assert!(json.contains("recommendation"));
    assert!(json.contains("truth_source"));
    assert_eq!(report.validate(), Ok(()));

    let mut invalid = report.clone();
    invalid.reproducibility.deterministic_seed = None;
    assert_eq!(
        invalid.validate(),
        Err(ReportValidationError::MissingSimulationSeed)
    );
}

#[test]
fn concurrent_atomic_metric_updates_do_not_lose_counts() {
    let metrics = Arc::new(RealtimeMetricCounters::new());
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let metrics = Arc::clone(&metrics);
            thread::spawn(move || {
                for _ in 0..10_000 {
                    metrics.record_callback_execution(5);
                    metrics.record_underrun();
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }

    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.callback_count, 40_000);
    assert_eq!(snapshot.callback_execution_ns_total, 200_000);
    assert_eq!(snapshot.underrun_count, 40_000);
}
