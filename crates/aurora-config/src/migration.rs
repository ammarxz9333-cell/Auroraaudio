use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    ConfigError, ErrorCategory, ErrorCode, ValidatedConfiguration, CURRENT_SCHEMA_VERSION,
    MAX_MIGRATION_DIAGNOSTICS, MAX_SERIALIZED_BYTES,
};

/// Bounded migration warning.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationWarning {
    /// Stable field path affected by migration.
    pub field_path: String,
    /// Concise warning message.
    pub detail: String,
}

/// Deterministic migration output and audit information.
#[derive(Clone, Debug, PartialEq)]
pub struct MigrationResult {
    /// Validated destination configuration.
    pub configuration: ValidatedConfiguration,
    /// Deterministically sorted changed field paths.
    pub changed_fields: BTreeSet<String>,
    /// Bounded deprecation warnings.
    pub warnings: Vec<MigrationWarning>,
}

/// Migrates the reviewable fixture schema version 0 into production version 1.
///
/// Version 0 differs only in schema metadata and the deprecated
/// `renderer.spread_percent` field. No unknown field is dropped.
pub fn migrate_v0_to_v1(bytes: &[u8]) -> Result<MigrationResult, ConfigError> {
    if bytes.len() > MAX_SERIALIZED_BYTES {
        return Err(migration_error(
            ErrorCode::SerializedSizeExceeded,
            "$",
            "migration input exceeds the serialized byte limit",
        ));
    }
    let mut value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| {
        migration_error(
            ErrorCode::InvalidJson,
            "$",
            "migration input is not valid JSON",
        )
    })?;
    let root = value.as_object_mut().ok_or_else(|| {
        migration_error(
            ErrorCode::UnsupportedMigration,
            "$",
            "migration input must be an object",
        )
    })?;
    let schema = root
        .get_mut("schema")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            migration_error(
                ErrorCode::UnsupportedMigration,
                "schema",
                "version 0 schema metadata is missing",
            )
        })?;
    if schema
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        != Some(0)
    {
        return Err(migration_error(
            ErrorCode::UnsupportedMigration,
            "schema.schema_version",
            "only source version 0 can migrate to version 1",
        ));
    }
    schema.insert(
        "schema_version".to_owned(),
        serde_json::Value::from(CURRENT_SCHEMA_VERSION),
    );
    schema.insert(
        "minimum_reader_version".to_owned(),
        serde_json::Value::from(CURRENT_SCHEMA_VERSION),
    );

    let renderer = root
        .get_mut("renderer")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            migration_error(
                ErrorCode::UnsupportedMigration,
                "renderer",
                "version 0 renderer object is missing",
            )
        })?;
    let percent = renderer
        .remove("spread_percent")
        .and_then(|value| value.as_f64())
        .ok_or_else(|| {
            migration_error(
                ErrorCode::MigrationDataLoss,
                "renderer.spread_percent",
                "deprecated spread percentage is missing or ambiguous",
            )
        })?;
    if !percent.is_finite() || !(0.0..=100.0).contains(&percent) {
        return Err(migration_error(
            ErrorCode::MigrationDataLoss,
            "renderer.spread_percent",
            "deprecated spread percentage is outside 0..=100",
        ));
    }
    renderer.insert(
        "spread".to_owned(),
        serde_json::Value::from(percent / 100.0),
    );

    let migrated = serde_json::to_vec(&value).map_err(|_| {
        migration_error(
            ErrorCode::InvalidJson,
            "$",
            "migrated document could not be serialized",
        )
    })?;
    let configuration = ValidatedConfiguration::from_json(&migrated)?;
    let changed_fields = [
        "schema.minimum_reader_version",
        "schema.schema_version",
        "renderer.spread",
        "renderer.spread_percent",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let mut warnings = vec![MigrationWarning {
        field_path: "renderer.spread_percent".to_owned(),
        detail: "deprecated percentage converted to normalized spread".to_owned(),
    }];
    warnings.truncate(MAX_MIGRATION_DIAGNOSTICS);
    Ok(MigrationResult {
        configuration,
        changed_fields,
        warnings,
    })
}

fn migration_error(code: ErrorCode, path: &str, detail: &str) -> ConfigError {
    let category = if code == ErrorCode::SerializedSizeExceeded {
        ErrorCategory::Bounds
    } else if code == ErrorCode::InvalidJson {
        ErrorCategory::Schema
    } else {
        ErrorCategory::Migration
    };
    ConfigError::new(
        code,
        path,
        category,
        detail,
        Some("use a supported explicit migration source without dropping fields"),
    )
}
