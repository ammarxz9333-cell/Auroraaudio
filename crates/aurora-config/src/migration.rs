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

pub fn migrate_v2_to_v3(bytes: &[u8]) -> Result<MigrationResult, ConfigError> {
    migrate_to_v3(bytes, 2)
}

pub fn migrate_v1_to_v3(bytes: &[u8]) -> Result<MigrationResult, ConfigError> {
    migrate_to_v3(bytes, 1)
}

pub fn migrate_v0_to_v3(bytes: &[u8]) -> Result<MigrationResult, ConfigError> {
    migrate_to_v3(bytes, 0)
}

fn migrate_to_v3(bytes: &[u8], source_version: u16) -> Result<MigrationResult, ConfigError> {
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
    validate_source_schema(root, source_version)?;

    let mut changed_fields = BTreeSet::from([
        "schema.minimum_reader_version".to_owned(),
        "schema.schema_version".to_owned(),
    ]);
    let mut warnings = Vec::new();

    if source_version < 2 {
        migrate_renderer(root, source_version, &mut changed_fields, &mut warnings)?;
    }
    migrate_legacy_backend(
        root,
        "input_device",
        "audio_input_backend",
        &mut changed_fields,
        &mut warnings,
    )?;
    migrate_legacy_backend(
        root,
        "output_device",
        "audio_output_backend",
        &mut changed_fields,
        &mut warnings,
    )?;

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

fn validate_source_schema(
    root: &serde_json::Map<String, serde_json::Value>,
    source_version: u16,
) -> Result<(), ConfigError> {
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
    Ok(())
}

fn migrate_renderer(
    root: &mut serde_json::Map<String, serde_json::Value>,
    source_version: u16,
    changed_fields: &mut BTreeSet<String>,
    warnings: &mut Vec<MigrationWarning>,
) -> Result<(), ConfigError> {
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
    let migrated = match renderer_type {
        "basic" if legacy.len() == 1 => component_reference(
            "org.aurora.renderer.basic",
            "renderer",
            serde_json::json!({}),
        ),
        "point_source_vbap" if legacy.len() == 1 => component_reference(
            "org.aurora.renderer.vbap",
            "renderer",
            serde_json::json!({"mode":"point_source"}),
        ),
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
            component_reference(
                "org.aurora.renderer.vbap",
                "renderer",
                serde_json::json!({"mode":"horizontal_spread","spread":spread}),
            )
        }
        _ => {
            return Err(migration_error(
                ErrorCode::MigrationDataLoss,
                "renderer",
                "legacy renderer cannot be migrated without ambiguity or data loss",
            ))
        }
    };
    root.insert("renderer".to_owned(), migrated);
    changed_fields.insert("renderer".to_owned());
    warnings.push(MigrationWarning {
        field_path: "renderer".to_owned(),
        detail: "legacy renderer enum migrated to a versioned renderer component reference"
            .to_owned(),
    });
    Ok(())
}

fn migrate_legacy_backend(
    root: &mut serde_json::Map<String, serde_json::Value>,
    field: &str,
    contract_kind: &str,
    changed_fields: &mut BTreeSet<String>,
    warnings: &mut Vec<MigrationWarning>,
) -> Result<(), ConfigError> {
    let Some(device) = root.get_mut(field) else {
        return Ok(());
    };
    if device.is_null() {
        return Ok(());
    }
    let device = device.as_object_mut().ok_or_else(|| {
        migration_error(
            ErrorCode::MigrationDataLoss,
            field,
            "legacy device selector is not an object",
        )
    })?;
    let expected_direction = if field == "input_device" {
        "input"
    } else {
        "output"
    };
    if device.get("direction").and_then(serde_json::Value::as_str) != Some(expected_direction) {
        return Err(migration_error(
            ErrorCode::MigrationDataLoss,
            field,
            "legacy device direction disagrees with its root field",
        ));
    }
    let legacy = device.remove("backend").ok_or_else(|| {
        migration_error(
            ErrorCode::MigrationDataLoss,
            field,
            "legacy backend selection is missing",
        )
    })?;
    if legacy.is_object() {
        device.insert("backend".to_owned(), legacy);
        return Ok(());
    }
    let legacy = legacy.as_str().ok_or_else(|| {
        migration_error(
            ErrorCode::MigrationDataLoss,
            field,
            "legacy backend selection is not a string",
        )
    })?;
    let component_id = match legacy {
        "virtual" => "org.aurora.backend.virtual",
        "cpal" => "org.aurora.backend.cpal",
        "offline" => "org.aurora.backend.offline",
        _ => {
            return Err(migration_error(
                ErrorCode::MigrationDataLoss,
                field,
                "legacy backend value is unknown and cannot be migrated safely",
            ))
        }
    };
    device.insert(
        "backend".to_owned(),
        component_reference(component_id, contract_kind, serde_json::json!({})),
    );
    let path = format!("{field}.backend");
    changed_fields.insert(path.clone());
    warnings.push(MigrationWarning {
        field_path: path,
        detail: "legacy backend enum migrated to a versioned audio-backend component reference"
            .to_owned(),
    });
    Ok(())
}

fn component_reference(
    component_id: &str,
    contract_kind: &str,
    configuration: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "component_id": component_id,
        "contract_kind": contract_kind,
        "contract_major": 1,
        "compatible_minor": {"minimum": 0, "maximum": 0},
        "configuration_schema": 1,
        "configuration": configuration,
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
