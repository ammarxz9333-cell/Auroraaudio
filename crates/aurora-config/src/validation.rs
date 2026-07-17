use std::collections::BTreeSet;

use crate::{
    AmbiguityPolicy, AuroraConfiguration, ConfigError, ErrorCategory, ErrorCode,
    FormatFallbackPolicy, LayoutKind, RendererConfiguration, CURRENT_SCHEMA_VERSION, MAX_CHANNELS,
    MAX_FALLBACK_SAMPLE_RATES, MAX_ROUTES, MAX_SERIALIZED_BYTES, MAX_SPEAKERS, MAX_STRING_BYTES,
    MINIMUM_READER_VERSION,
};

/// Immutable normalized configuration that passed every validation phase.
#[derive(Clone, Debug, PartialEq)]
pub struct ValidatedConfiguration {
    config: AuroraConfiguration,
}

impl ValidatedConfiguration {
    /// Validates and normalizes an in-memory configuration.
    pub fn new(mut config: AuroraConfiguration) -> Result<Self, ConfigError> {
        validate_root(&config)?;
        normalize(&mut config);
        Ok(Self { config })
    }

    /// Parses, validates, and normalizes bounded UTF-8 JSON.
    pub fn from_json(bytes: &[u8]) -> Result<Self, ConfigError> {
        if bytes.len() > MAX_SERIALIZED_BYTES {
            return Err(error(
                ErrorCode::SerializedSizeExceeded,
                "$",
                ErrorCategory::Bounds,
                "serialized configuration exceeds the byte limit",
            ));
        }
        let config = serde_json::from_slice(bytes).map_err(|_| {
            error(
                ErrorCode::InvalidJson,
                "$",
                ErrorCategory::Schema,
                "configuration is not valid schema JSON",
            )
        })?;
        Self::new(config)
    }

    /// Borrows the normalized configuration.
    pub fn config(&self) -> &AuroraConfiguration {
        &self.config
    }

    /// Returns deterministic compact canonical JSON.
    pub fn canonical_json(&self) -> Result<String, ConfigError> {
        let mut canonical = self.config.clone();
        canonical.schema.generated_by = None;
        serde_json::to_string(&canonical).map_err(|_| {
            error(
                ErrorCode::InvalidJson,
                "$",
                ErrorCategory::Schema,
                "validated configuration could not be serialized",
            )
        })
    }

    /// Returns deterministic indented diagnostic JSON.
    pub fn human_readable(&self) -> Result<String, ConfigError> {
        let mut canonical = self.config.clone();
        canonical.schema.generated_by = None;
        serde_json::to_string_pretty(&canonical).map_err(|_| {
            error(
                ErrorCode::InvalidJson,
                "$",
                ErrorCategory::Schema,
                "validated configuration could not be formatted",
            )
        })
    }

    /// Compares canonical semantics, excluding generated-by provenance.
    pub fn semantically_eq(&self, other: &Self) -> bool {
        self.canonical_json().ok() == other.canonical_json().ok()
    }
}

fn validate_root(config: &AuroraConfiguration) -> Result<(), ConfigError> {
    if config.schema.schema_name != "aurora.configuration"
        || config.schema.schema_version != CURRENT_SCHEMA_VERSION
        || config.schema.minimum_reader_version != MINIMUM_READER_VERSION
    {
        return Err(error(
            ErrorCode::UnsupportedSchemaVersion,
            "schema",
            ErrorCategory::Schema,
            "unsupported configuration schema metadata",
        ));
    }
    strings(config)?;
    if !(8_000..=192_000).contains(&config.audio_format.sample_rate) {
        return Err(numeric("audio_format.sample_rate"));
    }
    if config.audio_format.channel_count == 0
        || usize::from(config.audio_format.channel_count) > MAX_CHANNELS
    {
        return Err(numeric("audio_format.channel_count"));
    }
    if !(1..=8_192).contains(&config.audio_format.callback_frames) {
        return Err(numeric("audio_format.callback_frames"));
    }
    if let FormatFallbackPolicy::AllowListedSampleRates { sample_rates } =
        &config.audio_format.fallback_policy
    {
        if sample_rates.is_empty() || sample_rates.len() > MAX_FALLBACK_SAMPLE_RATES {
            return Err(limit("audio_format.fallback_policy.sample_rates"));
        }
        if sample_rates
            .iter()
            .any(|rate| !(8_000..=192_000).contains(rate))
        {
            return Err(numeric("audio_format.fallback_policy.sample_rates"));
        }
    }
    validate_devices(config)?;
    validate_routing(config)?;
    validate_layout(config)?;
    validate_renderer(config)?;
    validate_buffering(config)?;
    validate_diagnostics(config)?;
    validate_simulation(config)?;
    Ok(())
}

