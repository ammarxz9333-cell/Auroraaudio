use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{DiagnosticValue, TruthSource};

/// Reasons a diagnostic report fails schema validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReportValidationError {
    /// Report schema version is unsupported.
    UnsupportedSchemaVersion,
    /// Root component is empty.
    MissingRootComponent,
    /// Reproduction operation or scenario is empty.
    MissingReproducibility,
    /// Recommendation is empty.
    MissingRecommendation,
    /// A deterministic simulation report omitted its seed.
    MissingSimulationSeed,
}

/// Stable high-level diagnostic failure categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCategory {
    /// Configuration is malformed or unsupported.
    Configuration,
    /// Device discovery or capability selection failed.
    Device,
    /// Routing graph or channel mapping failed validation.
    Routing,
    /// Renderer selection or execution failed.
    Renderer,
    /// Queue or callback transport failed.
    RealtimeTransport,
    /// Deterministic simulation failed.
    Simulation,
    /// Benchmark execution failed.
    Benchmark,
    /// Recovery could not restore operation.
    Recovery,
    /// Failure does not fit another stable category.
    Internal,
}

/// Information required to reproduce a diagnostic failure.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Reproducibility {
    /// Exact command or operation description.
    pub operation: String,
    /// Fixture, scenario, or configuration identifier.
    pub scenario: String,
    /// Deterministic seed when the failure came from simulation.
    pub deterministic_seed: Option<u64>,
}

/// Structured support report for one failure.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticReport {
    /// Report schema version.
    pub schema_version: u16,
    /// Stable failure category.
    pub failure_category: FailureCategory,
    /// Aurora component where the root failure was identified.
    pub root_component: String,
    /// Reproduction information and optional simulation seed.
    pub reproducibility: Reproducibility,
    /// Operator or developer recommendation.
    pub recommendation: String,
    /// Exact source of truth for the report.
    pub truth_source: TruthSource,
    /// Additional deterministic context sorted by key.
    pub context: BTreeMap<String, DiagnosticValue>,
}

impl DiagnosticReport {
    /// Validates required report fields and simulation provenance.
    pub fn validate(&self) -> Result<(), ReportValidationError> {
        if self.schema_version != 1 {
            return Err(ReportValidationError::UnsupportedSchemaVersion);
        }
        if self.root_component.is_empty() {
            return Err(ReportValidationError::MissingRootComponent);
        }
        if self.reproducibility.operation.is_empty() || self.reproducibility.scenario.is_empty() {
            return Err(ReportValidationError::MissingReproducibility);
        }
        if self.recommendation.is_empty() {
            return Err(ReportValidationError::MissingRecommendation);
        }
        if self.truth_source == TruthSource::DeterministicSimulation
            && self.reproducibility.deterministic_seed.is_none()
        {
            return Err(ReportValidationError::MissingSimulationSeed);
        }
        Ok(())
    }

    /// Serializes this report as deterministic pretty JSON.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}
