use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    AuroraConfiguration, ComponentReference, ConfigError, DiagnosticsPolicy, ErrorCategory,
    ErrorCode, RoutingConfiguration, SimulationProfile, SpeakerLayoutConfiguration,
    ValidatedConfiguration, CURRENT_SCHEMA_VERSION, MAX_COMPOSITION_DEPTH, MAX_DESCRIPTION_BYTES,
    MAX_PRESETS, MAX_SERIALIZED_BYTES, MAX_STRING_BYTES, MAX_TAGS,
};

/// Stable preset payload category.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresetType {
    /// Complete Aurora configuration.
    FullConfiguration,
    /// Renderer selection only.
    Renderer,
    /// Speaker layout only.
    SpeakerLayout,
    /// Routing graph only.
    Routing,
    /// Diagnostics policy only.
    Diagnostics,
    /// Simulation profile only.
    Simulation,
}

/// Immutable typed preset payload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum PresetPayload {
    /// Complete configuration payload.
    FullConfiguration(Box<AuroraConfiguration>),
    /// Renderer configuration payload.
    Renderer(ComponentReference),
    /// Speaker-layout payload.
    SpeakerLayout(SpeakerLayoutConfiguration),
    /// Routing payload.
    Routing(RoutingConfiguration),
    /// Diagnostics payload.
    Diagnostics(DiagnosticsPolicy),
    /// Simulation payload.
    Simulation(SimulationProfile),
}

impl PresetPayload {
    fn preset_type(&self) -> PresetType {
        match self {
            Self::FullConfiguration(_) => PresetType::FullConfiguration,
            Self::Renderer(_) => PresetType::Renderer,
            Self::SpeakerLayout(_) => PresetType::SpeakerLayout,
            Self::Routing(_) => PresetType::Routing,
            Self::Diagnostics(_) => PresetType::Diagnostics,
            Self::Simulation(_) => PresetType::Simulation,
        }
    }
}

/// Named bounded reusable preset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    /// Stable preset identifier.
    pub id: String,
    /// Human-readable display name.
    pub display_name: String,
    /// Preset schema version.
    pub schema_version: u16,
    /// Declared payload type, checked against the payload.
    pub preset_type: PresetType,
    /// Immutable typed payload.
    pub payload: PresetPayload,
    /// Optional bounded description.
    pub description: Option<String>,
    /// Deterministically ordered tags after collection validation.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Referenced presets applied left-to-right before this payload.
    #[serde(default)]
    pub extends: Vec<String>,
}

/// Bounded deterministic preset collection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresetCollection {
    /// Collection schema version.
    pub schema_version: u16,
    /// Presets normalized by stable ID during materialization.
    pub presets: Vec<Preset>,
}

impl PresetCollection {
    /// Parses and validates a bounded preset collection.
    pub fn from_json(bytes: &[u8]) -> Result<Self, ConfigError> {
        if bytes.len() > MAX_SERIALIZED_BYTES {
            return Err(preset_error(
                ErrorCode::SerializedSizeExceeded,
                "$",
                "serialized preset collection exceeds the byte limit",
            ));
        }
        let collection: Self = serde_json::from_slice(bytes).map_err(|_| {
            preset_error(
                ErrorCode::InvalidJson,
                "$",
                "preset collection is not valid schema JSON",
            )
        })?;
        collection.validate()?;
        Ok(collection)
    }

    /// Validates schema, IDs, tags, references, and published bounds.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(preset_error(
                ErrorCode::UnsupportedSchemaVersion,
                "schema_version",
                "unsupported preset collection schema version",
            ));
        }
        if self.presets.is_empty() || self.presets.len() > MAX_PRESETS {
            return Err(preset_error(
                ErrorCode::CollectionLimitExceeded,
                "presets",
                "preset collection is empty or exceeds its bound",
            ));
        }
        let mut ids = BTreeSet::new();
        for preset in &self.presets {
            if !ids.insert(preset.id.as_str()) {
                return Err(preset_error(
                    ErrorCode::DuplicateIdentifier,
                    "presets[].id",
                    "preset ID occurs more than once",
                ));
            }
            if preset.schema_version != CURRENT_SCHEMA_VERSION
                || preset.preset_type != preset.payload.preset_type()
            {
                return Err(preset_error(
                    ErrorCode::UnsupportedSchemaVersion,
                    "presets[]",
                    "preset schema or declared payload type is inconsistent",
                ));
            }
            for (path, value, limit) in [
                ("presets[].id", preset.id.as_str(), MAX_STRING_BYTES),
                (
                    "presets[].display_name",
                    preset.display_name.as_str(),
                    MAX_STRING_BYTES,
                ),
                (
                    "presets[].description",
                    preset.description.as_deref().unwrap_or("description"),
                    MAX_DESCRIPTION_BYTES,
                ),
            ] {
                if value.trim().is_empty() || value.len() > limit {
                    return Err(preset_error(
                        ErrorCode::InvalidString,
                        path,
                        "preset text is empty or exceeds its bound",
                    ));
                }
            }
            if preset.tags.len() > MAX_TAGS || preset.extends.len() > MAX_COMPOSITION_DEPTH {
                return Err(preset_error(
                    ErrorCode::CollectionLimitExceeded,
                    "presets[].tags_or_extends",
                    "preset tags or direct references exceed their bound",
                ));
            }
            let mut tags = BTreeSet::new();
            for tag in &preset.tags {
                if tag.trim().is_empty()
                    || tag.len() > MAX_STRING_BYTES
                    || !tags.insert(tag.as_str())
                {
                    return Err(preset_error(
                        ErrorCode::InvalidString,
                        "presets[].tags",
                        "preset tag is empty, oversized, or duplicated",
                    ));
                }
            }
            let mut sibling_types = BTreeSet::new();
            for reference in &preset.extends {
                if reference.trim().is_empty() || reference.len() > MAX_STRING_BYTES {
                    return Err(preset_error(
                        ErrorCode::InvalidString,
                        "presets[].extends",
                        "preset reference is empty or oversized",
                    ));
                }
                let referenced = self
                    .presets
                    .iter()
                    .find(|candidate| candidate.id == *reference)
                    .ok_or_else(|| {
                        preset_error(
                            ErrorCode::PresetNotFound,
                            "presets[].extends",
                            "referenced preset does not exist",
                        )
                    })?;
                if !sibling_types.insert(referenced.preset_type) {
                    return Err(preset_error(
                        ErrorCode::PresetConflict,
                        "presets[].extends",
                        "direct preset references contain conflicting payload types",
                    ));
                }
            }
        }
        Ok(())
    }

    /// Serializes the collection with presets and tags sorted.
    ///
    /// Reference order is preserved because it defines overlay precedence.
    pub fn canonical_json(&self) -> Result<String, ConfigError> {
        self.validate()?;
        let mut canonical = self.clone();
        canonical.presets.sort_by(|a, b| a.id.cmp(&b.id));
        for preset in &mut canonical.presets {
            preset.tags.sort();
        }
        serde_json::to_string(&canonical).map_err(|_| {
            preset_error(
                ErrorCode::InvalidJson,
                "$",
                "validated preset collection could not be serialized",
            )
        })
    }
}