fn strings(config: &AuroraConfiguration) -> Result<(), ConfigError> {
    let mut fields = vec![
        ("engine.id", config.engine.id.as_str()),
        (
            "engine.recovery_policy",
            config.engine.recovery_policy.as_str(),
        ),
        ("speaker_layout.id", config.speaker_layout.id.as_str()),
    ];
    if let Some(value) = config.schema.generated_by.as_deref() {
        fields.push(("schema.generated_by", value));
    }
    for (index, channel) in config.routing.inputs.iter().enumerate() {
        fields.push(("routing.inputs[].id", channel.id.as_str()));
        fields.push(("routing.inputs[].label", channel.label.as_str()));
        let _ = index;
    }
    for channel in &config.routing.outputs {
        fields.push(("routing.outputs[].id", channel.id.as_str()));
        fields.push(("routing.outputs[].label", channel.label.as_str()));
    }
    for speaker in &config.speaker_layout.speakers {
        fields.push(("speaker_layout.speakers[].id", speaker.id.as_str()));
        fields.push(("speaker_layout.speakers[].role", speaker.role.as_str()));
        fields.push(("speaker_layout.speakers[].label", speaker.label.as_str()));
    }
    for (path, value) in fields {
        if value.trim().is_empty() || value.len() > MAX_STRING_BYTES {
            return Err(error(
                ErrorCode::InvalidString,
                path,
                ErrorCategory::Validation,
                "string is empty or exceeds the UTF-8 byte limit",
            ));
        }
    }
    Ok(())
}

fn validate_devices(config: &AuroraConfiguration) -> Result<(), ConfigError> {
    for (path, device) in [
        ("input_device", config.input_device.as_ref()),
        ("output_device", config.output_device.as_ref()),
    ] {
        let Some(device) = device else { continue };
        if device.stable_id.as_deref().map_or(true, str::is_empty)
            && device.friendly_name.as_deref().map_or(true, str::is_empty)
        {
            return Err(error(
                ErrorCode::AmbiguousDeviceSelection,
                path,
                ErrorCategory::DeviceIntent,
                "device intent requires a stable ID or friendly name",
            ));
        }
        if device.ambiguity_policy == AmbiguityPolicy::AllowFirst
            || (device.ambiguity_policy == AmbiguityPolicy::RequireStableIdentifier
                && device.stable_id.is_none())
        {
            return Err(error(
                ErrorCode::AmbiguousDeviceSelection,
                path,
                ErrorCategory::DeviceIntent,
                "device intent permits ambiguity or lacks its required stable ID",
            ));
        }
        for value in [device.stable_id.as_deref(), device.friendly_name.as_deref()]
            .into_iter()
            .flatten()
        {
            if value.len() > MAX_STRING_BYTES {
                return Err(error(
                    ErrorCode::InvalidString,
                    path,
                    ErrorCategory::DeviceIntent,
                    "device selector exceeds the UTF-8 byte limit",
                ));
            }
        }
    }
    Ok(())
}

