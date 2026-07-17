use std::fmt;

use serde::{Deserialize, Serialize};

use crate::MAX_STRING_BYTES;

/// Stable machine-readable configuration error code.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Input is not valid JSON for the requested schema.
    InvalidJson,
    /// Schema metadata is unsupported or inconsistent.
    UnsupportedSchemaVersion,
    /// A required string is missing or exceeds its bound.
    InvalidString,
    /// A numeric value is unsupported, non-finite, or outside its range.
    InvalidNumericValue,
    /// A bounded collection exceeds its limit.
    CollectionLimitExceeded,
    /// A stable identifier occurs more than once.
    DuplicateIdentifier,
    /// Routing assignments are duplicate, missing, or unknown.
    InvalidRouting,
    /// Device selection permits ambiguous or silent fallback behavior.
    AmbiguousDeviceSelection,
    /// Renderer kind is unsupported or incompatible with the layout.
    UnsupportedRenderer,
    /// Reserved metadata was requested as active behavior.
    UnsupportedReservedField,
    /// Buffer bounds are inconsistent.
    InvalidBufferBounds,
    /// Preset composition contains a conflict.
    PresetConflict,
    /// Preset composition contains a cycle.
    PresetCycle,
    /// Preset composition exceeds the maximum depth.
    PresetDepthExceeded,
    /// A preset reference cannot be resolved.
    PresetNotFound,
    /// Migration source or destination is unsupported.
    UnsupportedMigration,
    /// Migration would be ambiguous or lose data silently.
    MigrationDataLoss,
    /// Serialized input exceeds the published byte limit.
    SerializedSizeExceeded,
}

/// Broad stable error category suitable for support tooling.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    /// Parsing or schema selection failed.
    Schema,
    /// Field validation failed.
    Validation,
    /// Device-selection intent is unsafe or ambiguous.
    DeviceIntent,
    /// Routing intent is inconsistent.
    Routing,
    /// Renderer intent is unsupported.
    Renderer,
    /// Preset materialization failed.
    Preset,
    /// Schema migration failed.
    Migration,
    /// A configured resource bound was exceeded.
    Bounds,
}

/// Structured bounded configuration failure.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConfigError {
    /// Stable machine-readable error code.
    pub code: ErrorCode,
    /// Dot-separated field path.
    pub field_path: String,
    /// Stable broad category.
    pub category: ErrorCategory,
    /// Concise bounded human-readable detail.
    pub detail: String,
    /// Optional bounded remediation guidance.
    pub remediation: Option<String>,
}

impl ConfigError {
    pub(crate) fn new(
        code: ErrorCode,
        field_path: impl Into<String>,
        category: ErrorCategory,
        detail: impl Into<String>,
        remediation: Option<&str>,
    ) -> Self {
        Self {
            code,
            field_path: bounded(field_path.into()),
            category,
            detail: bounded(detail.into()),
            remediation: remediation.map(|value| bounded(value.to_owned())),
        }
    }
}

fn bounded(mut value: String) -> String {
    if value.len() <= MAX_STRING_BYTES {
        return value;
    }
    let mut boundary = MAX_STRING_BYTES;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
    value
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}: {:?} at {}",
            self.detail, self.code, self.field_path
        )
    }
}

impl std::error::Error for ConfigError {}
