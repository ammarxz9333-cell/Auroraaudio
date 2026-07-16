//! Deterministic virtual audio hardware for Aurora validation.
//!
//! The simulator owns its public configuration and report types. It depends on
//! Aurora's backend and realtime-engine contracts, never on CPAL.

mod backend;
mod clock;
mod loopback;
mod profile;
mod simulation;

pub use backend::SimAudioBackend;
pub use clock::{CallbackEvent, DeterministicRng, VirtualClock, VirtualScheduler};
pub use loopback::{simulate_latency, LatencySimulationConfig, SimulatedLatencyReport};
pub use profile::{
    builtin_profile, load_fault_timeline, CallbackSizePolicy, FaultAction, FaultEvent,
    SimulationProfile, VirtualDevice, VirtualSampleFormat,
};
pub use simulation::{
    run_duplex_simulation, validate_output_routing, DuplexSimulationConfig, DuplexSimulationReport,
    OutputValidationReport, SimulationError, StateTransitionRecord,
};

#[cfg(test)]
mod allocation_audit {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    pub struct CountingAllocator;

    thread_local! {
        static ACTIVE: Cell<bool> = const { Cell::new(false) };
        static COUNT: Cell<usize> = const { Cell::new(0) };
    }

    unsafe impl GlobalAlloc for CountingAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ACTIVE.with(|active| {
                if active.get() {
                    COUNT.with(|count| count.set(count.get() + 1));
                }
            });
            System.alloc(layout)
        }

        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            System.dealloc(pointer, layout);
        }

        unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            ACTIVE.with(|active| {
                if active.get() {
                    COUNT.with(|count| count.set(count.get() + 1));
                }
            });
            System.realloc(pointer, layout, size)
        }
    }

    pub fn count_allocations(action: impl FnOnce()) -> usize {
        COUNT.with(|count| count.set(0));
        ACTIVE.with(|active| active.set(true));
        action();
        ACTIVE.with(|active| active.set(false));
        COUNT.with(Cell::get)
    }
}

#[cfg(test)]
#[global_allocator]
static TEST_ALLOCATOR: allocation_audit::CountingAllocator = allocation_audit::CountingAllocator;
