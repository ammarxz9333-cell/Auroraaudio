use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    AudioFormatIntent, AuroraConfiguration, BufferingPolicy, ComponentReference, ConfigError,
    DeviceSelectionIntent, DiagnosticsPolicy, EngineConfiguration, ErrorCategory, ErrorCode,
    LayoutKind, RoutingConfiguration, SchemaMetadata, SimulationProfile, SpeakerConfiguration,
    SpeakerLayoutConfiguration, ValidatedConfiguration,
};

/// Native Aurora configuration schema that introduces elevation-aware layouts.
pub const CONFIGURATION_V4_SCHEMA_VERSION: u16 = 4;
/// Oldest reader that may consume native v4 documents without migration.
pub const CONFIGURATION_V4_MINIMUM_READER_VERSION: u16 = 4;

/// Layout vocabulary for Aurora Configuration v4.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutKindV4 {
    /// Canonical FL/FR layout.
    Stereo,
    /// Canonical FL/FR/FC/LFE/SL/SR layout.
    Surround51,
    /// Canonical FL/FR/FC/LFE/SL/SR/SBL/SBR layout.
    Surround71,
    /// Canonical FL/FR/FC/LFE/SL/SR/SBL/SBR/TFL/TFR/TRL/TRR layout.
    Surround714,
    /// Explicit irregular horizontal layout.
    CustomHorizontal,
}

/// Elevation-capable speaker-layout intent for Configuration v4.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeakerLayoutConfigurationV4 {
    /// Stable layout identifier.
    pub id: String,
    /// Layout vocabulary.
    pub kind: LayoutKindV4,
    /// Speakers normalized by stable identifier after validation.
    pub speakers: Vec<SpeakerConfiguration>,
    /// Explicitly activates elevation-aware geometry for a layout that supports it.
    #[serde(default)]
    pub elevation_rendering: bool,
}

/// Complete Aurora Configuration v4 control-plane document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuroraConfigurationV4 {
    /// Schema metadata. v4 documents require the exact 4/4 version pair.
    pub schema: SchemaMetadata,
    /// Engine intent.
    pub engine: EngineConfiguration,
    /// Audio format intent.
    pub audio_format: AudioFormatIntent,
    /// Optional input device intent.
    pub input_device: Option<DeviceSelectionIntent>,
    /// Optional output device intent.
    pub output_device: Option<DeviceSelectionIntent>,
    /// Explicit routing intent.
    pub routing: RoutingConfiguration,
    /// Elevation-capable speaker layout intent.
    pub speaker_layout: SpeakerLayoutConfigurationV4,
    /// Versioned renderer component reference.
    pub renderer: ComponentReference,
    /// Bounded buffering policy.
    pub buffering: BufferingPolicy,
    /// Diagnostics policy.
    pub diagnostics: DiagnosticsPolicy,
    /// Optional deterministic simulation profile.
    pub simulation: Option<SimulationProfile>,
}

/// Immutable normalized Configuration v4 that passed common and elevation validation.
#[derive(Clone, Debug, PartialEq)]
pub struct ValidatedConfigurationV4 {
    config: AuroraConfigurationV4,
}

impl ValidatedConfigurationV4 {
    /// Validates and normalizes one native v4 configuration.
    pub fn new(mut config: AuroraConfigurationV4) -> Result<Self, ConfigError> {
        validate_schema(&config)?;
        validate_elevation_layout(&config)?;

        // Reuse every established v3 invariant for common control-plane fields.
        // The shadow layout deliberately disables elevation and uses only v3
        // vocabulary; v4-only layout semantics are validated above.
        let shadow = ValidatedConfiguration::new(v3_shadow(&config))?;
        let normalized = shadow.config();
        config.engine = normalized.engine.clone();
        config.audio_format = normalized.audio_format.clone();
        config.input_device = normalized.input_device.clone();
        config.output_device = normalized.output_device.clone();
        config.routing = normalized.routing.clone();
        config.renderer = normalized.renderer.clone();
        config.buffering = normalized.buffering.clone();
        config.diagnostics = normalized.diagnostics.clone();
        config.simulation = normalized.simulation.clone();
        config
            .speaker_layout
            .speakers
            .sort_by(|left, right| left.id.cmp(&right.id));
        normalize_zero_elevations(&mut config.speaker_layout.speakers);
        Ok(Self { config })
    }

