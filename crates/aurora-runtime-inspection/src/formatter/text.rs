//! Deterministic human-readable formatting for inspection-owned reports.

use crate::{
    DeferredCapacityProjection, DspProjection, InspectionError, InspectionFactSemantics,
    InspectionFinding, InspectionOutputFormat, InspectionReport, RedactionState,
    RendererKindProjection, SetupStageProjection,
};

use super::{validate_report, MAX_TEXT_BYTES};

/// Stateless deterministic text formatter for inspection-owned reports.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TextFormatter;

impl TextFormatter {
    /// Formats one report with fixed sections, labels, indentation, and order.
    ///
    /// Values are never localized, terminal-wrapped, or ANSI styled. Sensitive
    /// strings retain the report's projection-time redaction state.
    ///
    /// # Errors
    ///
    /// Returns a structured [`InspectionError`] when report structure, floats,
    /// collection counts, string escaping, or the published text byte limit are
    /// invalid.
    pub fn format(report: &InspectionReport) -> Result<String, InspectionError> {
        validate_report(report)?;
        let mut output = TextOutput::default();
        output.line("Aurora Runtime Plan Inspection")?;
        output.line(&format!(
            "inspection_schema_version: {}",
            report.inspection_schema_version()
        ))?;
        output.line(&format!(
            "source_plan_schema_version: {}",
            report.source_plan_schema_version()
        ))?;
        output.line(&format!(
            "redaction_state: {}",
            redaction(report.redaction())
        ))?;
        output.line(&format!(
            "represented_semantics: {}",
            report
                .represented_semantics()
                .iter()
                .map(|value| semantics(*value))
                .collect::<Vec<_>>()
                .join(", ")
        ))?;
        output.line("evidence_disclaimer: no negotiated, observed, simulated, measured, runtime-ready, latency, or physical evidence")?;

        let runtime = report.runtime();
        let format = &runtime.requested_audio_format;
        output.line("")?;
        output.line("[requested]")?;
        output.line(&format!(
            "audio.sample_rate_hz: {}",
            format.requested_sample_rate
        ))?;
        output.line(&format!(
            "audio.sample_format: {}",
            quote(&format.requested_sample_format)?
        ))?;
        output.line(&format!(
            "audio.input_channels: {}",
            format.requested_input_channels
        ))?;
        output.line(&format!(
            "audio.output_channels: {}",
            format.requested_output_channels
        ))?;
        output.line(&format!(
            "audio.callback_frames: {}",
            format.requested_callback_frames
        ))?;
        output.line(&format!(
            "audio.fallback_policy: {}",
            quote(&format.requested_fallback_policy)?
        ))?;
        write_device(
            &mut output,
            "input_device",
            runtime.requested_devices.input.as_ref(),
        )?;
        write_device(
            &mut output,
            "output_device",
            runtime.requested_devices.output.as_ref(),
        )?;

        output.line("")?;
        output.line("[prepared]")?;
        output.line(&format!(
            "metadata.configuration_schema_version: {}",
            runtime.metadata.configuration_schema_version
        ))?;
        output.line(&format!(
            "metadata.contract_version: {}",
            runtime.metadata.contract_version
        ))?;
        output.line(&format!(
            "metadata.evidence: {}",
            quote(&runtime.metadata.evidence)?
        ))?;
        output.line(&format!(
            "renderer.kind: {}",
            renderer(runtime.prepared_renderer.kind)
        ))?;
        output.line(&format!(
            "renderer.horizontal_spread: {}",
            option_f32(runtime.prepared_renderer.horizontal_spread)?
        ))?;
        output.line(&format!("dsp.state: {}", dsp(runtime.prepared_dsp)))?;
        output.line(&format!(
            "topology.layout_kind: {}",
            quote(&runtime.prepared_topology.layout_kind)?
        ))?;
        write_topology(&mut output, report)?;
        write_capacities(&mut output, report)?;

        output.line("")?;
        output.line("[deferred]")?;
        for capacity in &runtime.capacities.deferred_until_setup {
            output.line(&format!("- {}", deferred(*capacity)))?;
        }

        output.line("")?;
        output.line("[setup]")?;
        output.line(&format!(
            "requested_input_backend: {}",
            option_string(report.setup().requested_input_backend.as_deref())?
        ))?;
        output.line(&format!(
            "requested_output_backend: {}",
            option_string(report.setup().requested_output_backend.as_deref())?
        ))?;
        output.line("stages:")?;
        for stage_value in &report.setup().stages {
            output.line(&format!("  - {}", stage(*stage_value)))?;
        }
        output.line("dependencies:")?;
        for dependency in &report.setup().dependencies {
            output.line(&format!(
                "  - {} <- {}",
                stage(dependency.stage),
                stage(dependency.depends_on)
            ))?;
        }
        output.line(&format!(
            "descriptive_plan_complete: {}",
            report.setup().descriptive_plan_complete
        ))?;

        output.line("")?;
        output.line("[findings]")?;
        for finding_value in report.findings() {
            output.line(&format!("- {}", finding(*finding_value)))?;
        }
        Ok(output.finish())
    }
}

