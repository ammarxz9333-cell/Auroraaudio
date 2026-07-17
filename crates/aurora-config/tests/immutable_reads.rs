mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;

use aurora_config::ValidatedConfiguration;
use common::stereo;

struct CountingAllocator;

thread_local! {
    static ENABLED: Cell<bool> = const { Cell::new(false) };
    static OPERATIONS: Cell<usize> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ENABLED.with(|enabled| {
            if enabled.get() {
                OPERATIONS.with(|count| count.set(count.get().saturating_add(1)));
            }
        });
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ENABLED.with(|enabled| {
            if enabled.get() {
                OPERATIONS.with(|count| count.set(count.get().saturating_add(1)));
            }
        });
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn immutable_field_reads_allocate_zero_times() {
    let validated = ValidatedConfiguration::new(stereo()).unwrap();
    OPERATIONS.with(|count| count.set(0));
    ENABLED.with(|enabled| enabled.set(true));
    for _ in 0..10_000 {
        black_box(validated.config().audio_format.sample_rate);
        black_box(validated.config().speaker_layout.speakers.as_slice());
        black_box(&validated.config().renderer);
    }
    ENABLED.with(|enabled| enabled.set(false));
    assert_eq!(OPERATIONS.with(Cell::get), 0);
}