    /// Parses, validates, and normalizes bounded UTF-8 JSON.
    pub fn from_json(bytes: &[u8]) -> Result<Self, ConfigError> {
        if bytes.len() > crate::MAX_SERIALIZED_BYTES {
            return Err(v4_error(
                ErrorCode::SerializedSizeExceeded,
                "$",
                ErrorCategory::Bounds,
                "serialized Configuration v4 exceeds the byte limit",
            ));
        }
        let config = serde_json::from_slice(bytes).map_err(|_| {
            v4_error(
                ErrorCode::InvalidJson,
                "$",
                ErrorCategory::Schema,
                "configuration is not valid Configuration v4 JSON",
            )
        })?;
        Self::new(config)
    }

    /// Borrows the normalized v4 configuration.
    pub fn config(&self) -> &AuroraConfigurationV4 {
        &self.config
    }

    /// Returns deterministic compact canonical JSON.
    pub fn canonical_json(&self) -> Result<String, ConfigError> {
        let mut canonical = self.config.clone();
        canonical.schema.generated_by = None;
        serde_json::to_string(&canonical).map_err(|_| {
            v4_error(
                ErrorCode::InvalidJson,
                "$",
                ErrorCategory::Schema,
                "validated Configuration v4 could not be serialized",
            )
        })
    }

    /// Compares canonical v4 semantics, excluding generated-by provenance.
    pub fn semantically_eq(&self, other: &Self) -> bool {
        self.canonical_json().ok() == other.canonical_json().ok()
    }
}

/// Result of an explicit v3 -> v4 migration.
#[derive(Clone, Debug, PartialEq)]
pub struct MigrationResultV4 {
    /// Validated destination document.
    pub configuration: ValidatedConfigurationV4,
    /// Deterministic set of fields changed by migration.
    pub changed_fields: BTreeSet<String>,
}

/// Migrates one already-valid Aurora Configuration v3 document into v4.
///
/// Existing horizontal layouts retain their semantics. The migration does not
/// invent height speakers or turn elevation on; 7.1.4 must be requested by a
/// native v4 document explicitly.
pub fn migrate_v3_to_v4(bytes: &[u8]) -> Result<MigrationResultV4, ConfigError> {
    let source = ValidatedConfiguration::from_json(bytes)?;
    if source.config().schema.schema_version != 3
        || source.config().schema.minimum_reader_version != 3
    {
        return Err(v4_error(
            ErrorCode::UnsupportedMigration,
            "schema",
            ErrorCategory::Migration,
            "v3 to v4 migration requires an exact Configuration v3 source",
        ));
    }
    let source = source.config();
    let destination = AuroraConfigurationV4 {
        schema: SchemaMetadata {
            schema_name: source.schema.schema_name.clone(),
            schema_version: CONFIGURATION_V4_SCHEMA_VERSION,
            minimum_reader_version: CONFIGURATION_V4_MINIMUM_READER_VERSION,
            generated_by: source.schema.generated_by.clone(),
        },
        engine: source.engine.clone(),
        audio_format: source.audio_format.clone(),
        input_device: source.input_device.clone(),
        output_device: source.output_device.clone(),
        routing: source.routing.clone(),
        speaker_layout: SpeakerLayoutConfigurationV4 {
            id: source.speaker_layout.id.clone(),
            kind: match source.speaker_layout.kind {
                LayoutKind::Stereo => LayoutKindV4::Stereo,
                LayoutKind::Surround51 => LayoutKindV4::Surround51,
                LayoutKind::Surround71 => LayoutKindV4::Surround71,
                LayoutKind::CustomHorizontal => LayoutKindV4::CustomHorizontal,
            },
            speakers: source.speaker_layout.speakers.clone(),
            elevation_rendering: false,
        },
        renderer: source.renderer.clone(),
        buffering: source.buffering.clone(),
        diagnostics: source.diagnostics.clone(),
        simulation: source.simulation.clone(),
    };
    Ok(MigrationResultV4 {
        configuration: ValidatedConfigurationV4::new(destination)?,
        changed_fields: BTreeSet::from([
            "schema.schema_version".to_owned(),
            "schema.minimum_reader_version".to_owned(),
        ]),
    })
}

