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