#[derive(Default)]
struct TextOutput {
    value: String,
}

impl TextOutput {
    fn line(&mut self, value: &str) -> Result<(), InspectionError> {
        let actual = self
            .value
            .len()
            .checked_add(value.len())
            .and_then(|size| size.checked_add(1))
            .ok_or(InspectionError::SerializedOutputTooLarge {
                format: InspectionOutputFormat::Text,
                actual: usize::MAX,
                maximum: MAX_TEXT_BYTES,
            })?;
        if actual > MAX_TEXT_BYTES {
            return Err(InspectionError::SerializedOutputTooLarge {
                format: InspectionOutputFormat::Text,
                actual,
                maximum: MAX_TEXT_BYTES,
            });
        }
        self.value.push_str(value);
        self.value.push('\n');
        Ok(())
    }
    fn finish(self) -> String {
        self.value
    }
}

fn write_device(
    output: &mut TextOutput,
    label: &str,
    device: Option<&crate::DeviceSelectorProjection>,
) -> Result<(), InspectionError> {
    match device {
        Some(device) => {
            output.line(&format!("{label}.present: true"))?;
            output.line(&format!(
                "{label}.stable_id: {}",
                option_string(device.stable_id.as_deref())?
            ))?;
            output.line(&format!(
                "{label}.friendly_name: {}",
                option_string(device.friendly_name.as_deref())?
            ))?;
            output.line(&format!(
                "{label}.backend: {}",
                quote(&device.requested_backend)?
            ))?;
            output.line(&format!(
                "{label}.ambiguity_policy: {}",
                quote(&device.ambiguity_policy)?
            ))
        }
        None => output.line(&format!("{label}.present: false")),
    }
}

fn write_topology(
    output: &mut TextOutput,
    report: &InspectionReport,
) -> Result<(), InspectionError> {
    let topology = &report.runtime().prepared_topology;
    output.line("topology.inputs:")?;
    for value in &topology.inputs {
        output.line(&format!(
            "  - id={} label={}",
            quote(&value.id)?,
            quote(&value.label)?
        ))?;
    }
    output.line("topology.outputs:")?;
    for value in &topology.outputs {
        output.line(&format!(
            "  - id={} label={}",
            quote(&value.id)?,
            quote(&value.label)?
        ))?;
    }
    output.line("topology.routes:")?;
    for value in &topology.routes {
        output.line(&format!(
            "  - input_id={} output_id={}",
            quote(&value.input_id)?,
            quote(&value.output_id)?
        ))?;
    }
    output.line("topology.inactive_output_ids:")?;
    for value in &topology.inactive_output_ids {
        output.line(&format!("  - {}", quote(value)?))?;
    }
    output.line("topology.speakers:")?;
    for value in &topology.speakers {
        output.line(&format!(
            "  - id={} label={} role={} position=[{},{},{}] active={}",
            quote(&value.id)?,
            quote(&value.label)?,
            quote(&value.channel_role)?,
            stable_f32(value.normalized_x)?,
            stable_f32(value.normalized_y)?,
            stable_f32(value.normalized_z)?,
            value.active
        ))?;
    }
    Ok(())
}

fn write_capacities(
    output: &mut TextOutput,
    report: &InspectionReport,
) -> Result<(), InspectionError> {
    let value = &report.runtime().capacities;
    output.line(&format!(
        "capacities.input_channels: {}",
        value.input_channels
    ))?;
    output.line(&format!(
        "capacities.output_channels: {}",
        value.output_channels
    ))?;
    output.line(&format!("capacities.routes: {}", value.routes))?;
    output.line(&format!("capacities.speakers: {}", value.speakers))?;
    output.line(&format!(
        "capacities.callback_frames: {}",
        value.callback_frames
    ))?;
    output.line(&format!(
        "capacities.renderer_output_gain_width: {}",
        value.renderer_output_gain_width
    ))
}

