use std::hint::black_box;

use aurora_diagnostics::{
    DiagnosticEvent, DiagnosticLog, EventId, EventTimestamp, RealtimeMetricCounters, Severity,
    TruthSource,
};
use criterion::{criterion_group, criterion_main, Criterion};

fn callback_metrics(c: &mut Criterion) {
    let metrics = RealtimeMetricCounters::new();
    c.bench_function("diagnostics_callback_atomic_updates", |b| {
        b.iter(|| {
            metrics.record_callback_execution(black_box(25_000));
            metrics.record_renderer_execution(black_box(18_000));
            metrics.record_queue_occupancy(black_box(128));
        });
    });
}

fn bounded_control_log(c: &mut Criterion) {
    let mut sequence = 0_u64;
    let mut log = DiagnosticLog::new(256, Severity::Info).unwrap();
    c.bench_function("diagnostics_bounded_control_log", |b| {
        b.iter(|| {
            let _ = log.push(DiagnosticEvent::new(
                EventTimestamp::Logical(sequence),
                "benchmark",
                Severity::Info,
                EventId::BenchmarkExecution,
                TruthSource::HostApiObservation,
            ));
            sequence = sequence.wrapping_add(1);
        });
    });
}

criterion_group!(benches, callback_metrics, bounded_control_log);
criterion_main!(benches);
