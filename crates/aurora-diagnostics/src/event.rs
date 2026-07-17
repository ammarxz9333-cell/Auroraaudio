use std::collections::BTreeMap;
use std::fmt::Write;

use serde::{Deserialize, Serialize};

/// Schema version emitted by this diagnostics milestone.
pub const DIAGNOSTIC_SCHEMA_VERSION: u16 = 1;

/// Structured-field key required when a record claims physical measurement.
pub const PHYSICAL_SIGNAL_PATH_FIELD: &str = "physical_signal_path";

/// Reasons truth-source evidence is inconsistent with its classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TruthSourceValidationError {
    /// Physical measurement omitted a non-empty documented signal path.
    MissingPhysicalSignalPath,
    /// A nonphysical truth source included physical signal-path evidence.
    UnexpectedPhysicalSignalPath,
}

/// Reasons an event fails schema validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventValidationError {
    /// Event schema version is unsupported.
    UnsupportedSchemaVersion,
    /// Event component is empty or whitespace only.
    MissingComponent,
    /// A payload field name is empty or whitespace only.
    EmptyPayloadKey,
    /// Truth-source evidence is missing or inconsistent.
    InvalidTruthSourceEvidence(TruthSourceValidationError),
}

/// Timestamp provenance for one event.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum EventTimestamp {
    /// Deterministic sequence value supplied by the producer.
    Logical(u64),
    /// Monotonic host time in nanoseconds, never wall-clock time.
    MonotonicNanoseconds(u64),
}

/// Diagnostic severity ordered from least to most urgent.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Fine-grained development information.
    Trace,
    /// Normal operational detail.
    Debug,
    /// Significant expected lifecycle information.
    Info,
    /// Degraded operation that may recover.
    Warning,
    /// Operation failed and requires attention.
    Error,
    /// Operation cannot continue safely.
    Critical,
}

/// Stable event identifiers owned by Aurora.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventId {
    /// Process or subsystem startup.
    Startup,
    /// Process or subsystem shutdown.
    Shutdown,
    /// Device enumeration or selection observation.
    DeviceDiscovery,
    /// Device or software capability report.
    CapabilityReport,
    /// Routing decision made from an explicit configuration.
    RoutingDecision,
    /// Renderer implementation selection.
    RendererSelection,
    /// Lifecycle or engine state transition.
    StateTransition,
    /// Audio path underrun observation.
    Underrun,
    /// Audio path overrun observation.
    Overrun,
    /// Recovery attempt or result.
    Recovery,
    /// Configuration validation result.
    ConfigurationValidation,
    /// Deterministic simulation execution.
    SimulationExecution,
    /// Benchmark execution or summary.
    BenchmarkExecution,
}

/// Exact provenance category required by the Aurora master reference.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TruthSource {
    /// Deterministic in-process verification without a virtual device.
    UnitTest,
    /// Deterministic modeled execution with explicit simulated truth.
    DeterministicSimulation,
    /// Execution through Aurora's software audio-device backend.
    VirtualAudioBackend,
    /// Metadata or behavior observed through a host API.
    HostApiObservation,
    /// Evidence captured from a documented physical signal path.
    PhysicalMeasurement,
}

/// Deterministically serializable payload value.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum DiagnosticValue {
    /// Boolean value.
    Boolean(bool),
    /// Signed integer value.
    Signed(i64),
    /// Unsigned integer value.
    Unsigned(u64),
    /// Text value.
    Text(String),
}

/// One structured diagnostic event.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticEvent {
    /// Diagnostics schema version.
    pub schema_version: u16,
    /// Logical or monotonic timestamp supplied by the producer.
    pub timestamp: EventTimestamp,
    /// Aurora component that produced the event.
    pub component: String,
    /// Event severity.
    pub severity: Severity,
    /// Stable event identifier.
    pub event_id: EventId,
    /// Structured payload sorted by key.
    pub payload: BTreeMap<String, DiagnosticValue>,
    /// Exact source of truth for the event.
    pub truth_source: TruthSource,
}

impl DiagnosticEvent {
    /// Creates an event with an empty ordered payload.
    pub fn new(
        timestamp: EventTimestamp,
        component: impl Into<String>,
        severity: Severity,
        event_id: EventId,
        truth_source: TruthSource,
    ) -> Self {
        Self {
            schema_version: DIAGNOSTIC_SCHEMA_VERSION,
            timestamp,
            component: component.into(),
            severity,
            event_id,
            payload: BTreeMap::new(),
            truth_source,
        }
    }

    /// Adds or replaces one payload field.
    #[must_use]
    pub fn with_field(mut self, key: impl Into<String>, value: DiagnosticValue) -> Self {
        self.payload.insert(key.into(), value);
        self
    }

    /// Validates schema version, required fields, and truth-source evidence.
    pub fn validate(&self) -> Result<(), EventValidationError> {
        if self.schema_version != DIAGNOSTIC_SCHEMA_VERSION {
            return Err(EventValidationError::UnsupportedSchemaVersion);
        }
        if self.component.trim().is_empty() {
            return Err(EventValidationError::MissingComponent);
        }
        if self.payload.keys().any(|key| key.trim().is_empty()) {
            return Err(EventValidationError::EmptyPayloadKey);
        }
        validate_truth_source_evidence(self.truth_source, &self.payload)
            .map_err(EventValidationError::InvalidTruthSourceEvidence)
    }

    /// Serializes this event as deterministic compact JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Formats this event as one deterministic human-readable line.
    #[must_use]
    pub fn to_human_line(&self) -> String {
        let mut line = format!(
            "timestamp={:?} severity={:?} component={} event={:?} truth_source={:?}",
            self.timestamp, self.severity, self.component, self.event_id, self.truth_source
        );
        for (key, value) in &self.payload {
            let _ = write!(line, " {key}={value:?}");
        }
        line
    }

    /// Estimates retained bytes using string lengths and fixed field overhead.
    #[must_use]
    pub fn estimated_size_bytes(&self) -> usize {
        const EVENT_OVERHEAD: usize = 128;
        const FIELD_OVERHEAD: usize = 64;
        self.payload.iter().fold(
            EVENT_OVERHEAD.saturating_add(self.component.len()),
            |total, (key, value)| {
                let value_bytes = match value {
                    DiagnosticValue::Text(text) => text.len(),
                    DiagnosticValue::Boolean(_)
                    | DiagnosticValue::Signed(_)
                    | DiagnosticValue::Unsigned(_) => std::mem::size_of::<DiagnosticValue>(),
                };
                total
                    .saturating_add(FIELD_OVERHEAD)
                    .saturating_add(key.len())
                    .saturating_add(value_bytes)
            },
        )
    }
}

pub(crate) fn validate_truth_source_evidence(
    truth_source: TruthSource,
    fields: &BTreeMap<String, DiagnosticValue>,
) -> Result<(), TruthSourceValidationError> {
    let physical_path = fields.get(PHYSICAL_SIGNAL_PATH_FIELD);
    match (truth_source, physical_path) {
        (TruthSource::PhysicalMeasurement, Some(DiagnosticValue::Text(path)))
            if !path.trim().is_empty() =>
        {
            Ok(())
        }
        (TruthSource::PhysicalMeasurement, _) => {
            Err(TruthSourceValidationError::MissingPhysicalSignalPath)
        }
        (_, None) => Ok(()),
        (_, Some(_)) => Err(TruthSourceValidationError::UnexpectedPhysicalSignalPath),
    }
}