fn validate_routing(config: &AuroraConfiguration) -> Result<(), ConfigError> {
    let routing = &config.routing;
    if routing.inputs.len() > MAX_CHANNELS
        || routing.outputs.len() > MAX_CHANNELS
        || routing.routes.len() > MAX_ROUTES
    {
        return Err(limit("routing"));
    }
    if routing.outputs.len() != usize::from(config.audio_format.channel_count) {
        return Err(error(
            ErrorCode::InvalidRouting,
            "routing.outputs",
            ErrorCategory::Routing,
            "output identity count must equal the audio channel count",
        ));
    }
    let inputs = unique_ids(
        routing.inputs.iter().map(|value| value.id.as_str()),
        "routing.inputs",
    )?;
    let outputs = unique_ids(
        routing.outputs.iter().map(|value| value.id.as_str()),
        "routing.outputs",
    )?;
    let inactive = unique_ids(
        routing.inactive_outputs.iter().map(String::as_str),
        "routing.inactive_outputs",
    )?;
    let mut assigned_inputs = BTreeSet::new();
    let mut assigned_outputs = BTreeSet::new();
    for route in &routing.routes {
        if !inputs.contains(route.input.as_str()) || !outputs.contains(route.output.as_str()) {
            return Err(routing_error("route references an unknown channel"));
        }
        if inactive.contains(route.output.as_str())
            || !assigned_inputs.insert(route.input.as_str())
            || !assigned_outputs.insert(route.output.as_str())
        {
            return Err(routing_error(
                "route is duplicate or targets an inactive output",
            ));
        }
    }
    if outputs
        .iter()
        .any(|output| !inactive.contains(output) && !assigned_outputs.contains(output))
    {
        return Err(routing_error("an active output has no assignment"));
    }
    Ok(())
}

fn validate_layout(config: &AuroraConfiguration) -> Result<(), ConfigError> {
    let layout = &config.speaker_layout;
    if layout.speakers.is_empty() || layout.speakers.len() > MAX_SPEAKERS {
        return Err(limit("speaker_layout.speakers"));
    }
    if layout.elevation_rendering {
        return Err(error(
            ErrorCode::UnsupportedReservedField,
            "speaker_layout.elevation_rendering",
            ErrorCategory::Validation,
            "elevation rendering is reserved and unsupported",
        ));
    }
    unique_ids(
        layout.speakers.iter().map(|value| value.id.as_str()),
        "speaker_layout.speakers",
    )?;
    for speaker in &layout.speakers {
        if !speaker.azimuth_degrees.is_finite()
            || !(-180.0..=180.0).contains(&speaker.azimuth_degrees)
            || speaker
                .elevation_degrees
                .is_some_and(|value| !value.is_finite() || !(-90.0..=90.0).contains(&value))
        {
            return Err(numeric("speaker_layout.speakers.geometry"));
        }
    }
    let active_roles = layout
        .speakers
        .iter()
        .filter(|speaker| speaker.active)
        .map(|speaker| speaker.role.as_str())
        .collect::<BTreeSet<_>>();
    let expected: &[&str] = match layout.kind {
        LayoutKind::Stereo => &["FL", "FR"],
        LayoutKind::Surround51 => &["FL", "FR", "FC", "LFE", "SL", "SR"],
        LayoutKind::Surround71 => &["FL", "FR", "FC", "LFE", "SL", "SR", "SBL", "SBR"],
        LayoutKind::CustomHorizontal => &[],
    };
    if !expected.is_empty()
        && (active_roles.len() != expected.len()
            || expected.iter().any(|role| !active_roles.contains(role)))
    {
        return Err(error(
            ErrorCode::InvalidRouting,
            "speaker_layout.speakers.role",
            ErrorCategory::Routing,
            "standard layout roles are missing or duplicated",
        ));
    }
    Ok(())
}

fn validate_renderer(config: &AuroraConfiguration) -> Result<(), ConfigError> {
    let active = config
        .speaker_layout
        .speakers
        .iter()
        .filter(|speaker| speaker.active)
        .count();
    match config.renderer {
        RendererConfiguration::Basic => Ok(()),
        RendererConfiguration::PointSourceVbap if active >= 2 => Ok(()),
        RendererConfiguration::HorizontalSpread { spread }
            if active >= 2 && spread.is_finite() && (0.0..=1.0).contains(&spread) =>
        {
            Ok(())
        }
        _ => Err(error(
            ErrorCode::UnsupportedRenderer,
            "renderer",
            ErrorCategory::Renderer,
            "renderer is invalid or incompatible with the active layout",
        )),
    }
}