fn validate_schema(config: &AuroraConfigurationV4) -> Result<(), ConfigError> {
    if config.schema.schema_name != "aurora.configuration"
        || config.schema.schema_version != CONFIGURATION_V4_SCHEMA_VERSION
        || config.schema.minimum_reader_version != CONFIGURATION_V4_MINIMUM_READER_VERSION
    {
        return Err(v4_error(
            ErrorCode::UnsupportedSchemaVersion,
            "schema",
            ErrorCategory::Schema,
            "Configuration v4 requires schema_version=4 and minimum_reader_version=4",
        ));
    }
    Ok(())
}

fn validate_elevation_layout(config: &AuroraConfigurationV4) -> Result<(), ConfigError> {
    let layout = &config.speaker_layout;
    if layout.speakers.is_empty() || layout.speakers.len() > crate::MAX_SPEAKERS {
        return Err(v4_error(
            ErrorCode::CollectionLimitExceeded,
            "speaker_layout.speakers",
            ErrorCategory::Bounds,
            "speaker layout is empty or exceeds its published bound",
        ));
    }

    for speaker in &layout.speakers {
        if !speaker.azimuth_degrees.is_finite()
            || !(-180.0..=180.0).contains(&speaker.azimuth_degrees)
            || speaker
                .elevation_degrees
                .is_some_and(|value| !value.is_finite() || !(-90.0..=90.0).contains(&value))
        {
            return Err(v4_error(
                ErrorCode::InvalidNumericValue,
                "speaker_layout.speakers.geometry",
                ErrorCategory::Validation,
                "speaker azimuth/elevation is non-finite or outside its geometric bound",
            ));
        }
    }

    match layout.kind {
        LayoutKindV4::Surround714 => validate_surround_714(config),
        LayoutKindV4::Stereo
        | LayoutKindV4::Surround51
        | LayoutKindV4::Surround71
        | LayoutKindV4::CustomHorizontal => validate_horizontal_v4(config),
    }
}

fn validate_horizontal_v4(config: &AuroraConfigurationV4) -> Result<(), ConfigError> {
    if config.speaker_layout.elevation_rendering
        || config.speaker_layout.speakers.iter().any(|speaker| {
            speaker
                .elevation_degrees
                .is_some_and(|elevation| elevation.abs() > 1.0e-6)
        })
    {
        return Err(v4_error(
            ErrorCode::UnsupportedReservedField,
            "speaker_layout.elevation_rendering",
            ErrorCategory::Validation,
            "horizontal v4 layouts cannot activate nonzero elevation",
        ));
    }
    let expected = match config.speaker_layout.kind {
        LayoutKindV4::Stereo => &["FL", "FR"][..],
        LayoutKindV4::Surround51 => &["FL", "FR", "FC", "LFE", "SL", "SR"][..],
        LayoutKindV4::Surround71 => &["FL", "FR", "FC", "LFE", "SL", "SR", "SBL", "SBR"][..],
        LayoutKindV4::CustomHorizontal => &[][..],
        LayoutKindV4::Surround714 => unreachable!("7.1.4 has dedicated validation"),
    };
    validate_standard_roles(&config.speaker_layout.speakers, expected)
}

