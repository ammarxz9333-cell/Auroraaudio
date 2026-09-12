from pathlib import Path
import json
import re


def read(path):
    return Path(path).read_text()


def write(path, text):
    Path(path).write_text(text)


def replace_once(text, old, new, label):
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one occurrence, found {count}")
    return text.replace(old, new, 1)


# --- config schema v3 -------------------------------------------------------
p = Path('crates/aurora-config/src/limits.rs')
t = p.read_text()
t = replace_once(t, 'pub const CURRENT_SCHEMA_VERSION: u16 = 2;', 'pub const CURRENT_SCHEMA_VERSION: u16 = 3;', 'current schema')
t = replace_once(t, 'pub const MINIMUM_READER_VERSION: u16 = 2;', 'pub const MINIMUM_READER_VERSION: u16 = 3;', 'minimum reader')
p.write_text(t)

p = Path('crates/aurora-config/src/model.rs')
t = p.read_text()
backend_enum = '''/// Audio backend selection intent.\n#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]\n#[serde(rename_all = "snake_case")]\npub enum BackendIntent {\n    /// Use the deterministic Aurora virtual backend.\n    Virtual,\n    /// Use CPAL when a later control-plane integration resolves the intent.\n    Cpal,\n    /// Offline operation without a device backend.\n    Offline,\n}\n\n'''
t = replace_once(t, backend_enum, '', 'BackendIntent enum')
t = replace_once(
    t,
    '''/// Device-selection intent that does not claim device existence.\n#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]\n#[serde(deny_unknown_fields)]\npub struct DeviceSelectionIntent {''',
    '''/// Device-selection intent that does not claim device existence.\n#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]\n#[serde(deny_unknown_fields)]\npub struct DeviceSelectionIntent {''',
    'DeviceSelectionIntent derive',
)
t = replace_once(
    t,
    '    /// Intended backend.\n    pub backend: BackendIntent,',
    '    /// Versioned backend component reference resolved before activation.\n    pub backend: ComponentReference,',
    'DeviceSelectionIntent backend field',
)
p.write_text(t)

p = Path('crates/aurora-config/src/validation.rs')
t = p.read_text()
# Add backend strings to bounded string validation.
anchor = '''    if let Some(value) = config.renderer.implementation_version_pin.as_deref() {\n        fields.push(("renderer.implementation_version_pin", value));\n    }\n'''
insert = anchor + '''    if let Some(device) = config.input_device.as_ref() {\n        fields.push(("input_device.backend.component_id", device.backend.component_id.as_str()));\n        if let Some(value) = device.backend.implementation_version_pin.as_deref() {\n            fields.push(("input_device.backend.implementation_version_pin", value));\n        }\n    }\n    if let Some(device) = config.output_device.as_ref() {\n        fields.push(("output_device.backend.component_id", device.backend.component_id.as_str()));\n        if let Some(value) = device.backend.implementation_version_pin.as_deref() {\n            fields.push(("output_device.backend.implementation_version_pin", value));\n        }\n    }\n'''
t = replace_once(t, anchor, insert, 'backend string bounds')
# Validate direction-specific backend references while validating device intent.
needle = '''        for value in [device.stable_id.as_deref(), device.friendly_name.as_deref()]\n            .into_iter()\n            .flatten()\n        {\n            if value.trim().is_empty() || value.len() > MAX_STRING_BYTES {\n                return Err(error(\n                    ErrorCode::InvalidString,\n                    path,\n                    ErrorCategory::DeviceIntent,\n                    "device selector exceeds the UTF-8 byte limit",\n                ));\n            }\n        }\n'''
replacement = needle + '''        let expected_contract = match path {\n            "input_device" => ComponentContractKind::AudioInputBackend,\n            "output_device" => ComponentContractKind::AudioOutputBackend,\n            _ => unreachable!("device validation uses fixed root paths"),\n        };\n        validate_component_reference(\n            &device.backend,\n            expected_contract,\n            if path == "input_device" {\n                "input_device.backend"\n            } else {\n                "output_device.backend"\n            },\n        )?;\n'''
t = replace_once(t, needle, replacement, 'device backend validation')
old_renderer = '''fn validate_renderer(config: &AuroraConfiguration) -> Result<(), ConfigError> {\n    let reference = &config.renderer;\n    if reference.contract_kind != ComponentContractKind::Renderer\n        || reference.contract_major == 0\n        || reference.compatible_minor.minimum > reference.compatible_minor.maximum\n        || reference.configuration_schema == 0\n        || !reference.configuration.is_object()\n    {\n        return Err(error(\n            ErrorCode::InvalidComponentReference,\n            "renderer",\n            ErrorCategory::Component,\n            "renderer component reference has an invalid contract, version range, or payload shape",\n        ));\n    }\n    let mut entries = 0usize;\n    validate_component_payload(\n        &reference.configuration,\n        "renderer.configuration",\n        0,\n        &mut entries,\n    )\n}\n'''
new_renderer = '''fn validate_renderer(config: &AuroraConfiguration) -> Result<(), ConfigError> {\n    validate_component_reference(\n        &config.renderer,\n        ComponentContractKind::Renderer,\n        "renderer",\n    )\n}\n\nfn validate_component_reference(\n    reference: &crate::ComponentReference,\n    expected_contract: ComponentContractKind,\n    path: &str,\n) -> Result<(), ConfigError> {\n    if reference.contract_kind != expected_contract\n        || reference.contract_major == 0\n        || reference.compatible_minor.minimum > reference.compatible_minor.maximum\n        || reference.configuration_schema == 0\n        || !reference.configuration.is_object()\n    {\n        return Err(error(\n            ErrorCode::InvalidComponentReference,\n            path,\n            ErrorCategory::Component,\n            "component reference has an invalid contract, version range, or payload shape",\n        ));\n    }\n    let mut entries = 0usize;\n    let configuration_path = match expected_contract {\n        ComponentContractKind::Renderer => "renderer.configuration",\n        ComponentContractKind::AudioInputBackend => "input_device.backend.configuration",\n        ComponentContractKind::AudioOutputBackend => "output_device.backend.configuration",\n        _ => "component.configuration",\n    };\n    validate_component_payload(\n        &reference.configuration,\n        configuration_path,\n        0,\n        &mut entries,\n    )\n}\n'''
t = replace_once(t, old_renderer, new_renderer, 'component reference validator')
p.write_text(t)