/// Materializes one preset into a validated complete configuration.
pub fn materialize_preset(
    collection: &PresetCollection,
    preset_id: &str,
) -> Result<ValidatedConfiguration, ConfigError> {
    collection.validate()?;
    let by_id = collection
        .presets
        .iter()
        .map(|preset| (preset.id.as_str(), preset))
        .collect::<BTreeMap<_, _>>();
    let mut stack = Vec::with_capacity(MAX_COMPOSITION_DEPTH + 1);
    let mut ordered = Vec::new();
    visit(preset_id, &by_id, &mut stack, &mut ordered, 0)?;

    let mut config = None;
    for preset in ordered {
        match &preset.payload {
            PresetPayload::FullConfiguration(value) => config = Some((**value).clone()),
            PresetPayload::Renderer(value) => require_config(&mut config)?.renderer = value.clone(),
            PresetPayload::SpeakerLayout(value) => {
                require_config(&mut config)?.speaker_layout = value.clone()
            }
            PresetPayload::Routing(value) => require_config(&mut config)?.routing = value.clone(),
            PresetPayload::Diagnostics(value) => {
                require_config(&mut config)?.diagnostics = value.clone()
            }
            PresetPayload::Simulation(value) => {
                require_config(&mut config)?.simulation = Some(value.clone())
            }
        }
    }
    ValidatedConfiguration::new(config.ok_or_else(|| {
        preset_error(
            ErrorCode::PresetConflict,
            "preset.payload",
            "materialization requires a full configuration base",
        )
    })?)
}

fn visit<'a>(
    id: &str,
    by_id: &BTreeMap<&str, &'a Preset>,
    stack: &mut Vec<String>,
    ordered: &mut Vec<&'a Preset>,
    depth: usize,
) -> Result<(), ConfigError> {
    if depth > MAX_COMPOSITION_DEPTH {
        return Err(preset_error(
            ErrorCode::PresetDepthExceeded,
            "presets[].extends",
            "preset composition exceeds the maximum depth",
        ));
    }
    if stack.iter().any(|value| value == id) {
        return Err(preset_error(
            ErrorCode::PresetCycle,
            "presets[].extends",
            "preset composition contains a cycle",
        ));
    }
    let preset = by_id.get(id).copied().ok_or_else(|| {
        preset_error(
            ErrorCode::PresetNotFound,
            "presets[].extends",
            "referenced preset does not exist",
        )
    })?;
    stack.push(id.to_owned());
    for reference in &preset.extends {
        visit(reference, by_id, stack, ordered, depth + 1)?;
    }
    stack.pop();
    ordered.push(preset);
    Ok(())
}

fn require_config(
    config: &mut Option<AuroraConfiguration>,
) -> Result<&mut AuroraConfiguration, ConfigError> {
    config.as_mut().ok_or_else(|| {
        preset_error(
            ErrorCode::PresetConflict,
            "preset.payload",
            "an overlay was applied before a full configuration base",
        )
    })
}

fn preset_error(code: ErrorCode, path: &str, detail: &str) -> ConfigError {
    let category = if matches!(
        code,
        ErrorCode::CollectionLimitExceeded
            | ErrorCode::SerializedSizeExceeded
            | ErrorCode::InvalidString
    ) {
        ErrorCategory::Bounds
    } else if matches!(
        code,
        ErrorCode::InvalidJson | ErrorCode::UnsupportedSchemaVersion
    ) {
        ErrorCategory::Schema
    } else {
        ErrorCategory::Preset
    };
    ConfigError::new(
        code,
        path,
        category,
        detail,
        Some("correct the preset graph and materialize again"),
    )
}