fn validate_buffering(config: &AuroraConfiguration) -> Result<(), ConfigError> {
    let policy = &config.buffering;
    if policy.minimum_fill_frames > policy.target_fill_frames
        || policy.target_fill_frames > policy.maximum_fill_frames
        || policy.maximum_fill_frames > policy.ring_capacity_frames
        || policy.ring_capacity_frames == 0
        || !policy.require_preallocated_storage
    {
        return Err(error(
            ErrorCode::InvalidBufferBounds,
            "buffering",
            ErrorCategory::Validation,
            "buffer fill bounds or preallocation policy are invalid",
        ));
    }
    Ok(())
}

fn validate_diagnostics(config: &AuroraConfiguration) -> Result<(), ConfigError> {
    if config.diagnostics.retention_events == 0
        || config.diagnostics.retention_events > 65_536
        || config.diagnostics.allowed_truth_sources.is_empty()
    {
        return Err(limit("diagnostics"));
    }
    Ok(())
}

fn validate_simulation(config: &AuroraConfiguration) -> Result<(), ConfigError> {
    let Some(profile) = &config.simulation else {
        return Ok(());
    };
    if !profile.drift_ppm.is_finite() || !(-500.0..=500.0).contains(&profile.drift_ppm) {
        return Err(numeric("simulation.drift_ppm"));
    }
    for (path, value) in [
        (
            "simulation.scenario_family",
            profile.scenario_family.as_str(),
        ),
        ("simulation.fault_profile", profile.fault_profile.as_str()),
        ("simulation.replay_id", profile.replay_id.as_str()),
    ] {
        if value.trim().is_empty() || value.len() > MAX_STRING_BYTES {
            return Err(error(
                ErrorCode::InvalidString,
                path,
                ErrorCategory::Validation,
                "simulation identifier is empty or oversized",
            ));
        }
    }
    Ok(())
}

fn normalize(config: &mut AuroraConfiguration) {
    config.routing.inputs.sort_by(|a, b| a.id.cmp(&b.id));
    config.routing.outputs.sort_by(|a, b| a.id.cmp(&b.id));
    config
        .routing
        .routes
        .sort_by(|a, b| (&a.output, &a.input).cmp(&(&b.output, &b.input)));
    config.routing.inactive_outputs.sort();
    config
        .speaker_layout
        .speakers
        .sort_by(|a, b| a.id.cmp(&b.id));
    if let FormatFallbackPolicy::AllowListedSampleRates { sample_rates } =
        &mut config.audio_format.fallback_policy
    {
        sample_rates.sort_unstable();
        sample_rates.dedup();
    }
}

fn unique_ids<'a>(
    values: impl Iterator<Item = &'a str>,
    path: &str,
) -> Result<BTreeSet<&'a str>, ConfigError> {
    let mut set = BTreeSet::new();
    for value in values {
        if !set.insert(value) {
            return Err(error(
                ErrorCode::DuplicateIdentifier,
                path,
                ErrorCategory::Validation,
                "stable identifier occurs more than once",
            ));
        }
    }
    Ok(set)
}

fn error(code: ErrorCode, path: &str, category: ErrorCategory, detail: &str) -> ConfigError {
    ConfigError::new(
        code,
        path,
        category,
        detail,
        Some("correct the field and validate again"),
    )
}

fn numeric(path: &str) -> ConfigError {
    error(
        ErrorCode::InvalidNumericValue,
        path,
        ErrorCategory::Validation,
        "numeric value is non-finite, zero, unsupported, or outside its bound",
    )
}

fn limit(path: &str) -> ConfigError {
    error(
        ErrorCode::CollectionLimitExceeded,
        path,
        ErrorCategory::Bounds,
        "collection is empty where required or exceeds its published bound",
    )
}

fn routing_error(detail: &str) -> ConfigError {
    error(
        ErrorCode::InvalidRouting,
        "routing.routes",
        ErrorCategory::Routing,
        detail,
    )
}