fn quote(value: &str) -> Result<String, InspectionError> {
    if value.len() > MAX_TEXT_BYTES {
        return Err(InspectionError::SerializedOutputTooLarge {
            format: InspectionOutputFormat::Text,
            actual: value.len(),
            maximum: MAX_TEXT_BYTES,
        });
    }
    serde_json::to_string(value).map_err(|_| InspectionError::JsonSerializationFailed)
}
fn option_string(value: Option<&str>) -> Result<String, InspectionError> {
    value
        .map(quote)
        .transpose()
        .map(|value| value.unwrap_or_else(|| "absent".to_owned()))
}
fn stable_f32(value: f32) -> Result<String, InspectionError> {
    serde_json::to_string(&value).map_err(|_| InspectionError::JsonSerializationFailed)
}
fn option_f32(value: Option<f32>) -> Result<String, InspectionError> {
    value
        .map(stable_f32)
        .transpose()
        .map(|value| value.unwrap_or_else(|| "absent".to_owned()))
}
fn redaction(value: RedactionState) -> &'static str {
    match value {
        RedactionState::Redacted => "redacted",
        RedactionState::UnredactedLocal => "unredacted_local",
    }
}
fn semantics(value: InspectionFactSemantics) -> &'static str {
    match value {
        InspectionFactSemantics::Requested => "requested",
        InspectionFactSemantics::Prepared => "prepared",
        InspectionFactSemantics::Deferred => "deferred",
        InspectionFactSemantics::Negotiated => "negotiated",
        InspectionFactSemantics::Observed => "observed",
        InspectionFactSemantics::Simulated => "simulated",
        InspectionFactSemantics::Measured => "measured",
    }
}
fn renderer(value: RendererKindProjection) -> &'static str {
    match value {
        RendererKindProjection::BasicInverseDistance => "basic_inverse_distance",
        RendererKindProjection::PointSourceHorizontalVbap => "point_source_horizontal_vbap",
        RendererKindProjection::HorizontalSpreadVbap => "horizontal_spread_vbap",
    }
}
fn dsp(value: DspProjection) -> &'static str {
    match value {
        DspProjection::None => "none",
        DspProjection::DeferredByCurrentSchema => "deferred_by_current_schema",
    }
}
fn deferred(value: DeferredCapacityProjection) -> &'static str {
    match value {
        DeferredCapacityProjection::RendererScratch => "renderer_scratch",
        DeferredCapacityProjection::RendererHistory => "renderer_history",
        DeferredCapacityProjection::ImplementationTemporary => "implementation_temporary",
        DeferredCapacityProjection::DelayProcessor => "delay_processor",
        DeferredCapacityProjection::Asrc => "asrc",
        DeferredCapacityProjection::BackendRing => "backend_ring",
    }
}
fn stage(value: SetupStageProjection) -> &'static str {
    match value {
        SetupStageProjection::DeviceSelectionIntent => "device_selection_intent",
        SetupStageProjection::RequestedFormatPlanning => "requested_format_planning",
        SetupStageProjection::RendererPreparationIntent => "renderer_preparation_intent",
        SetupStageProjection::DspPreparationIntent => "dsp_preparation_intent",
        SetupStageProjection::BackendPreparationIntent => "backend_preparation_intent",
        SetupStageProjection::SetupPlanComplete => "setup_plan_complete",
    }
}
fn finding(value: InspectionFinding) -> &'static str {
    match value {
        InspectionFinding::ControlPlaneIntentOnly => "control_plane_intent_only",
        InspectionFinding::SetupCapacitiesDeferred => "setup_capacities_deferred",
        InspectionFinding::SensitiveIdentifiersRedacted => "sensitive_identifiers_redacted",
        InspectionFinding::CanonicalSetupDependencyOrderRepresented => {
            "canonical_setup_dependency_order_represented"
        }
        InspectionFinding::RequestedDeviceSelectionUnresolved => {
            "requested_device_selection_unresolved"
        }
        InspectionFinding::InactiveOutputsPresent => "inactive_outputs_present",
        InspectionFinding::DspAbsent => "dsp_absent",
        InspectionFinding::DspSetupDeferredByCurrentSchema => {
            "dsp_setup_deferred_by_current_schema"
        }
        InspectionFinding::CustomSpeakerRolesPresent => "custom_speaker_roles_present",
        InspectionFinding::SetupPlanCompletionDescriptiveOnly => {
            "setup_plan_completion_descriptive_only"
        }
    }
}
