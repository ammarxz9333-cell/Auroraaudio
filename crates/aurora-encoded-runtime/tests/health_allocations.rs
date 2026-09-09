use std::time::Duration;

use aurora_direct_earc_decoder::DirectEarcTransportTelemetry;
use aurora_encoded_runtime::health::{HealthReporter, RuntimeCounters};

#[global_allocator]
static ALLOCATOR: aurora_test_alloc::CountingAllocator = aurora_test_alloc::CountingAllocator;

#[test]
fn publication_and_coalescing_allocate_nothing_after_startup() {
    let snapshot = RuntimeCounters::default().snapshot(
        DirectEarcTransportTelemetry {
            pending_carrier_bytes: 0,
            discarded_bytes: 0,
            malformed_headers: 0,
            iec61937_locked: false,
            observation_epoch: 1,
            total_bursts: 0,
            bursts_since_lock: 0,
            total_format_changes: 0,
            relocks: 0,
            last_valid_burst_age_ms: None,
            last_burst_spacing_bytes: None,
            min_burst_spacing_bytes: None,
            max_burst_spacing_bytes: None,
        },
        None,
    );
    let reporter = HealthReporter::start(Duration::from_secs(60), snapshot, |_, _| {}).unwrap();
    assert_eq!(
        aurora_test_alloc::count_allocations(|| {
            for _ in 0..1000 {
                reporter.publish(snapshot);
            }
        }),
        0
    );
}