fn validate_surround_714(config: &AuroraConfigurationV4) -> Result<(), ConfigError> {
    if !config.speaker_layout.elevation_rendering {
        return Err(v4_error(
            ErrorCode::UnsupportedReservedField,
            "speaker_layout.elevation_rendering",
            ErrorCategory::Validation,
            "surround 7.1.4 requires elevation_rendering=true",
        ));
    }
    if config.audio_format.channel_count != 12 {
        return Err(v4_error(
            ErrorCode::InvalidRouting,
            "audio_format.channel_count",
            ErrorCategory::Routing,
            "surround 7.1.4 requires exactly 12 output channels",
        ));
    }
    const EXPECTED: &[&str] = &[
        "FL", "FR", "FC", "LFE", "SL", "SR", "SBL", "SBR", "TFL", "TFR", "TRL", "TRR",
    ];
    validate_standard_roles(&config.speaker_layout.speakers, EXPECTED)?;

    for speaker in config
        .speaker_layout
        .speakers
        .iter()
        .filter(|speaker| speaker.active)
    {
        let is_height = matches!(speaker.role.as_str(), "TFL" | "TFR" | "TRL" | "TRR");
        if is_height {
            if !speaker
                .elevation_degrees
                .is_some_and(|elevation| elevation > 0.0 && elevation <= 90.0)
            {
                return Err(v4_error(
                    ErrorCode::InvalidNumericValue,
                    "speaker_layout.speakers.elevation_degrees",
                    ErrorCategory::Validation,
                    "active 7.1.4 height speakers require positive elevation",
                ));
            }
        } else if speaker
            .elevation_degrees
            .is_some_and(|elevation| elevation.abs() > 1.0e-6)
        {
            return Err(v4_error(
                ErrorCode::InvalidNumericValue,
                "speaker_layout.speakers.elevation_degrees",
                ErrorCategory::Validation,
                "active 7.1.4 bed speakers must remain at zero elevation",
            ));
        }
    }
    Ok(())
}

fn validate_standard_roles(
    speakers: &[SpeakerConfiguration],
    expected: &[&str],
) -> Result<(), ConfigError> {
    if expected.is_empty() {
        return Ok(());
    }
    let active = speakers
        .iter()
        .filter(|speaker| speaker.active)
        .collect::<Vec<_>>();
    let roles = active
        .iter()
        .map(|speaker| speaker.role.as_str())
        .collect::<BTreeSet<_>>();
    if active.len() != expected.len()
        || roles.len() != expected.len()
        || expected.iter().any(|role| !roles.contains(role))
    {
        return Err(v4_error(
            ErrorCode::InvalidRouting,
            "speaker_layout.speakers.role",
            ErrorCategory::Routing,
            "standard v4 layout roles are missing or duplicated",
        ));
    }
    Ok(())
}

fn normalize_zero_elevations(speakers: &mut [SpeakerConfiguration]) {
    for speaker in speakers {
        if speaker
            .elevation_degrees
            .is_some_and(|elevation| elevation.abs() <= 1.0e-6)
        {
            speaker.elevation_degrees = None;
        }
    }
}

fn v3_shadow(config: &AuroraConfigurationV4) -> AuroraConfiguration {
    AuroraConfiguration {
        schema: SchemaMetadata {
            schema_name: "aurora.configuration".to_owned(),
            schema_version: crate::CURRENT_SCHEMA_VERSION,
            minimum_reader_version: crate::MINIMUM_READER_VERSION,
            generated_by: config.schema.generated_by.clone(),
        },
        engine: config.engine.clone(),
        audio_format: config.audio_format.clone(),
        input_device: config.input_device.clone(),
        output_device: config.output_device.clone(),
        routing: config.routing.clone(),
        speaker_layout: SpeakerLayoutConfiguration {
            id: config.speaker_layout.id.clone(),
            kind: match config.speaker_layout.kind {
                LayoutKindV4::Stereo => LayoutKind::Stereo,
                LayoutKindV4::Surround51 => LayoutKind::Surround51,
                LayoutKindV4::Surround71 => LayoutKind::Surround71,
                LayoutKindV4::Surround714 | LayoutKindV4::CustomHorizontal => {
                    LayoutKind::CustomHorizontal
                }
            },
            speakers: config
                .speaker_layout
                .speakers
                .iter()
                .cloned()
                .map(|mut speaker| {
                    speaker.elevation_degrees = None;
                    speaker
                })
                .collect(),
            elevation_rendering: false,
        },
        renderer: config.renderer.clone(),
        buffering: config.buffering.clone(),
        diagnostics: config.diagnostics.clone(),
        simulation: config.simulation.clone(),
    }
}

