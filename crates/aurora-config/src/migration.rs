use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    ConfigError, ErrorCategory, ErrorCode, ValidatedConfiguration, CURRENT_SCHEMA_VERSION,
    MAX_MIGRATION_DIAGNOSTICS, MAX_SERIALIZED_BYTES,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MigrationWarning {
    pub field_path: String,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MigrationResult {
    pub configuration: ValidatedConfiguration,
    pub changed_fields: BTreeSet<String>,
    pub warnings: Vec<MigrationWarning>,
}

pub fn migrate_v1_to_v2(bytes: &[u8]) -> Result<MigrationResult, ConfigError> {
    migrate_to_v2(bytes, 1)
}

pub fn migrate_v0_to_v2(bytes: &[u8]) -> Result<MigrationResult, ConfigError> {
    migrate_to_v2(bytes, 0)
}

fn migrate_to_v2(bytes: &[u8], source_version: u16) -> Result<MigrationResult, ConfigError> {
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
        .get("schema")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| {
            migration_error(
                ErrorCode::UnsupportedMigration,
                "schema",
                "schema metadata is missing",
            )
        })?;
    if schema
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        != Some(u64::from(source_version))
        || schema
            .get("minimum_reader_version")
            .and_then(serde_json::Value::as_u64)
            != Some(u64::from(source_version))
    {
        return Err(migration_error(
            ErrorCode::UnsupportedMigration,
            "schema",
            "migration source schema metadata does not match the requested source version",
        ));
    }

    let renderer = root
        .get_mut("renderer")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            migration_error(
                ErrorCode::UnsupportedMigration,
                "renderer",
                "legacy renderer object is missing",
            )
        })?;
    let mut changed_fields = BTreeSet::from([
        "schema.minimum_reader_version".to_owned(),
        "schema.schema_version".to_owned(),
        "renderer".to_owned(),
    ]);
    let mut warnings = Vec::new();
    if source_version == 0 {
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
        changed_fields.insert("renderer.spread_percent".to_owned());
        warnings.push(MigrationWarning {
  field_path: "renderer.spread_percent".to_owned(),
  detail: "deprecated percentage converted to normalized spread before renderer-component migration".to_owned(),
        });
    }

    let legacy = renderer.clone();
    let renderer_type = legacy
        .get("type")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            migration_error(
                ErrorCode::MigrationDataLoss,
                "renderer.type",
                "legacy renderer type is missing",
            )
        })?;
    let migrated_renderer = match renderer_type {
        "basic" if legacy.len() == 1 => serde_json::json!({
        "component_id": "org.aurora.renderer.basic",
        "contract_kind": "renderer",
        "contract_major": 1,
        "compatible_minor": {"minimum": 0, "maximum": 0},
        "configuration_schema": 1,
        "configuration": {}
              }),
        "point_source_vbap" if legacy.len() == 1 => serde_json::json!({
        "component_id": "org.aurora.renderer.vbap",
        "contract_kind": "renderer",
        "contract_major": 1,
        "compatible_minor": {"minimum": 0, "maximum": 0},
        "configuration_schema": 1,
        "configuration": {"mode": "point_source"}
              }),
        "horizontal_spread" if legacy.len() == 2 => {
            let spread = legacy
                .get("spread")
                .and_then(serde_json::Value::as_f64)
                .ok_or_else(|| {
                    migration_error(
                        ErrorCode::MigrationDataLoss,
                        "renderer.spread",
                        "legacy horizontal spread is missing or ambiguous",
                    )
                })?;
            if !spread.is_finite() || !(0.0..=1.0).contains(&spread) {
                return Err(migration_error(
                    ErrorCode::MigrationDataLoss,
                    "renderer.spread",
                    "legacy horizontal spread is outside 0..=1",
                ));
            }
            serde_json::json!({
                "component_id": "org.aurora.renderer.vbap",
                "contract_kind": "renderer",
                "contract_major": 1,
                "compatible_minor": {"minimum": 0, "maximum": 0},
                "configuration_schema": 1,
                "configuration": {"mode": "horizontal_spread", "spread": spread}
            })
        }
        _ => {
            return Err(migration_error(
                ErrorCode::MigrationDataLoss,
                "renderer",
                "legacy renderer cannot be migrated without ambiguity or data loss",
            ))
        }
    };
    root.insert("renderer".to_owned(), migrated_renderer);
    let schema = root
        .get_mut("schema")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            migration_error(
                ErrorCode::UnsupportedMigration,
                "schema",
                "schema metadata is missing",
            )
        })?;
    schema.insert(
        "schema_version".to_owned(),
        serde_json::Value::from(CURRENT_SCHEMA_VERSION),
    );
    schema.insert(
        "minimum_reader_version".to_owned(),
        serde_json::Value::from(CURRENT_SCHEMA_VERSION),
    );
    warnings.push(MigrationWarning {
        field_path: "renderer".to_owned(),
        detail: "legacy renderer enum migrated to a versioned renderer component reference"
            .to_owned(),
    });
    warnings.truncate(MAX_MIGRATION_DIAGNOSTICS);

    let migrated = serde_json::to_vec(&value).map_err(|_| {
        migration_error(
            ErrorCode::InvalidJson,
            "$",
            "migrated document could not be serialized",
        )
    })?;
    let configuration = ValidatedConfiguration::from_json(&migrated)?;
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
        Some("use the explicit migration matching the source schema version"),
    )
}
