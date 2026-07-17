use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use aurora_diagnostics::RealtimeMetricCounters;

struct CountingAllocator;

static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn callback_metric_recording_allocates_zero_times() {
    let metrics = RealtimeMetricCounters::new();
    ALLOCATIONS.store(0, Ordering::SeqCst);
    COUNTING.store(true, Ordering::SeqCst);
    for index in 0..10_000 {
        metrics.record_callback_execution(index);
        metrics.record_renderer_execution(index / 2);
        metrics.record_queue_occupancy(index % 256);
        metrics.record_underrun();
        metrics.record_overrun();
        metrics.record_recovery();
        metrics.record_dropped_frames(1);
    }
    COUNTING.store(false, Ordering::SeqCst);

    assert_eq!(ALLOCATIONS.load(Ordering::SeqCst), 0);
    assert_eq!(metrics.snapshot().callback_count, 10_000);
}