# --- explicit migration to v3 ----------------------------------------------
migration = r'''use std::collections::BTreeSet;

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
        .ok_or_else(|| migration_error(ErrorCode::UnsupportedMigration, "schema", "schema metadata is missing"))?;
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
        .ok_or_else(|| migration_error(ErrorCode::UnsupportedMigration, "schema", "schema metadata is missing"))?;
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
        .ok_or_else(|| migration_error(ErrorCode::UnsupportedMigration, "renderer", "legacy renderer object is missing"))?;
    if source_version == 0 {
        let percent = renderer
            .remove("spread_percent")
            .and_then(|value| value.as_f64())
            .ok_or_else(|| migration_error(ErrorCode::MigrationDataLoss, "renderer.spread_percent", "deprecated spread percentage is missing or ambiguous"))?;
        if !percent.is_finite() || !(0.0..=100.0).contains(&percent) {
            return Err(migration_error(ErrorCode::MigrationDataLoss, "renderer.spread_percent", "deprecated spread percentage is outside 0..=100"));
        }
        renderer.insert("spread".to_owned(), serde_json::Value::from(percent / 100.0));
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
        .ok_or_else(|| migration_error(ErrorCode::MigrationDataLoss, "renderer.type", "legacy renderer type is missing"))?;
    let migrated = match renderer_type {
        "basic" if legacy.len() == 1 => component_reference("org.aurora.renderer.basic", "renderer", serde_json::json!({})),
        "point_source_vbap" if legacy.len() == 1 => component_reference("org.aurora.renderer.vbap", "renderer", serde_json::json!({"mode":"point_source"})),
        "horizontal_spread" if legacy.len() == 2 => {
            let spread = legacy
                .get("spread")
                .and_then(serde_json::Value::as_f64)
                .ok_or_else(|| migration_error(ErrorCode::MigrationDataLoss, "renderer.spread", "legacy horizontal spread is missing or ambiguous"))?;
            if !spread.is_finite() || !(0.0..=1.0).contains(&spread) {
                return Err(migration_error(ErrorCode::MigrationDataLoss, "renderer.spread", "legacy horizontal spread is outside 0..=1"));
            }
            component_reference(
                "org.aurora.renderer.vbap",
                "renderer",
                serde_json::json!({"mode":"horizontal_spread","spread":spread}),
            )
        }
        _ => return Err(migration_error(ErrorCode::MigrationDataLoss, "renderer", "legacy renderer cannot be migrated without ambiguity or data loss")),
    };
    root.insert("renderer".to_owned(), migrated);
    changed_fields.insert("renderer".to_owned());
    warnings.push(MigrationWarning {
        field_path: "renderer".to_owned(),
        detail: "legacy renderer enum migrated to a versioned renderer component reference".to_owned(),
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
    let Some(device) = root.get_mut(field) else { return Ok(()); };
    if device.is_null() {
        return Ok(());
    }
    let device = device.as_object_mut().ok_or_else(|| {
        migration_error(ErrorCode::MigrationDataLoss, field, "legacy device selector is not an object")
    })?;
    let expected_direction = if field == "input_device" { "input" } else { "output" };
    if device.get("direction").and_then(serde_json::Value::as_str) != Some(expected_direction) {
        return Err(migration_error(
            ErrorCode::MigrationDataLoss,
            field,
            "legacy device direction disagrees with its root field",
        ));
    }
    let legacy = device.remove("backend").ok_or_else(|| {
        migration_error(ErrorCode::MigrationDataLoss, field, "legacy backend selection is missing")
    })?;
    if legacy.is_object() {
        device.insert("backend".to_owned(), legacy);
        return Ok(());
    }
    let legacy = legacy.as_str().ok_or_else(|| {
        migration_error(ErrorCode::MigrationDataLoss, field, "legacy backend selection is not a string")
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
        detail: "legacy backend enum migrated to a versioned audio-backend component reference".to_owned(),
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
'''
write('crates/aurora-config/src/migration.rs', migration)

p = Path('crates/aurora-config/src/lib.rs')
t = p.read_text()
t = replace_once(
    t,
    'pub use migration::{migrate_v0_to_v2, migrate_v1_to_v2, MigrationResult, MigrationWarning};',
    'pub use migration::{migrate_v0_to_v3, migrate_v1_to_v3, migrate_v2_to_v3, MigrationResult, MigrationWarning};',
    'migration exports',
)
p.write_text(t)

# Helpers for tests and callers that construct backend refs.
p = Path('crates/aurora-config/tests/common/mod.rs')
t = p.read_text()
helper_anchor = '''fn renderer_reference(component_id: &str, configuration: serde_json::Value) -> ComponentReference {\n    ComponentReference {\n        component_id: component_id.to_owned(),\n        contract_kind: ComponentContractKind::Renderer,\n        contract_major: 1,\n        compatible_minor: CompatibleMinorRange {\n            minimum: 0,\n            maximum: 0,\n        },\n        implementation_version_pin: None,\n        configuration_schema: 1,\n        configuration,\n    }\n}\n'''
helper_insert = helper_anchor + '''\n#[allow(dead_code)]\npub fn backend_reference(component_id: &str, direction: DeviceDirection) -> ComponentReference {\n    ComponentReference {\n        component_id: component_id.to_owned(),\n        contract_kind: match direction {\n            DeviceDirection::Input => ComponentContractKind::AudioInputBackend,\n            DeviceDirection::Output => ComponentContractKind::AudioOutputBackend,\n        },\n        contract_major: 1,\n        compatible_minor: CompatibleMinorRange { minimum: 0, maximum: 0 },\n        implementation_version_pin: None,\n        configuration_schema: 1,\n        configuration: serde_json::json!({}),\n    }\n}\n'''
t = replace_once(t, helper_anchor, helper_insert, 'backend test helper')
p.write_text(t)

# Generate schema-v3 fixtures from v2 fixtures while preserving v2 migration inputs.
def migrate_fixture_value(value):
    value = json.loads(json.dumps(value))
    schema = value.get('schema')
    if isinstance(schema, dict) and schema.get('schema_version') == 2:
        schema['schema_version'] = 3
        schema['minimum_reader_version'] = 3
    for field, kind in [('input_device', 'audio_input_backend'), ('output_device', 'audio_output_backend')]:
        device = value.get(field)
        if not isinstance(device, dict):
            continue
        backend = device.get('backend')
        if isinstance(backend, str):
            ids = {
                'virtual': 'org.aurora.backend.virtual',
                'cpal': 'org.aurora.backend.cpal',
                'offline': 'org.aurora.backend.offline',
            }
            if backend not in ids:
                raise SystemExit(f'{field}: unsupported fixture backend {backend}')
            device['backend'] = {
                'component_id': ids[backend],
                'contract_kind': kind,
                'contract_major': 1,
                'compatible_minor': {'minimum': 0, 'maximum': 0},
                'configuration_schema': 1,
                'configuration': {},
            }
    return value

fixture_root = Path('fixtures/config')
for src in sorted(fixture_root.glob('*-v2.json')):
    dst = src.with_name(src.name.replace('-v2.json', '-v3.json'))
    value = migrate_fixture_value(json.loads(src.read_text()))
    dst.write_text(json.dumps(value, separators=(',', ':')) + '\n')

# Switch current-schema fixture consumers to v3, but keep explicit migration v2 inputs intact.
current_names = [
    'minimal', 'stereo-basic', 'surround-5-1', 'surround-7-1',
    'irregular-horizontal', 'phase-3a-point-source', 'phase-3b-spread',
    'invalid-duplicate-speaker', 'invalid-ambiguous-device',
]
for path in list(Path('crates').rglob('*.rs')):
    text = path.read_text()
    original = text
    for name in current_names:
        text = text.replace(f'fixtures/config/{name}-v2.json', f'fixtures/config/{name}-v3.json')
    if text != original:
        path.write_text(text)

