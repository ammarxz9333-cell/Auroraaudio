#![forbid(unsafe_code)]
//! Hardware-independent diagnostics and telemetry primitives.
//!
//! Structured diagnostics are control-thread facilities. Audio callbacks may
//! use only [`RealtimeMetricCounters`], whose recording methods perform fixed
//! atomic operations without allocation, locks, formatting, or I/O.

mod event;
mod log;
mod metrics;
mod report;
mod snapshot;

pub use event::{DiagnosticEvent, DiagnosticValue, EventId, EventTimestamp, Severity, TruthSource};
pub use log::{
    DiagnosticLog, DiagnosticLogError, LogDisposition, SharedDiagnosticLog, DEFAULT_MAX_EVENT_BYTES,
};
pub use metrics::{MetricSnapshot, RealtimeMetricCounters};
pub use report::{DiagnosticReport, FailureCategory, ReportValidationError, Reproducibility};
pub use snapshot::{
    BuildInformation, DiagnosticSnapshot, MemorySummary, QueueStatus, RoutingEdge,
    SnapshotValidationError,
};
