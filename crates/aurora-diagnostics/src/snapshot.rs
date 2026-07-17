use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{DiagnosticValue, MetricSnapshot, TruthSource};

/// Reasons a snapshot fails schema validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotValidationError {
    /// Snapshot schema version is unsupported.
    UnsupportedSchemaVersion,
    /// Selected renderer is empty.
    MissingRenderer,
    /// Queue occupancy exceeds the declared bounded capacity.
    QueueExceedsCapacity,
    /// Retained diagnostic bytes exceed the declared capacity.
    MemoryExceedsCapacity,
    /// Build platform, version, or commit identity is empty.
    MissingBuildIdentity,
}

/// Build and platform identity included in a diagnostic snapshot.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BuildInformation {
    /// Operating system or target platform label.
    pub platform: String,
    /// Aurora package or release version.
    pub version: String,
    /// Exact Git commit, or an explicit `unknown` value when unavailable.
    pub git_commit: String,
}

/// One deterministic routing edge.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct RoutingEdge {
    /// Source endpoint or channel.
    pub source: String,
    /// Destination endpoint or channel.
    pub destination: String,
}

/// Bounded queue state captured on a control thread.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct QueueStatus {
    /// Current queue occupancy in frames.
    pub occupancy_frames: u64,
    /// Configured queue capacity in frames.
    pub capacity_frames: u64,
}

/// Declared memory capacities, not process resident memory measurements.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MemorySummary {
    /// Bytes reserved in fixed callback-owned storage.
    pub realtime_reserved_bytes: u64,
    /// Bytes currently retained by bounded diagnostic storage.
    pub diagnostics_retained_bytes: u64,
    /// Maximum configured bytes for bounded diagnostic storage.
    pub diagnostics_capacity_bytes: u64,
}

/// Deterministic support snapshot assembled outside real-time callbacks.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticSnapshot {
    /// Snapshot schema version.
    pub schema_version: u16,
    /// Active configuration sorted by key.
    pub active_configuration: BTreeMap<String, DiagnosticValue>,
    /// Explicit selected renderer identifier.
    pub selected_renderer: String,
    /// Routing edges sorted lexicographically.
    pub routing_graph: BTreeSet<RoutingEdge>,
    /// Current bounded queue state.
    pub queue_status: QueueStatus,
    /// Declared memory-capacity summary.
    pub memory: MemorySummary,
    /// Enabled Cargo or runtime features sorted by name.
    pub enabled_features: BTreeSet<String>,
    /// Platform, version, and source revision.
    pub build: BuildInformation,
    /// Atomic performance metric snapshot.
    pub metrics: MetricSnapshot,
    /// Exact source of truth for this snapshot.
    pub truth_source: TruthSource,
}

impl DiagnosticSnapshot {
    /// Validates required fields and bounded-capacity invariants.
    pub fn validate(&self) -> Result<(), SnapshotValidationError> {
        if self.schema_version != 1 {
            return Err(SnapshotValidationError::UnsupportedSchemaVersion);
        }
        if self.selected_renderer.is_empty() {
            return Err(SnapshotValidationError::MissingRenderer);
        }
        if self.queue_status.occupancy_frames > self.queue_status.capacity_frames {
            return Err(SnapshotValidationError::QueueExceedsCapacity);
        }
        if self.memory.diagnostics_retained_bytes > self.memory.diagnostics_capacity_bytes {
            return Err(SnapshotValidationError::MemoryExceedsCapacity);
        }
        if self.build.platform.is_empty()
            || self.build.version.is_empty()
            || self.build.git_commit.is_empty()
        {
            return Err(SnapshotValidationError::MissingBuildIdentity);
        }
        Ok(())
    }

    /// Serializes this snapshot as deterministic pretty JSON.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}