# Update simple test constructors from enum backend values to versioned refs.
for path in [
    Path('crates/aurora-config/tests/contracts.rs'),
    Path('crates/aurora-runtime-assembly/src/derivation.rs'),
    Path('crates/aurora-runtime-assembly/src/setup.rs'),
    Path('crates/aurora-runtime-assembly/tests/contracts.rs'),
    Path('crates/aurora-runtime-inspection/src/model/tests.rs'),
]:
    text = path.read_text()
    text = text.replace('backend: BackendIntent::Cpal,\n            direction: DeviceDirection::Output,', 'backend: common_backend_reference("org.aurora.backend.cpal", DeviceDirection::Output),\n            direction: DeviceDirection::Output,')
    text = text.replace('backend: BackendIntent::Cpal,\n        direction: DeviceDirection::Output,', 'backend: common_backend_reference("org.aurora.backend.cpal", DeviceDirection::Output),\n        direction: DeviceDirection::Output,')
    text = text.replace('backend: BackendIntent::Cpal,\n            direction: DeviceDirection::Input,', 'backend: common_backend_reference("org.aurora.backend.cpal", DeviceDirection::Input),\n            direction: DeviceDirection::Input,')
    text = text.replace('backend: BackendIntent::Cpal,\n        direction: DeviceDirection::Input,', 'backend: common_backend_reference("org.aurora.backend.cpal", DeviceDirection::Input),\n        direction: DeviceDirection::Input,')
    text = text.replace('backend: BackendIntent::Virtual,\n            direction: DeviceDirection::Output,', 'backend: common_backend_reference("org.aurora.backend.virtual", DeviceDirection::Output),\n            direction: DeviceDirection::Output,')
    text = text.replace('backend: BackendIntent::Virtual,\n        direction: DeviceDirection::Output,', 'backend: common_backend_reference("org.aurora.backend.virtual", DeviceDirection::Output),\n        direction: DeviceDirection::Output,')
    text = text.replace('backend: BackendIntent::Virtual,\n            direction: DeviceDirection::Input,', 'backend: common_backend_reference("org.aurora.backend.virtual", DeviceDirection::Input),\n            direction: DeviceDirection::Input,')
    text = text.replace('backend: BackendIntent::Virtual,\n        direction: DeviceDirection::Input,', 'backend: common_backend_reference("org.aurora.backend.virtual", DeviceDirection::Input),\n        direction: DeviceDirection::Input,')
    if text != path.read_text():
        path.write_text(text)

# Config tests get a local helper alias and v3 migration expectations.
p = Path('crates/aurora-config/tests/contracts.rs')
t = p.read_text()
t = t.replace(
    'point_source_vbap_reference, stereo, unknown_renderer_reference,\n};',
    'point_source_vbap_reference, stereo, unknown_renderer_reference, backend_reference as common_backend_reference,\n};',
)
t = t.replace('fn migration_to_v2_is_explicit_deterministic_and_fail_closed()', 'fn migration_to_v3_is_explicit_deterministic_and_fail_closed()')
t = t.replace('migrate_v1_to_v2(v1)', 'migrate_v1_to_v3(v1)')
t = t.replace('migrate_v0_to_v2(v0)', 'migrate_v0_to_v3(v0)')
t = t.replace('migrate_v1_to_v2(invalid)', 'migrate_v1_to_v3(invalid)')
t = t.replace('migrate_v1_to_v2(missing_reader.as_bytes())', 'migrate_v1_to_v3(missing_reader.as_bytes())')
t = t.replace('schema.schema_version, 2', 'schema.schema_version, 3')
t = t.replace('fixtures/config/migration-expected-v2.json', 'fixtures/config/migration-expected-v3.json')
# Add direct v2->v3 migration assertion with legacy backend strings.
marker = '''    assert!(migrated_v0.configuration.semantically_eq(&expected));\n\n    let invalid = br#"{\\"schema\\":{\\"schema_version\\":2}}"#;'''
addition = '''    assert!(migrated_v0.configuration.semantically_eq(&expected));\n\n    let mut legacy_v2: serde_json::Value = serde_json::from_slice(include_bytes!(\n        "../../../fixtures/config/stereo-basic-v2.json"\n    )).unwrap();\n    legacy_v2["input_device"] = serde_json::json!({\n        "stable_id":"capture-1", "friendly_name":null, "backend":"virtual",\n        "direction":"input", "ambiguity_policy":"require_stable_identifier"\n    });\n    legacy_v2["output_device"] = serde_json::json!({\n        "stable_id":"playback-1", "friendly_name":null, "backend":"cpal",\n        "direction":"output", "ambiguity_policy":"require_stable_identifier"\n    });\n    let migrated_v2 = migrate_v2_to_v3(&serde_json::to_vec(&legacy_v2).unwrap()).unwrap();\n    assert!(migrated_v2.changed_fields.contains("input_device.backend"));\n    assert!(migrated_v2.changed_fields.contains("output_device.backend"));\n    assert_eq!(migrated_v2.configuration.config().input_device.as_ref().unwrap().backend.component_id, "org.aurora.backend.virtual");\n    assert_eq!(migrated_v2.configuration.config().output_device.as_ref().unwrap().backend.component_id, "org.aurora.backend.cpal");\n\n    let invalid = br#"{\\"schema\\":{\\"schema_version\\":2}}"#;'''
if marker in t:
    t = t.replace(marker, addition, 1)
p.write_text(t)

