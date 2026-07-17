use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use aurora_diagnostics::RealtimeMetricCounters;

struct CountingAllocator;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count_allocation();
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count_allocation();
        unsafe { System.realloc(pointer, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn count_allocation() {
    COUNTING.with(|counting| {
        if counting.get() {
            ALLOCATIONS.with(|allocations| {
                allocations.set(allocations.get().saturating_add(1));
            });
        }
    });
}

fn measured_allocations(operation: impl FnOnce()) -> usize {
    ALLOCATIONS.with(|allocations| allocations.set(0));
    COUNTING.with(|counting| counting.set(true));
    operation();
    COUNTING.with(|counting| counting.set(false));
    ALLOCATIONS.with(Cell::get)
}

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