fn v4_error(code: ErrorCode, path: &str, category: ErrorCategory, detail: &str) -> ConfigError {
    ConfigError::new(
        code,
        path,
        category,
        detail,
        Some("correct the v4 field and validate again"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SURROUND_714: &[u8] = include_bytes!("../../../fixtures/config/surround-7-1-4-v4.json");
    const STEREO_V3: &[u8] = include_bytes!("../../../fixtures/config/stereo-basic-v3.json");

    #[test]
    fn canonical_7_1_4_validates_and_normalizes_zero_bed_elevation() {
        let validated = ValidatedConfigurationV4::from_json(SURROUND_714).unwrap();
        assert_eq!(validated.config().audio_format.channel_count, 12);
        assert!(validated.config().speaker_layout.elevation_rendering);
        assert_eq!(
            validated.config().speaker_layout.kind,
            LayoutKindV4::Surround714
        );
        assert!(validated
            .config()
            .speaker_layout
            .speakers
            .iter()
            .filter(|speaker| !matches!(speaker.role.as_str(), "TFL" | "TFR" | "TRL" | "TRR"))
            .all(|speaker| speaker.elevation_degrees.is_none()));
    }

    #[test]
    fn height_without_positive_elevation_fails_closed() {
        let mut config: AuroraConfigurationV4 = serde_json::from_slice(SURROUND_714).unwrap();
        config
            .speaker_layout
            .speakers
            .iter_mut()
            .find(|speaker| speaker.role == "TFL")
            .unwrap()
            .elevation_degrees = None;
        assert_eq!(
            ValidatedConfigurationV4::new(config).unwrap_err().code,
            ErrorCode::InvalidNumericValue
        );
    }

    #[test]
    fn missing_height_role_fails_closed() {
        let mut config: AuroraConfigurationV4 = serde_json::from_slice(SURROUND_714).unwrap();
        config
            .speaker_layout
            .speakers
            .iter_mut()
            .find(|speaker| speaker.role == "TRR")
            .unwrap()
            .active = false;
        assert_eq!(
            ValidatedConfigurationV4::new(config).unwrap_err().code,
            ErrorCode::InvalidRouting
        );
    }

    #[test]
    fn horizontal_layout_cannot_enable_elevation() {
        let migrated = migrate_v3_to_v4(STEREO_V3).unwrap();
        let mut config = migrated.configuration.config().clone();
        config.speaker_layout.elevation_rendering = true;
        assert_eq!(
            ValidatedConfigurationV4::new(config).unwrap_err().code,
            ErrorCode::UnsupportedReservedField
        );
    }

    #[test]
    fn surround_714_requires_twelve_channels() {
        let mut config: AuroraConfigurationV4 = serde_json::from_slice(SURROUND_714).unwrap();
        config.audio_format.channel_count = 8;
        assert_eq!(
            ValidatedConfigurationV4::new(config).unwrap_err().code,
            ErrorCode::InvalidRouting
        );
    }

    #[test]
    fn v3_migration_is_explicit_and_semantics_preserving() {
        let migrated = migrate_v3_to_v4(STEREO_V3).unwrap();
        assert_eq!(
            migrated.configuration.config().schema.schema_version,
            CONFIGURATION_V4_SCHEMA_VERSION
        );
        assert_eq!(
            migrated.configuration.config().speaker_layout.kind,
            LayoutKindV4::Stereo
        );
        assert_eq!(
            migrated.changed_fields,
            BTreeSet::from([
                "schema.minimum_reader_version".to_owned(),
                "schema.schema_version".to_owned(),
            ])
        );
    }
}