# --- runtime component identities -----------------------------------------
p = Path('crates/aurora-runtime-assembly/src/lib.rs')
t = p.read_text()
t = replace_once(
    t,
    'use aurora_config::{AmbiguityPolicy, BackendIntent, FormatFallbackPolicy, SampleFormatIntent};',
    'use aurora_config::{AmbiguityPolicy, ComponentContractKind, FormatFallbackPolicy, SampleFormatIntent};',
    'runtime assembly config imports',
)
t = replace_once(t, 'mod derivation;\nmod renderer_registry;\nmod setup;', 'mod backend_registry;\nmod derivation;\nmod renderer_registry;\nmod setup;', 'backend registry module')
t = replace_once(
    t,
    'pub use derivation::{prepare_runtime_plan, prepare_runtime_plan_with_registry};',
    'pub use derivation::{prepare_runtime_plan, prepare_runtime_plan_with_registries, prepare_runtime_plan_with_registry};',
    'derivation exports',
)
t = replace_once(
    t,
    '''pub use renderer_registry::{\n    RendererComponentRegistration, RendererComponentRegistry, RendererConfigurationResolver,\n    BASIC_RENDERER_IMPLEMENTATION_VERSION, REALTIME_RENDERER_CONTRACT_MINOR,\n    VBAP_RENDERER_IMPLEMENTATION_VERSION,\n};''',
    '''pub use backend_registry::{\n    BackendComponentCapabilities, BackendComponentRegistration, BackendComponentRegistry,\n    BackendConfigurationResolver, BackendPlatformAvailability, CPAL_BACKEND_IMPLEMENTATION_ID,\n    CPAL_BACKEND_IMPLEMENTATION_VERSION, OFFLINE_BACKEND_IMPLEMENTATION_ID,\n    OFFLINE_BACKEND_IMPLEMENTATION_VERSION, REALTIME_BACKEND_CONTRACT_MINOR,\n    VIRTUAL_BACKEND_IMPLEMENTATION_ID, VIRTUAL_BACKEND_IMPLEMENTATION_VERSION,\n};\npub use renderer_registry::{\n    RendererComponentRegistration, RendererComponentRegistry, RendererConfigurationResolver,\n};''',
    'registry exports',
)
const_anchor = '''pub const REALTIME_RENDERER_CONTRACT_VERSION: u16 = 1;\n/// Compatible realtime-delay contract version selected by runtime assembly.\npub const REALTIME_DELAY_CONTRACT_VERSION: u16 = 1;'''
const_new = '''pub const REALTIME_RENDERER_CONTRACT_VERSION: u16 = 1;\n/// Compatible renderer contract minor selected by runtime assembly.\npub const REALTIME_RENDERER_CONTRACT_MINOR: u16 = 0;\n/// Aurora basic renderer implementation version.\npub const BASIC_RENDERER_IMPLEMENTATION_VERSION: &str = "0.1.0";\n/// Aurora VBAP renderer implementation version.\npub const VBAP_RENDERER_IMPLEMENTATION_VERSION: &str = "0.1.0";\n/// Compatible realtime-delay contract version selected by runtime assembly.\npub const REALTIME_DELAY_CONTRACT_VERSION: u16 = 1;\n/// Compatible realtime-delay contract minor selected by runtime assembly.\npub const REALTIME_DELAY_CONTRACT_MINOR: u16 = 0;\n/// Aurora basic delay implementation version.\npub const BASIC_DELAY_IMPLEMENTATION_VERSION: &str = "0.1.0";'''
t = replace_once(t, const_anchor, const_new, 'component version constants')
# Realtime delay identity constructor.
t = replace_once(
    t,
    '''PreparedComponentIdentity::new(\n                BASIC_DELAY_IMPLEMENTATION_ID,\n                REALTIME_DELAY_CONTRACT_VERSION,\n            )''',
    '''PreparedComponentIdentity::new(\n                BASIC_DELAY_IMPLEMENTATION_ID,\n                BASIC_DELAY_IMPLEMENTATION_VERSION,\n                REALTIME_DELAY_CONTRACT_VERSION,\n                REALTIME_DELAY_CONTRACT_MINOR,\n            )''',
    'delay component identity',
)
old_identity = '''/// Stable implementation identity plus the Aurora contract version it is prepared against.\n#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub struct PreparedComponentIdentity {\n    implementation_id: &'static str,\n    contract_version: u16,\n}\n\nimpl PreparedComponentIdentity {\n    /// Creates one deterministic prepared component identity.\n    pub const fn new(implementation_id: &'static str, contract_version: u16) -> Self {\n        Self {\n            implementation_id,\n            contract_version,\n        }\n    }\n\n    /// Returns the stable implementation identifier.\n    pub const fn implementation_id(self) -> &'static str {\n        self.implementation_id\n    }\n\n    /// Returns the compatible Aurora component-contract version.\n    pub const fn contract_version(self) -> u16 {\n        self.contract_version\n    }\n}\n'''
new_identity = '''/// Stable implementation identity plus exact implementation and Aurora contract versions.\n#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub struct PreparedComponentIdentity {\n    implementation_id: &'static str,\n    implementation_version: &'static str,\n    contract_major: u16,\n    contract_minor: u16,\n}\n\nimpl PreparedComponentIdentity {\n    /// Creates one deterministic prepared component identity.\n    pub const fn new(\n        implementation_id: &'static str,\n        implementation_version: &'static str,\n        contract_major: u16,\n        contract_minor: u16,\n    ) -> Self {\n        Self { implementation_id, implementation_version, contract_major, contract_minor }\n    }\n\n    pub const fn implementation_id(self) -> &'static str { self.implementation_id }\n    pub const fn implementation_version(self) -> &'static str { self.implementation_version }\n    pub const fn contract_major(self) -> u16 { self.contract_major }\n    pub const fn contract_minor(self) -> u16 { self.contract_minor }\n    /// Compatibility alias retained for callers that treated the old version as major.\n    pub const fn contract_version(self) -> u16 { self.contract_major }\n}\n'''
t = replace_once(t, old_identity, new_identity, 'PreparedComponentIdentity')
# Renderer constructors get exact versions.
t = t.replace(
    'PreparedComponentIdentity::new(\n                BASIC_RENDERER_IMPLEMENTATION_ID,\n                REALTIME_RENDERER_CONTRACT_VERSION,\n            )',
    'PreparedComponentIdentity::new(\n                BASIC_RENDERER_IMPLEMENTATION_ID,\n                BASIC_RENDERER_IMPLEMENTATION_VERSION,\n                REALTIME_RENDERER_CONTRACT_VERSION,\n                REALTIME_RENDERER_CONTRACT_MINOR,\n            )',
)
t = t.replace(
    'PreparedComponentIdentity::new(\n                VBAP_RENDERER_IMPLEMENTATION_ID,\n                REALTIME_RENDERER_CONTRACT_VERSION,\n            )',
    'PreparedComponentIdentity::new(\n                VBAP_RENDERER_IMPLEMENTATION_ID,\n                VBAP_RENDERER_IMPLEMENTATION_VERSION,\n                REALTIME_RENDERER_CONTRACT_VERSION,\n                REALTIME_RENDERER_CONTRACT_MINOR,\n            )',
)
# Replace device selector backend enum with a prepared component descriptor.
t = replace_once(
    t,
    '''/// Passive selector copied from validated configuration intent.\n#[derive(Clone, Debug, Eq, PartialEq)]\npub struct PreparedDeviceSelectorIntent {\n    stable_id: Option<String>,\n    friendly_name: Option<String>,\n    backend: BackendIntent,\n    ambiguity_policy: AmbiguityPolicy,\n}\n''',
    '''/// Prepared backend component selected by a fail-closed registry.\n#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub struct PreparedBackendComponentIntent {\n    identity: PreparedComponentIdentity,\n    contract_kind: ComponentContractKind,\n    configuration_schema: u16,\n}\n\nimpl PreparedBackendComponentIntent {\n    pub const fn new(identity: PreparedComponentIdentity, contract_kind: ComponentContractKind, configuration_schema: u16) -> Self {\n        Self { identity, contract_kind, configuration_schema }\n    }\n    pub const fn identity(self) -> PreparedComponentIdentity { self.identity }\n    pub const fn contract_kind(self) -> ComponentContractKind { self.contract_kind }\n    pub const fn configuration_schema(self) -> u16 { self.configuration_schema }\n}\n\n/// Passive selector copied from validated configuration intent after backend resolution.\n#[derive(Clone, Debug, Eq, PartialEq)]\npub struct PreparedDeviceSelectorIntent {\n    stable_id: Option<String>,\n    friendly_name: Option<String>,\n    backend: PreparedBackendComponentIntent,\n    ambiguity_policy: AmbiguityPolicy,\n}\n''',
    'prepared backend intent',
)
t = replace_once(t, '        backend: BackendIntent,', '        backend: PreparedBackendComponentIntent,', 'selector constructor backend')
t = replace_once(
    t,
    '''    /// Returns the requested backend family.\n    pub fn backend(&self) -> BackendIntent {\n        self.backend\n    }''',
    '''    /// Returns the prepared backend component selection.\n    pub fn backend(&self) -> PreparedBackendComponentIntent {\n        self.backend\n    }''',
    'selector backend getter',
)
# Add backend registry errors.
t = replace_once(
    t,
    '''    RendererComponent {\n        component_id: String,\n        issue: RendererComponentIssue,\n    },''',
    '''    RendererComponent {\n        component_id: String,\n        issue: RendererComponentIssue,\n    },\n    /// Audio backend component registry resolution failed before activation.\n    BackendComponent {\n        component_id: String,\n        issue: BackendComponentIssue,\n    },''',
    'backend runtime error variant',
)
renderer_issue_end = '''pub enum RendererComponentIssue {\n    /// No registry entry exists for the requested component ID.\n    UnknownComponentId,\n    /// The component reference does not target the renderer contract.\n    ContractKindMismatch,\n    /// Contract major/minor compatibility does not include the registered contract.\n    IncompatibleContractVersion,\n    /// An exact requested implementation version differs from the registered version.\n    IncompatibleImplementationVersion,\n    /// The component-specific configuration schema is unsupported.\n    UnsupportedConfigurationSchema,\n    /// Component-specific configuration could not be validated.\n    InvalidConfiguration,\n    /// The requested layout cannot satisfy component capabilities.\n    LayoutCapabilityMismatch,\n    /// Registry construction attempted to add a duplicate component ID.\n    DuplicateRegistration,\n}\n'''
backend_issue = renderer_issue_end + '''\n/// Stable audio-backend component registry failure categories.\n#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub enum BackendComponentIssue {\n    UnknownComponentId,\n    ContractKindMismatch,\n    IncompatibleContractVersion,\n    IncompatibleImplementationVersion,\n    UnsupportedConfigurationSchema,\n    InvalidConfiguration,\n    FormatCapabilityMismatch,\n    RealtimeSafetyMismatch,\n    PlatformUnavailable,\n    DuplicateRegistration,\n}\n'''
t = replace_once(t, renderer_issue_end, backend_issue, 'BackendComponentIssue')
# Display arm.
t = replace_once(
    t,
    '''            Self::RendererComponent {\n                component_id,\n                issue,\n            } => {\n                write!(\n                    formatter,\n                    "renderer component {component_id} rejected: {issue:?}"\n                )\n            }''',
    '''            Self::RendererComponent { component_id, issue } => {\n                write!(formatter, "renderer component {component_id} rejected: {issue:?}")\n            }\n            Self::BackendComponent { component_id, issue } => {\n                write!(formatter, "backend component {component_id} rejected: {issue:?}")\n            }''',
    'backend display arm',
)
p.write_text(t)

