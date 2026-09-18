//! Deterministic virtual audio hardware for Aurora validation.
//!
//! The simulator owns its public configuration and report types. It depends on
//! Aurora's backend and realtime-engine contracts, never on CPAL.

mod backend;
mod clock;
mod head_tracker;
mod loopback;
mod network;
mod profile;
mod simulation;

pub use backend::SimAudioBackend;
pub use clock::{CallbackEvent, DeterministicRng, VirtualClock, VirtualScheduler};
pub use head_tracker::{
    simulate_head_tracker_delivery, HeadTrackerDeliveryEvent, HeadTrackerFaultProfile,
    HeadTrackerSimulationConfig, HeadTrackerSimulationError, SimulatedHeadTrackerAnchor,
    SimulatedHeadTrackerSample,
};
pub use loopback::{simulate_latency, LatencySimulationConfig, SimulatedLatencyReport};
pub use network::{SimNetworkTransport, SimulatedNetworkBlock};
pub use profile::{
    builtin_profile, load_fault_timeline, CallbackSizePolicy, FaultAction, FaultEvent,
    SimulationProfile, VirtualDevice, VirtualSampleFormat,
};
pub use simulation::{
    run_duplex_simulation, validate_output_routing, DuplexSimulationConfig, DuplexSimulationReport,
    OutputValidationReport, SimulationError, StateTransitionRecord,
};

#[cfg(test)]
use aurora_test_alloc as allocation_audit;

#[cfg(test)]
#[global_allocator]
static TEST_ALLOCATOR: allocation_audit::CountingAllocator = allocation_audit::CountingAllocator;
