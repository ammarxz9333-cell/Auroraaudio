use aurora_diagnostics::RealtimeMetricCounters;
use aurora_test_alloc::{count_allocations as measured_allocations, CountingAllocator};
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn callback_metric_recording_allocates_zero_times() {
    let metrics = RealtimeMetricCounters::new();
    let allocations = measured_allocations(|| {
        for index in 0..10_000 {
            metrics.record_callback_execution(index);
            metrics.record_renderer_execution(index / 2);
            metrics.record_queue_occupancy(index % 256);
            metrics.record_underrun();
            metrics.record_overrun();
            metrics.record_recovery();
            metrics.record_dropped_frames(1);
        }
    });

    assert_eq!(allocations, 0);
    assert_eq!(metrics.snapshot().callback_count, 10_000);
}