# Renderer registry uses exact identity versions from crate constants.
p = Path('crates/aurora-runtime-assembly/src/renderer_registry.rs')
t = p.read_text()
t = t.replace(
    '    PreparedComponentIdentity, PreparedRendererPlan, RendererComponentIssue,\n    RuntimePreparationError, BASIC_RENDERER_IMPLEMENTATION_ID, REALTIME_RENDERER_CONTRACT_VERSION,\n    VBAP_RENDERER_IMPLEMENTATION_ID,\n};\n\npub const REALTIME_RENDERER_CONTRACT_MINOR: u16 = 0;\npub const BASIC_RENDERER_IMPLEMENTATION_VERSION: &str = "0.1.0";\npub const VBAP_RENDERER_IMPLEMENTATION_VERSION: &str = "0.1.0";',
    '    PreparedComponentIdentity, PreparedRendererPlan, RendererComponentIssue,\n    RuntimePreparationError, BASIC_RENDERER_IMPLEMENTATION_ID,\n    BASIC_RENDERER_IMPLEMENTATION_VERSION, REALTIME_RENDERER_CONTRACT_MINOR,\n    REALTIME_RENDERER_CONTRACT_VERSION, VBAP_RENDERER_IMPLEMENTATION_ID,\n    VBAP_RENDERER_IMPLEMENTATION_VERSION,\n};',
)
t = replace_once(
    t,
    '''PreparedComponentIdentity::new(\n            registration.component_id,\n            registration.contract_major,\n        )''',
    '''PreparedComponentIdentity::new(\n            registration.component_id,\n            registration.implementation_version,\n            registration.contract_major,\n            registration.contract_minor,\n        )''',
    'renderer registry exact identity',
)
p.write_text(t)

# New backend registry.
backend_registry = r'''use aurora_config::{
    ComponentContractKind, ComponentReference, DeviceDirection, OperatingMode,
};
use serde_json::Value;

use crate::{
    BackendComponentIssue, PreparedBackendComponentIntent, PreparedComponentIdentity,
    RuntimePreparationError,
};

pub const REALTIME_BACKEND_CONTRACT_MINOR: u16 = 0;
pub const VIRTUAL_BACKEND_IMPLEMENTATION_ID: &str = "org.aurora.backend.virtual";
pub const CPAL_BACKEND_IMPLEMENTATION_ID: &str = "org.aurora.backend.cpal";
pub const OFFLINE_BACKEND_IMPLEMENTATION_ID: &str = "org.aurora.backend.offline";
pub const VIRTUAL_BACKEND_IMPLEMENTATION_VERSION: &str = "0.1.0";
pub const CPAL_BACKEND_IMPLEMENTATION_VERSION: &str = "0.1.0";
pub const OFFLINE_BACKEND_IMPLEMENTATION_VERSION: &str = "0.1.0";
const REALTIME_BACKEND_CONTRACT_VERSION: u16 = 1;

pub type BackendConfigurationResolver = fn(&Value) -> Result<(), BackendComponentIssue>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendPlatformAvailability {
    Portable,
    HostAudioBuild,
    OfflineOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackendComponentCapabilities {
    min_sample_rate: u32,
    max_sample_rate: u32,
    max_channels: usize,
    realtime_safe: bool,
    platform: BackendPlatformAvailability,
}

impl BackendComponentCapabilities {
    pub const fn new(
        min_sample_rate: u32,
        max_sample_rate: u32,
        max_channels: usize,
        realtime_safe: bool,
        platform: BackendPlatformAvailability,
    ) -> Self {
        Self { min_sample_rate, max_sample_rate, max_channels, realtime_safe, platform }
    }
}

#[derive(Clone, Copy)]
pub struct BackendComponentRegistration {
    component_id: &'static str,
    implementation_version: &'static str,
    contract_kind: ComponentContractKind,
    contract_major: u16,
    contract_minor: u16,
    configuration_schema: u16,
    capabilities: BackendComponentCapabilities,
    resolver: BackendConfigurationResolver,
}

impl BackendComponentRegistration {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        component_id: &'static str,
        implementation_version: &'static str,
        contract_kind: ComponentContractKind,
        contract_major: u16,
        contract_minor: u16,
        configuration_schema: u16,
        capabilities: BackendComponentCapabilities,
        resolver: BackendConfigurationResolver,
    ) -> Self {
        Self { component_id, implementation_version, contract_kind, contract_major, contract_minor, configuration_schema, capabilities, resolver }
    }
    pub const fn component_id(self) -> &'static str { self.component_id }
    pub const fn implementation_version(self) -> &'static str { self.implementation_version }
    pub const fn contract_kind(self) -> ComponentContractKind { self.contract_kind }
}

#[derive(Clone)]
pub struct BackendComponentRegistry {
    registrations: Vec<BackendComponentRegistration>,
}

impl BackendComponentRegistry {
    pub fn builtin() -> Self {
        let portable = BackendComponentCapabilities::new(8_000, 192_000, 32, true, BackendPlatformAvailability::Portable);
        let host = BackendComponentCapabilities::new(8_000, 192_000, 32, true, BackendPlatformAvailability::HostAudioBuild);
        let offline = BackendComponentCapabilities::new(8_000, 192_000, 32, false, BackendPlatformAvailability::OfflineOnly);
        let mut registrations = Vec::new();
        for kind in [ComponentContractKind::AudioInputBackend, ComponentContractKind::AudioOutputBackend] {
            registrations.push(BackendComponentRegistration::new(VIRTUAL_BACKEND_IMPLEMENTATION_ID, VIRTUAL_BACKEND_IMPLEMENTATION_VERSION, kind, REALTIME_BACKEND_CONTRACT_VERSION, REALTIME_BACKEND_CONTRACT_MINOR, 1, portable, resolve_empty));
            registrations.push(BackendComponentRegistration::new(CPAL_BACKEND_IMPLEMENTATION_ID, CPAL_BACKEND_IMPLEMENTATION_VERSION, kind, REALTIME_BACKEND_CONTRACT_VERSION, REALTIME_BACKEND_CONTRACT_MINOR, 1, host, resolve_empty));
            registrations.push(BackendComponentRegistration::new(OFFLINE_BACKEND_IMPLEMENTATION_ID, OFFLINE_BACKEND_IMPLEMENTATION_VERSION, kind, REALTIME_BACKEND_CONTRACT_VERSION, REALTIME_BACKEND_CONTRACT_MINOR, 1, offline, resolve_empty));
        }
        Self { registrations }
    }

    pub fn with_registration(mut self, registration: BackendComponentRegistration) -> Result<Self, RuntimePreparationError> {
        if self.registrations.iter().any(|entry| entry.component_id == registration.component_id && entry.contract_kind == registration.contract_kind) {
            return Err(component_error(registration.component_id, BackendComponentIssue::DuplicateRegistration));
        }
        self.registrations.push(registration);
        Ok(self)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn resolve(
        &self,
        reference: &ComponentReference,
        direction: DeviceDirection,
        sample_rate: u32,
        channel_count: usize,
        operating_mode: OperatingMode,
    ) -> Result<PreparedBackendComponentIntent, RuntimePreparationError> {
        let expected_kind = match direction {
            DeviceDirection::Input => ComponentContractKind::AudioInputBackend,
            DeviceDirection::Output => ComponentContractKind::AudioOutputBackend,
        };
        if reference.contract_kind != expected_kind {
            return Err(component_error(&reference.component_id, BackendComponentIssue::ContractKindMismatch));
        }
        let registration = self.registrations.iter().find(|entry| entry.component_id == reference.component_id && entry.contract_kind == expected_kind).copied().ok_or_else(|| component_error(&reference.component_id, BackendComponentIssue::UnknownComponentId))?;
        if reference.contract_major != registration.contract_major
            || reference.compatible_minor.minimum > registration.contract_minor
            || reference.compatible_minor.maximum < registration.contract_minor
        {
            return Err(component_error(&reference.component_id, BackendComponentIssue::IncompatibleContractVersion));
        }
        if reference.implementation_version_pin.as_deref().is_some_and(|pin| pin != registration.implementation_version) {
            return Err(component_error(&reference.component_id, BackendComponentIssue::IncompatibleImplementationVersion));
        }
        if reference.configuration_schema != registration.configuration_schema {
            return Err(component_error(&reference.component_id, BackendComponentIssue::UnsupportedConfigurationSchema));
        }
        (registration.resolver)(&reference.configuration).map_err(|issue| component_error(&reference.component_id, issue))?;
        let caps = registration.capabilities;
        if sample_rate < caps.min_sample_rate || sample_rate > caps.max_sample_rate || channel_count > caps.max_channels {
            return Err(component_error(&reference.component_id, BackendComponentIssue::FormatCapabilityMismatch));
        }
        if operating_mode == OperatingMode::LiveIntent && !caps.realtime_safe {
            return Err(component_error(&reference.component_id, BackendComponentIssue::RealtimeSafetyMismatch));
        }
        let platform_ok = match caps.platform {
            BackendPlatformAvailability::Portable => true,
            BackendPlatformAvailability::HostAudioBuild => cfg!(any(target_os = "linux", target_os = "windows", target_os = "macos", target_os = "android", target_os = "ios")),
            BackendPlatformAvailability::OfflineOnly => operating_mode == OperatingMode::Offline,
        };
        if !platform_ok {
            return Err(component_error(&reference.component_id, BackendComponentIssue::PlatformUnavailable));
        }
        Ok(PreparedBackendComponentIntent::new(
            PreparedComponentIdentity::new(registration.component_id, registration.implementation_version, registration.contract_major, registration.contract_minor),
            registration.contract_kind,
            registration.configuration_schema,
        ))
    }
}

fn resolve_empty(payload: &Value) -> Result<(), BackendComponentIssue> {
    match payload.as_object() {
        Some(values) if values.is_empty() => Ok(()),
        _ => Err(BackendComponentIssue::InvalidConfiguration),
    }
}

fn component_error(component_id: &str, issue: BackendComponentIssue) -> RuntimePreparationError {
    RuntimePreparationError::BackendComponent { component_id: component_id.to_owned(), issue }
}
'''
write('crates/aurora-runtime-assembly/src/backend_registry.rs', backend_registry)

