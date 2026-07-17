use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// Fixed atomic counters safe to update from real-time callbacks.
#[derive(Debug, Default)]
pub struct RealtimeMetricCounters {
    callback_count: AtomicU64,
    callback_execution_ns_total: AtomicU64,
    callback_execution_ns_max: AtomicU64,
    renderer_count: AtomicU64,
    renderer_execution_ns_total: AtomicU64,
    renderer_execution_ns_max: AtomicU64,
    queue_occupancy: AtomicU64,
    queue_occupancy_max: AtomicU64,
    underrun_count: AtomicU64,
    overrun_count: AtomicU64,
    recovery_count: AtomicU64,
    dropped_frames: AtomicU64,
    simulation_frames: AtomicU64,
    simulation_elapsed_ns: AtomicU64,
    benchmark_samples: AtomicU64,
    benchmark_elapsed_ns_total: AtomicU64,
    benchmark_elapsed_ns_max: AtomicU64,
}

impl RealtimeMetricCounters {
    /// Creates zeroed metrics without dynamic storage.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            callback_count: AtomicU64::new(0),
            callback_execution_ns_total: AtomicU64::new(0),
            callback_execution_ns_max: AtomicU64::new(0),
            renderer_count: AtomicU64::new(0),
            renderer_execution_ns_total: AtomicU64::new(0),
            renderer_execution_ns_max: AtomicU64::new(0),
            queue_occupancy: AtomicU64::new(0),
            queue_occupancy_max: AtomicU64::new(0),
            underrun_count: AtomicU64::new(0),
            overrun_count: AtomicU64::new(0),
            recovery_count: AtomicU64::new(0),
            dropped_frames: AtomicU64::new(0),
            simulation_frames: AtomicU64::new(0),
            simulation_elapsed_ns: AtomicU64::new(0),
            benchmark_samples: AtomicU64::new(0),
            benchmark_elapsed_ns_total: AtomicU64::new(0),
            benchmark_elapsed_ns_max: AtomicU64::new(0),
        }
    }

    /// Records one callback duration in nanoseconds.
    pub fn record_callback_execution(&self, elapsed_ns: u64) {
        increment_timing(
            &self.callback_count,
            &self.callback_execution_ns_total,
            &self.callback_execution_ns_max,
            elapsed_ns,
        );
    }

    /// Records one renderer duration in nanoseconds.
    pub fn record_renderer_execution(&self, elapsed_ns: u64) {
        increment_timing(
            &self.renderer_count,
            &self.renderer_execution_ns_total,
            &self.renderer_execution_ns_max,
            elapsed_ns,
        );
    }

    /// Records current bounded queue occupancy.
    pub fn record_queue_occupancy(&self, frames: u64) {
        self.queue_occupancy.store(frames, Ordering::Relaxed);
        self.queue_occupancy_max
            .fetch_max(frames, Ordering::Relaxed);
    }

    /// Increments the underrun counter.
    pub fn record_underrun(&self) {
        increment(&self.underrun_count, 1);
    }

    /// Increments the overrun counter.
    pub fn record_overrun(&self) {
        increment(&self.overrun_count, 1);
    }

    /// Increments the recovery counter.
    pub fn record_recovery(&self) {
        increment(&self.recovery_count, 1);
    }

    /// Adds dropped frames to the cumulative count.
    pub fn record_dropped_frames(&self, frames: u64) {
        increment(&self.dropped_frames, frames);
    }

    /// Adds deterministic simulation work and elapsed host time.
    pub fn record_simulation_progress(&self, frames: u64, elapsed_ns: u64) {
        increment(&self.simulation_frames, frames);
        increment(&self.simulation_elapsed_ns, elapsed_ns);
    }

    /// Records one benchmark sample duration in nanoseconds.
    pub fn record_benchmark_sample(&self, elapsed_ns: u64) {
        increment_timing(
            &self.benchmark_samples,
            &self.benchmark_elapsed_ns_total,
            &self.benchmark_elapsed_ns_max,
            elapsed_ns,
        );
    }

    /// Copies a coherent-enough operational snapshot on a control thread.
    #[must_use]
    pub fn snapshot(&self) -> MetricSnapshot {
        MetricSnapshot {
            callback_count: load(&self.callback_count),
            callback_execution_ns_total: load(&self.callback_execution_ns_total),
            callback_execution_ns_max: load(&self.callback_execution_ns_max),
            renderer_count: load(&self.renderer_count),
            renderer_execution_ns_total: load(&self.renderer_execution_ns_total),
            renderer_execution_ns_max: load(&self.renderer_execution_ns_max),
            queue_occupancy: load(&self.queue_occupancy),
            queue_occupancy_max: load(&self.queue_occupancy_max),
            underrun_count: load(&self.underrun_count),
            overrun_count: load(&self.overrun_count),
            recovery_count: load(&self.recovery_count),
            dropped_frames: load(&self.dropped_frames),
            simulation_frames: load(&self.simulation_frames),
            simulation_elapsed_ns: load(&self.simulation_elapsed_ns),
            benchmark_samples: load(&self.benchmark_samples),
            benchmark_elapsed_ns_total: load(&self.benchmark_elapsed_ns_total),
            benchmark_elapsed_ns_max: load(&self.benchmark_elapsed_ns_max),
        }
    }
}

