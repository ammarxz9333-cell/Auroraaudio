use aurora_diagnostics::TruthSource;
use serde::{Deserialize, Serialize};

use crate::{
    AuroraConfiguration, ConfigError, ErrorCategory, ErrorCode, RedactionMode,
    ValidatedConfiguration,
};

/// Redacted device intent suitable for diagnostic snapshots.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RedactedDeviceIntent {
    /// Stable ID or redaction marker.
    pub stable_id: Option<String>,
    /// Friendly names are always omitted.
    pub friendly_name: Option<String>,
}

/// Control-thread-only redacted configuration representation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RedactedConfiguration {
    /// Accurate evidence source for this representation.
    pub truth_source: TruthSource,
    /// Redacted normalized configuration.
    pub configuration: AuroraConfiguration,
}

impl RedactedConfiguration {
    /// Creates a redacted snapshot without modifying the validated source.
    pub fn new(
        source: &ValidatedConfiguration,
        mode: RedactionMode,
        truth_source: TruthSource,
    ) -> Self {
        let mut configuration = source.config().clone();
        configuration.schema.generated_by = None;
        for device in [
            configuration.input_device.as_mut(),
            configuration.output_device.as_mut(),
        ]
        .into_iter()
        .flatten()
        {
            device.friendly_name = None;
            if mode == RedactionMode::Strict {
                device.stable_id = device.stable_id.as_ref().map(|_| "[redacted]".to_owned());
            }
        }
        if let Some(simulation) = configuration.simulation.as_mut() {
            simulation.replay_id = "[redacted]".to_owned();
        }
        Self {
            truth_source,
            configuration,
        }
    }

    /// Deterministically serializes the redacted representation.
    pub fn canonical_json(&self) -> Result<String, ConfigError> {
        serde_json::to_string(self).map_err(|_| {
            ConfigError::new(
                ErrorCode::InvalidJson,
                "$",
                ErrorCategory::Schema,
                "redacted configuration could not be serialized",
                Some("report the schema serialization defect"),
            )
        })
    }
}