# Derivation: use both registries and resolve backend before producing selectors.
p = Path('crates/aurora-runtime-assembly/src/derivation.rs')
t = p.read_text()
t = t.replace(
    '    PreparedTopologyPlan, RendererComponentRegistry, RuntimeCapacityPlan, RuntimeInvariant,\n    RuntimePlanMetadata, RuntimePreparationError,\n};',
    '    BackendComponentRegistry, PreparedTopologyPlan, RendererComponentRegistry, RuntimeCapacityPlan, RuntimeInvariant,\n    RuntimePlanMetadata, RuntimePreparationError,\n};',
)
old_entry = '''pub fn prepare_runtime_plan(\n    configuration: &ValidatedConfiguration,\n) -> Result<PreparedRuntimePlan, RuntimePreparationError> {\n    let registry = RendererComponentRegistry::builtin();\n    prepare_runtime_plan_with_registry(configuration, &registry)\n}\n\n/// Derives a runtime plan using an explicit renderer-component registry.\npub fn prepare_runtime_plan_with_registry(\n    configuration: &ValidatedConfiguration,\n    registry: &RendererComponentRegistry,\n) -> Result<PreparedRuntimePlan, RuntimePreparationError> {\n'''
new_entry = '''pub fn prepare_runtime_plan(\n    configuration: &ValidatedConfiguration,\n) -> Result<PreparedRuntimePlan, RuntimePreparationError> {\n    let renderer_registry = RendererComponentRegistry::builtin();\n    let backend_registry = BackendComponentRegistry::builtin();\n    prepare_runtime_plan_with_registries(configuration, &renderer_registry, &backend_registry)\n}\n\n/// Derives a runtime plan using an explicit renderer registry and the built-in backend registry.\npub fn prepare_runtime_plan_with_registry(\n    configuration: &ValidatedConfiguration,\n    renderer_registry: &RendererComponentRegistry,\n) -> Result<PreparedRuntimePlan, RuntimePreparationError> {\n    let backend_registry = BackendComponentRegistry::builtin();\n    prepare_runtime_plan_with_registries(configuration, renderer_registry, &backend_registry)\n}\n\n/// Derives a runtime plan using explicit renderer and audio-backend registries.\npub fn prepare_runtime_plan_with_registries(\n    configuration: &ValidatedConfiguration,\n    renderer_registry: &RendererComponentRegistry,\n    backend_registry: &BackendComponentRegistry,\n) -> Result<PreparedRuntimePlan, RuntimePreparationError> {\n'''
t = replace_once(t, old_entry, new_entry, 'derivation entrypoints')
t = replace_once(t, '    let renderer = registry.resolve(&config.renderer, active_speakers)?;', '    let renderer = renderer_registry.resolve(&config.renderer, active_speakers)?;', 'renderer registry variable')
old_devices = '''    let device_intent = PreparedDeviceIntent::new(\n        prepare_device_selector(config.input_device.as_ref(), DeviceDirection::Input)?,\n        prepare_device_selector(config.output_device.as_ref(), DeviceDirection::Output)?,\n    );'''
new_devices = '''    let device_intent = PreparedDeviceIntent::new(\n        prepare_device_selector(\n            config.input_device.as_ref(),\n            DeviceDirection::Input,\n            backend_registry,\n            config.engine.operating_mode,\n            config.audio_format.sample_rate,\n            input_channel_count,\n        )?,\n        prepare_device_selector(\n            config.output_device.as_ref(),\n            DeviceDirection::Output,\n            backend_registry,\n            config.engine.operating_mode,\n            config.audio_format.sample_rate,\n            output_channel_count,\n        )?,\n    );'''
t = replace_once(t, old_devices, new_devices, 'device registry resolution')
old_fn = '''fn prepare_device_selector(\n    selector: Option<&DeviceSelectionIntent>,\n    expected_direction: DeviceDirection,\n) -> Result<Option<PreparedDeviceSelectorIntent>, RuntimePreparationError> {'''
new_fn = '''fn prepare_device_selector(\n    selector: Option<&DeviceSelectionIntent>,\n    expected_direction: DeviceDirection,\n    backend_registry: &BackendComponentRegistry,\n    operating_mode: aurora_config::OperatingMode,\n    sample_rate: u32,\n    channel_count: usize,\n) -> Result<Option<PreparedDeviceSelectorIntent>, RuntimePreparationError> {'''
t = replace_once(t, old_fn, new_fn, 'prepare_device_selector signature')
t = replace_once(
    t,
    '''    PreparedDeviceSelectorIntent::new(\n        selector.stable_id.clone(),\n        selector.friendly_name.clone(),\n        selector.backend,\n        selector.ambiguity_policy,\n    )''',
    '''    let backend = backend_registry.resolve(\n        &selector.backend,\n        expected_direction,\n        sample_rate,\n        channel_count,\n        operating_mode,\n    )?;\n    PreparedDeviceSelectorIntent::new(\n        selector.stable_id.clone(),\n        selector.friendly_name.clone(),\n        backend,\n        selector.ambiguity_policy,\n    )''',
    'prepare selector backend',
)
# Test import cleanup and local helper.
t = t.replace('AmbiguityPolicy, BackendIntent, CompatibleMinorRange, ComponentContractKind,', 'AmbiguityPolicy, CompatibleMinorRange, ComponentContractKind,')
# If the helper alias was introduced by generic replacements, add helper function inside tests.
t = t.replace(
    '    fn validated(bytes: &[u8]) -> ValidatedConfiguration {',
    '    fn common_backend_reference(component_id: &str, direction: DeviceDirection) -> ComponentReference {\n        ComponentReference { component_id: component_id.to_owned(), contract_kind: match direction { DeviceDirection::Input => ComponentContractKind::AudioInputBackend, DeviceDirection::Output => ComponentContractKind::AudioOutputBackend }, contract_major: 1, compatible_minor: CompatibleMinorRange { minimum: 0, maximum: 0 }, implementation_version_pin: None, configuration_schema: 1, configuration: serde_json::json!({}) }\n    }\n\n    fn validated(bytes: &[u8]) -> ValidatedConfiguration {',
    1,
)
p.write_text(t)