fn load(counter: &AtomicU64) -> u64 {
    counter.load(Ordering::Relaxed)
}

fn increment(counter: &AtomicU64, amount: u64) {
    counter.fetch_add(amount, Ordering::Relaxed);
}

fn increment_timing(count: &AtomicU64, total: &AtomicU64, maximum: &AtomicU64, elapsed_ns: u64) {
    increment(count, 1);
    increment(total, elapsed_ns);
    maximum.fetch_max(elapsed_ns, Ordering::Relaxed);
}

/// Serializable control-thread copy of all performance metrics.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct MetricSnapshot {
    /// Number of callback observations.
    pub callback_count: u64,
    /// Sum of observed callback execution nanoseconds.
    pub callback_execution_ns_total: u64,
    /// Maximum observed callback execution nanoseconds.
    pub callback_execution_ns_max: u64,
    /// Number of renderer observations.
    pub renderer_count: u64,
    /// Sum of observed renderer execution nanoseconds.
    pub renderer_execution_ns_total: u64,
    /// Maximum observed renderer execution nanoseconds.
    pub renderer_execution_ns_max: u64,
    /// Most recently observed queue occupancy in frames.
    pub queue_occupancy: u64,
    /// Maximum observed queue occupancy in frames.
    pub queue_occupancy_max: u64,
    /// Cumulative underrun count.
    pub underrun_count: u64,
    /// Cumulative overrun count.
    pub overrun_count: u64,
    /// Cumulative successful or attempted recoveries.
    pub recovery_count: u64,
    /// Cumulative dropped frame count.
    pub dropped_frames: u64,
    /// Cumulative simulated frames.
    pub simulation_frames: u64,
    /// Cumulative host-observed simulation execution nanoseconds.
    pub simulation_elapsed_ns: u64,
    /// Number of benchmark samples.
    pub benchmark_samples: u64,
    /// Sum of benchmark sample nanoseconds.
    pub benchmark_elapsed_ns_total: u64,
    /// Maximum benchmark sample nanoseconds.
    pub benchmark_elapsed_ns_max: u64,
}

impl MetricSnapshot {
    /// Returns simulated frames per second when elapsed time is available.
    #[must_use]
    pub fn simulation_frames_per_second(&self) -> Option<f64> {
        (self.simulation_elapsed_ns != 0).then(|| {
            self.simulation_frames as f64 * 1_000_000_000.0 / self.simulation_elapsed_ns as f64
        })
    }

    /// Returns mean benchmark sample duration when samples are available.
    #[must_use]
    pub fn benchmark_mean_ns(&self) -> Option<f64> {
        (self.benchmark_samples != 0)
            .then(|| self.benchmark_elapsed_ns_total as f64 / self.benchmark_samples as f64)
    }
}