# Setup plan carries prepared backend descriptors, not implementation enums.
p = Path('crates/aurora-runtime-assembly/src/setup.rs')
t = p.read_text()
t = t.replace('use aurora_config::BackendIntent;\n\n', '')
t = t.replace(
    '    PreparedAudioFormatIntent, PreparedDeviceIntent, PreparedDspPlan, PreparedRendererPlan,\n    PreparedRuntimePlan, PreparedTopologyPlan,',
    '    PreparedAudioFormatIntent, PreparedBackendComponentIntent, PreparedDeviceIntent, PreparedDspPlan, PreparedRendererPlan,\n    PreparedRuntimePlan, PreparedTopologyPlan,',
)
t = t.replace('requested_input_backend: Option<BackendIntent>,', 'requested_input_backend: Option<PreparedBackendComponentIntent>,')
t = t.replace('requested_output_backend: Option<BackendIntent>,', 'requested_output_backend: Option<PreparedBackendComponentIntent>,')
t = t.replace('pub fn requested_input_backend(self) -> Option<BackendIntent>', 'pub fn requested_input_backend(self) -> Option<PreparedBackendComponentIntent>')
t = t.replace('pub fn requested_output_backend(self) -> Option<BackendIntent>', 'pub fn requested_output_backend(self) -> Option<PreparedBackendComponentIntent>')
t = t.replace('AmbiguityPolicy, BackendIntent, DeviceDirection, DeviceSelectionIntent,', 'AmbiguityPolicy, CompatibleMinorRange, ComponentContractKind, ComponentReference, DeviceDirection, DeviceSelectionIntent,')
# Add test helper if tests module exists.
t = t.replace(
    '    fn validated(bytes: &[u8]) -> ValidatedConfiguration {',
    '    fn common_backend_reference(component_id: &str, direction: DeviceDirection) -> ComponentReference {\n        ComponentReference { component_id: component_id.to_owned(), contract_kind: match direction { DeviceDirection::Input => ComponentContractKind::AudioInputBackend, DeviceDirection::Output => ComponentContractKind::AudioOutputBackend }, contract_major: 1, compatible_minor: CompatibleMinorRange { minimum: 0, maximum: 0 }, implementation_version_pin: None, configuration_schema: 1, configuration: serde_json::json!({}) }\n    }\n\n    fn validated(bytes: &[u8]) -> ValidatedConfiguration {',
    1,
)
p.write_text(t)

# Runtime assembly integration tests: helper and enum removal.
p = Path('crates/aurora-runtime-assembly/tests/contracts.rs')
t = p.read_text()
t = t.replace('AmbiguityPolicy, BackendIntent, DeviceDirection, DeviceSelectionIntent, ValidatedConfiguration,', 'AmbiguityPolicy, CompatibleMinorRange, ComponentContractKind, ComponentReference, DeviceDirection, DeviceSelectionIntent, ValidatedConfiguration,')
if 'fn common_backend_reference' not in t:
    insert_at = t.index('\n\n#[test]')
    helper = '''\n\nfn common_backend_reference(component_id: &str, direction: DeviceDirection) -> ComponentReference {\n    ComponentReference { component_id: component_id.to_owned(), contract_kind: match direction { DeviceDirection::Input => ComponentContractKind::AudioInputBackend, DeviceDirection::Output => ComponentContractKind::AudioOutputBackend }, contract_major: 1, compatible_minor: CompatibleMinorRange { minimum: 0, maximum: 0 }, implementation_version_pin: None, configuration_schema: 1, configuration: serde_json::json!({}) }\n}\n'''
    t = t[:insert_at] + helper + t[insert_at:]
p.write_text(t)

# --- runtime inspection exact component identity --------------------------
p = Path('crates/aurora-runtime-inspection/src/model.rs')
t = p.read_text()
t = replace_once(t, 'pub const INSPECTION_SCHEMA_VERSION: u16 = 2;', 'pub const INSPECTION_SCHEMA_VERSION: u16 = 3;', 'inspection schema')
old_projection = '''pub struct ComponentIdentityProjection {\n    /// Stable implementation identifier selected by runtime assembly.\n    pub implementation_id: String,\n    /// Aurora contract version the selection is prepared against.\n    pub contract_version: u16,\n}\n'''
new_projection = '''pub struct ComponentIdentityProjection {\n    /// Stable implementation identifier selected by runtime assembly.\n    pub implementation_id: String,\n    /// Exact selected implementation version.\n    pub implementation_version: String,\n    /// Aurora contract major version.\n    pub contract_major: u16,\n    /// Aurora contract minor version.\n    pub contract_minor: u16,\n}\n'''
t = replace_once(t, old_projection, new_projection, 'inspection component identity')
old_device_projection = '''    /// Requested backend family.\n    pub requested_backend: String,\n    /// Requested ambiguity policy.\n    pub ambiguity_policy: String,'''
new_device_projection = '''    /// Prepared backend component identity selected from the requested reference.\n    pub backend_component: ComponentIdentityProjection,\n    /// Direction-specific Aurora backend contract kind.\n    pub backend_contract_kind: String,\n    /// Backend component configuration schema.\n    pub backend_configuration_schema: u16,\n    /// Requested ambiguity policy.\n    pub ambiguity_policy: String,'''
t = replace_once(t, old_device_projection, new_device_projection, 'device backend projection')
# Projection of renderer/delay identities.
t = t.replace(
    '''implementation_id: self\n                        .source_string(renderer_component.implementation_id())?,\n                    contract_version: renderer_component.contract_version(),''',
    '''implementation_id: self.source_string(renderer_component.implementation_id())?,\n                    implementation_version: self.source_string(renderer_component.implementation_version())?,\n                    contract_major: renderer_component.contract_major(),\n                    contract_minor: renderer_component.contract_minor(),''',
)
t = t.replace(
    '''implementation_id: self.source_string(delay_component.implementation_id())?,\n                    contract_version: delay_component.contract_version(),''',
    '''implementation_id: self.source_string(delay_component.implementation_id())?,\n                    implementation_version: self.source_string(delay_component.implementation_version())?,\n                    contract_major: delay_component.contract_major(),\n                    contract_minor: delay_component.contract_minor(),''',
)
old_device_map = '                    requested_backend: debug_value(selector.backend()),\n                    ambiguity_policy: debug_value(selector.ambiguity_policy()),'
new_device_map = '''                    backend_component: {\n                        let identity = selector.backend().identity();\n                        ComponentIdentityProjection {\n                            implementation_id: self.source_string(identity.implementation_id())?,\n                            implementation_version: self.source_string(identity.implementation_version())?,\n                            contract_major: identity.contract_major(),\n                            contract_minor: identity.contract_minor(),\n                        }\n                    },\n                    backend_contract_kind: debug_value(selector.backend().contract_kind()),\n                    backend_configuration_schema: selector.backend().configuration_schema(),\n                    ambiguity_policy: debug_value(selector.ambiguity_policy()),'''
t = replace_once(t, old_device_map, new_device_map, 'device backend inspection')
# Setup summary now emits stable component IDs.
t = t.replace('plan.backend().requested_input_backend().map(debug_value)', 'plan.backend().requested_input_backend().map(|backend| backend.identity().implementation_id().to_owned())')
t = t.replace('plan.backend().requested_output_backend().map(debug_value)', 'plan.backend().requested_output_backend().map(|backend| backend.identity().implementation_id().to_owned())')
p.write_text(t)

# Text formatter for richer identities and device backend details.
p = Path('crates/aurora-runtime-inspection/src/formatter/text.rs')
t = p.read_text()
t = t.replace(
    '''        output.line(&format!(\n            "component.renderer.contract_version: {}",\n            runtime.prepared_components.renderer.contract_version\n        ))?;''',
    '''        output.line(&format!("component.renderer.implementation_version: {}", quote(&runtime.prepared_components.renderer.implementation_version)?))?;\n        output.line(&format!("component.renderer.contract_major: {}", runtime.prepared_components.renderer.contract_major))?;\n        output.line(&format!("component.renderer.contract_minor: {}", runtime.prepared_components.renderer.contract_minor))?;''',
)
t = t.replace(
    '''        output.line(&format!(\n            "component.realtime_delay.contract_version: {}",\n            runtime.prepared_components.realtime_delay.contract_version\n        ))?;''',
    '''        output.line(&format!("component.realtime_delay.implementation_version: {}", quote(&runtime.prepared_components.realtime_delay.implementation_version)?))?;\n        output.line(&format!("component.realtime_delay.contract_major: {}", runtime.prepared_components.realtime_delay.contract_major))?;\n        output.line(&format!("component.realtime_delay.contract_minor: {}", runtime.prepared_components.realtime_delay.contract_minor))?;''',
)
t = t.replace(
    '''            output.line(&format!(\n                "{label}.backend: {}",\n                quote(&device.requested_backend)?\n            ))?;''',
    '''            output.line(&format!("{label}.backend.implementation_id: {}", quote(&device.backend_component.implementation_id)?))?;\n            output.line(&format!("{label}.backend.implementation_version: {}", quote(&device.backend_component.implementation_version)?))?;\n            output.line(&format!("{label}.backend.contract_kind: {}", quote(&device.backend_contract_kind)?))?;\n            output.line(&format!("{label}.backend.contract_major: {}", device.backend_component.contract_major))?;\n            output.line(&format!("{label}.backend.contract_minor: {}", device.backend_component.contract_minor))?;\n            output.line(&format!("{label}.backend.configuration_schema: {}", device.backend_configuration_schema))?;''',
)
p.write_text(t)

# Inspection tests helper and enum removal.
p = Path('crates/aurora-runtime-inspection/src/model/tests.rs')
t = p.read_text()
t = t.replace('AmbiguityPolicy, BackendIntent, DeviceDirection, DeviceSelectionIntent, ValidatedConfiguration,', 'AmbiguityPolicy, CompatibleMinorRange, ComponentContractKind, ComponentReference, DeviceDirection, DeviceSelectionIntent, ValidatedConfiguration,')
if 'fn common_backend_reference' not in t:
    pos = t.index('\n\nfn prepared_plans') if '\n\nfn prepared_plans' in t else t.index('\n\n#[test]')
    helper = '''\n\nfn common_backend_reference(component_id: &str, direction: DeviceDirection) -> ComponentReference {\n    ComponentReference { component_id: component_id.to_owned(), contract_kind: match direction { DeviceDirection::Input => ComponentContractKind::AudioInputBackend, DeviceDirection::Output => ComponentContractKind::AudioOutputBackend }, contract_major: 1, compatible_minor: CompatibleMinorRange { minimum: 0, maximum: 0 }, implementation_version_pin: None, configuration_schema: 1, configuration: serde_json::json!({}) }\n}\n'''
    t = t[:pos] + helper + t[pos:]
p.write_text(t)

# Update docs and handoff truth.
p = Path('docs/configuration.md')
t = p.read_text()
t += '''\n\n## Audio backend component references (schema v3)\n\nSchema v3 replaces `BackendIntent::{Virtual,Cpal,Offline}` in device selectors with direction-specific versioned `ComponentReference` values. The root configuration validates only the bounded Aurora-owned component envelope. Runtime assembly resolves input/output backend references through an explicit fail-closed registry and converts them to prepared typed backend intent before setup. Unknown IDs, incompatible contracts/versions/config schemas, invalid payloads, unsupported format/channel capability, realtime-safety mismatch, and unavailable platform class are rejected before activation. Migration from v0/v1/v2 is explicit; old documents are never silently reinterpreted as schema v3.\n'''
p.write_text(t)

p = Path('AGENTS.md')
t = p.read_text()
t = t.replace('Latest integrated architecture slice: **PR #129**, squash commit `8ac1db7293628456eded4dc61fb79f791eaf497d`.', 'Latest integrated architecture slice: **PR #129**, squash commit `8ac1db7293628456eded4dc61fb79f791eaf497d`. Active short-lived branch: `refactor/backend-component-ref-v2` for issue #130 / parent #119.')
t = t.replace('### Next slice — backend component references', '### Active slice — backend component references (#130)')
p.write_text(t)
