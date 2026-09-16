//! Versioned, bounded, inspection-owned projection model.

use aurora_runtime_assembly::{
    PreparedDeviceSelectorIntent, PreparedDspPlan, PreparedLayoutKind, PreparedRendererKind,
    PreparedRuntimePlan, PreparedSetupPlan, SetupDependency, SetupStage,
};
use serde::Serialize;

use crate::{InspectionError, InspectionLimit, InspectionOptions, PlanRelationship};

/// Current inspection projection schema version.
pub const INSPECTION_SCHEMA_VERSION: u16 = 4;
/// Maximum retained deterministic findings.
pub const MAX_FINDINGS: usize = 32;
/// Maximum bytes in one source string.
pub const MAX_STRING_BYTES: usize = 256;
/// Maximum cumulative bytes copied from source strings.
pub const MAX_TOTAL_STRING_BYTES: usize = 32 * 1024;
/// Maximum identities in either channel direction.
pub const MAX_CHANNELS: usize = 32;
/// Maximum projected routes.
pub const MAX_ROUTES: usize = 64;
/// Maximum projected speakers.
pub const MAX_SPEAKERS: usize = 32;
/// Maximum projected setup stages.
pub const MAX_SETUP_STAGES: usize = 6;
/// Maximum projected setup dependencies.
pub const MAX_SETUP_DEPENDENCIES: usize = 9;

/// Whether sensitive requested identifiers were retained in a report.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RedactionState {
    /// Deterministic category markers replace sensitive values.
    Redacted,
    /// An explicit local option retained source values.
    UnredactedLocal,
}

/// Semantic state assigned to an inspection fact.
///
/// Schema 1 reports only [`Self::Requested`], [`Self::Prepared`], and
/// [`Self::Deferred`]. The remaining variants make category distinctions
/// explicit for callers and must not be inferred by projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionFactSemantics {
    /// Requested, unresolved intent.
    Requested,
    /// Deterministically prepared control-plane fact.
    Prepared,
    /// Explicitly deferred setup fact.
    Deferred,
    /// Negotiated host state, which schema 1 never reports.
    Negotiated,
    /// Observed runtime or host state, which schema 1 never reports.
    Observed,
    /// Simulated evidence, which schema 1 never reports.
    Simulated,
    /// Physically measured evidence, which schema 1 never reports.
    Measured,
}

/// One deterministic finding about represented inspection facts.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionFinding {
    /// The report contains control-plane intent, not negotiated or observed state.
    ControlPlaneIntentOnly,
    /// Setup-derived capacities remain explicitly deferred.
    SetupCapacitiesDeferred,
    /// Sensitive identifiers were replaced while presence and structure remain.
    SensitiveIdentifiersRedacted,
    /// Canonical setup dependency order is represented without reordering.
    CanonicalSetupDependencyOrderRepresented,
    /// Requested device selectors remain unresolved intent.
    RequestedDeviceSelectionUnresolved,
    /// One or more intentionally inactive output identities are represented.
    InactiveOutputsPresent,
    /// The accepted prepared plan explicitly contains no DSP graph.
    DspAbsent,
    /// DSP setup is explicitly deferred by the current accepted schema.
    DspSetupDeferredByCurrentSchema,
    /// One or more custom speaker roles are represented.
    CustomSpeakerRolesPresent,
    /// `SetupPlanComplete` describes plan completeness, not runtime readiness.
    SetupPlanCompletionDescriptiveOnly,
}

/// Immutable inspection-owned report projected from paired prepared plans.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct InspectionReport {
    inspection_schema_version: u16,
    source_plan_schema_version: u16,
    runtime: RuntimePlanProjection,
    setup: SetupPlanProjection,
    findings: Vec<InspectionFinding>,
    redaction: RedactionState,
    represented_semantics: [InspectionFactSemantics; 3],
}

impl InspectionReport {
    /// Projects accepted prepared-plan accessors into an owned inspection report.
    ///
    /// Source vectors and fixed setup vectors retain their accepted canonical
    /// order. Projection validates all published bounds before returning and
    /// never silently truncates semantic entries.
    ///
    /// # Errors
    ///
    /// Returns [`InspectionError`] when paired plans disagree, a finite bound is
    /// exceeded, or checked cumulative string accounting overflows.
    pub fn project(
        runtime: &PreparedRuntimePlan,
        setup: &PreparedSetupPlan,
        options: InspectionOptions,
    ) -> Result<Self, InspectionError> {
        validate_plan_pair(runtime, setup)?;
        let mut context = ProjectionContext::new(options);
        let runtime = context.runtime(runtime)?;
        let setup = context.setup(setup)?;
        let redaction = if options.reveals_local_identifiers() {
            RedactionState::UnredactedLocal
        } else {
            RedactionState::Redacted
        };
        let findings = conformance_findings(&runtime, &setup, redaction);
        check_limit(InspectionLimit::Findings, findings.len(), MAX_FINDINGS)?;
        Ok(Self {
            inspection_schema_version: INSPECTION_SCHEMA_VERSION,
            source_plan_schema_version: runtime.metadata.contract_version,
            runtime,
            setup,
            findings,
            redaction,
            represented_semantics: [
                InspectionFactSemantics::Requested,
                InspectionFactSemantics::Prepared,
                InspectionFactSemantics::Deferred,
            ],
        })
    }

    /// Returns the inspection-owned schema version.
    pub fn inspection_schema_version(&self) -> u16 {
        self.inspection_schema_version
    }
    /// Returns the accepted runtime-plan contract version projected by the report.
    pub fn source_plan_schema_version(&self) -> u16 {
        self.source_plan_schema_version
    }
    /// Returns projected prepared runtime facts.
    pub fn runtime(&self) -> &RuntimePlanProjection {
        &self.runtime
    }
    /// Returns projected prepared setup facts.
    pub fn setup(&self) -> &SetupPlanProjection {
        &self.setup
    }
    /// Returns findings in fixed deterministic order.
    pub fn findings(&self) -> &[InspectionFinding] {
        &self.findings
    }
    /// Returns whether identifiers were redacted.
    pub fn redaction(&self) -> RedactionState {
        self.redaction
    }
    /// Returns the fact semantics represented by schema 1.
    pub fn represented_semantics(&self) -> &[InspectionFactSemantics] {
        &self.represented_semantics
    }
}

/// Projected prepared runtime facts.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RuntimePlanProjection {
    /// Prepared-plan metadata.
    pub metadata: MetadataProjection,
    /// Requested audio-format intent.
    pub requested_audio_format: AudioFormatProjection,
    /// Prepared renderer intent.
    pub prepared_renderer: RendererProjection,
    /// Prepared DSP graph state from configuration intent.
    pub prepared_dsp: DspProjection,
    /// Prepared renderer and realtime-delay implementation identities.
    pub prepared_components: RealtimeComponentProjection,
    /// Prepared routing and layout facts.
    pub prepared_topology: TopologyProjection,
    /// Unresolved requested device intent.
    pub requested_devices: DeviceIntentProjection,
    /// Known and deferred capacity facts.
    pub capacities: CapacityProjection,
}

/// Runtime-plan provenance projected without timestamps or host evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MetadataProjection {
    /// Source configuration schema version.
    pub configuration_schema_version: u16,
    /// Accepted runtime-plan contract version.
    pub contract_version: u16,
    /// Stable prepared-plan evidence category.
    pub evidence: String,
}

/// Requested, unnegotiated audio-format facts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AudioFormatProjection {
    /// Requested sample rate in hertz.
    pub requested_sample_rate: u32,
    /// Requested sample representation.
    pub requested_sample_format: String,
    /// Requested input channel count.
    pub requested_input_channels: usize,
    /// Requested output channel count.
    pub requested_output_channels: usize,
    /// Requested callback frame count.
    pub requested_callback_frames: u32,
    /// Explicit requested fallback policy.
    pub requested_fallback_policy: String,
}

/// Prepared renderer selection without a renderer instance.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RendererProjection {
    /// Prepared renderer family.
    pub kind: RendererKindProjection,
    /// Prepared normalized horizontal spread, when applicable.
    pub horizontal_spread: Option<f32>,
}

/// Inspection-owned renderer family vocabulary.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RendererKindProjection {
    /// Basic inverse-distance intent.
    BasicInverseDistance,
    /// Point-source horizontal VBAP intent.
    PointSourceHorizontalVbap,
    /// Horizontal spread VBAP intent.
    HorizontalSpreadVbap,
    /// External object-to-PCM renderer intent.
    ExternalObjectPcm,
}

/// One prepared component implementation identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ComponentIdentityProjection {
    /// Stable implementation identifier selected by runtime assembly.
    pub implementation_id: String,
    /// Exact selected implementation version.
    pub implementation_version: String,
    /// Aurora contract major version.
    pub contract_major: u16,
    /// Aurora contract minor version.
    pub contract_minor: u16,
}

/// Prepared realtime component selections.
///
/// These are deterministic control-plane selections, not observed or activated runtime state.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RealtimeComponentProjection {
    /// Prepared renderer implementation identity.
    pub renderer: ComponentIdentityProjection,
    /// Prepared realtime delay implementation identity.
    pub realtime_delay: ComponentIdentityProjection,
}

/// Prepared DSP state supported by the accepted schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DspProjection {
    /// No DSP graph was requested.
    None,
    /// DSP setup remains deferred by the current schema.
    DeferredByCurrentSchema,
}

/// Prepared canonical routing and speaker layout.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TopologyProjection {
    /// Inputs in accepted canonical vector order.
    pub inputs: Vec<ChannelProjection>,
    /// Outputs in accepted canonical vector order.
    pub outputs: Vec<ChannelProjection>,
    /// Routes in accepted canonical vector order.
    pub routes: Vec<RouteProjection>,
    /// Inactive output identities in accepted canonical vector order.
    pub inactive_output_ids: Vec<String>,
    /// Prepared layout classification.
    pub layout_kind: String,
    /// Speakers in accepted canonical vector order.
    pub speakers: Vec<SpeakerProjection>,
}

/// Projected logical channel identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ChannelProjection {
    /// Stable or deterministically redacted identity.
    pub id: String,
    /// Local or deterministically redacted label.
    pub label: String,
}

/// Projected input-to-output route.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RouteProjection {
    /// Referenced projected input identity.
    pub input_id: String,
    /// Referenced projected output identity.
    pub output_id: String,
}

/// Projected speaker descriptor.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SpeakerProjection {
    /// Stable or deterministically redacted speaker identity.
    pub id: String,
    /// Local or deterministically redacted label.
    pub label: String,
    /// Canonical role, or a deterministic marker for a custom role when redacted.
    pub channel_role: String,
    /// Normalized dimensionless x direction.
    pub normalized_x: f32,
    /// Normalized dimensionless y direction.
    pub normalized_y: f32,
    /// Normalized dimensionless z direction retained from prepared intent.
    pub normalized_z: f32,
    /// Prepared participation intent; this is not observed availability.
    pub active: bool,
}

/// Unresolved input and output device-selection intent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DeviceIntentProjection {
    /// Requested input selector, if present.
    pub input: Option<DeviceSelectorProjection>,
    /// Requested output selector, if present.
    pub output: Option<DeviceSelectorProjection>,
}

/// One unresolved requested selector.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DeviceSelectorProjection {
    /// Present stable identifier, retained or replaced by a category marker.
    pub stable_id: Option<String>,
    /// Present friendly name, retained or replaced by a category marker.
    pub friendly_name: Option<String>,
    /// Prepared backend component identity selected from the requested reference.
    pub backend_component: ComponentIdentityProjection,
    /// Direction-specific Aurora backend contract kind.
    pub backend_contract_kind: String,
    /// Backend component configuration schema.
    pub backend_configuration_schema: u16,
    /// Requested ambiguity policy.
    pub ambiguity_policy: String,
}

/// Known and explicitly setup-deferred capacity facts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CapacityProjection {
    /// Plan-known input count.
    pub input_channels: usize,
    /// Plan-known output count.
    pub output_channels: usize,
    /// Plan-known route count.
    pub routes: usize,
    /// Plan-known speaker descriptor count.
    pub speakers: usize,
    /// Plan-known callback frame count.
    pub callback_frames: usize,
    /// Plan-known renderer output gain width.
    pub renderer_output_gain_width: usize,
    /// Explicitly deferred setup capacity names in fixed order.
    pub deferred_until_setup: Vec<DeferredCapacityProjection>,
}

/// A capacity whose value is absent until later setup.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeferredCapacityProjection {
    /// Renderer scratch.
    RendererScratch,
    /// Renderer history.
    RendererHistory,
    /// Implementation temporary storage.
    ImplementationTemporary,
    /// Delay processor storage.
    DelayProcessor,
    /// ASRC storage.
    Asrc,
    /// Backend ring storage.
    BackendRing,
}

/// Projected descriptive setup-plan facts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SetupPlanProjection {
    /// Setup stages in accepted fixed order.
    pub stages: Vec<SetupStageProjection>,
    /// Setup dependencies in accepted fixed order.
    pub dependencies: Vec<SetupDependencyProjection>,
    /// Requested input backend, if present.
    pub requested_input_backend: Option<String>,
    /// Requested output backend, if present.
    pub requested_output_backend: Option<String>,
    /// Whether the descriptive `SetupPlanComplete` stage is present.
    pub descriptive_plan_complete: bool,
}

/// Inspection-owned setup stage vocabulary.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStageProjection {
    /// Device-selection intent.
    DeviceSelectionIntent,
    /// Requested-format planning.
    RequestedFormatPlanning,
    /// Renderer preparation intent.
    RendererPreparationIntent,
    /// DSP preparation intent.
    DspPreparationIntent,
    /// Backend preparation intent.
    BackendPreparationIntent,
    /// Descriptive setup-plan completeness only.
    SetupPlanComplete,
}

/// One projected fixed setup dependency.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct SetupDependencyProjection {
    /// Dependent stage.
    pub stage: SetupStageProjection,
    /// Required predecessor.
    pub depends_on: SetupStageProjection,
}

fn conformance_findings(
    runtime: &RuntimePlanProjection,
    setup: &SetupPlanProjection,
    redaction: RedactionState,
) -> Vec<InspectionFinding> {
    let mut findings = vec![
        InspectionFinding::ControlPlaneIntentOnly,
        InspectionFinding::SetupCapacitiesDeferred,
    ];
    if redaction == RedactionState::Redacted {
        findings.push(InspectionFinding::SensitiveIdentifiersRedacted);
    }
    findings.push(InspectionFinding::CanonicalSetupDependencyOrderRepresented);
    if runtime.requested_devices.input.is_some() || runtime.requested_devices.output.is_some() {
        findings.push(InspectionFinding::RequestedDeviceSelectionUnresolved);
    }
    if !runtime.prepared_topology.inactive_output_ids.is_empty() {
        findings.push(InspectionFinding::InactiveOutputsPresent);
    }
    findings.push(match runtime.prepared_dsp {
        DspProjection::None => InspectionFinding::DspAbsent,
        DspProjection::DeferredByCurrentSchema => {
            InspectionFinding::DspSetupDeferredByCurrentSchema
        }
    });
    if runtime
        .prepared_topology
        .speakers
        .iter()
        .any(|speaker| !is_standard_channel_role(&speaker.channel_role))
    {
        findings.push(InspectionFinding::CustomSpeakerRolesPresent);
    }
    if setup.descriptive_plan_complete {
        findings.push(InspectionFinding::SetupPlanCompletionDescriptiveOnly);
    }
    findings
}

fn is_standard_channel_role(role: &str) -> bool {
    matches!(
        role,
        "front-left"
            | "front-right"
            | "front-center"
            | "low-frequency-effects"
            | "surround-left"
            | "surround-right"
            | "surround-back-left"
            | "surround-back-right"
            | "top-front-left"
            | "top-front-right"
            | "top-rear-left"
            | "top-rear-right"
    )
}

struct ProjectionContext {
    reveal: bool,
    string_bytes: usize,
}

impl ProjectionContext {
    fn new(options: InspectionOptions) -> Self {
        Self {
            reveal: options.reveals_local_identifiers(),
            string_bytes: 0,
        }
    }

    fn source_string(&mut self, value: &str) -> Result<String, InspectionError> {
        check_limit(InspectionLimit::StringBytes, value.len(), MAX_STRING_BYTES)?;
        self.string_bytes = self
            .string_bytes
            .checked_add(value.len())
            .ok_or(InspectionError::StringAccountingOverflow)?;
        check_limit(
            InspectionLimit::TotalStringBytes,
            self.string_bytes,
            MAX_TOTAL_STRING_BYTES,
        )?;
        Ok(value.to_owned())
    }

    fn sensitive(&mut self, value: &str, marker: String) -> Result<String, InspectionError> {
        let source = self.source_string(value)?;
        Ok(if self.reveal { source } else { marker })
    }

    fn runtime(
        &mut self,
        plan: &PreparedRuntimePlan,
    ) -> Result<RuntimePlanProjection, InspectionError> {
        let routing = plan.topology().routing();
        check_limit(
            InspectionLimit::Channels,
            routing.inputs().len(),
            MAX_CHANNELS,
        )?;
        check_limit(
            InspectionLimit::Channels,
            routing.outputs().len(),
            MAX_CHANNELS,
        )?;
        check_limit(InspectionLimit::Routes, routing.routes().len(), MAX_ROUTES)?;
        check_limit(
            InspectionLimit::Speakers,
            plan.topology().layout().speakers().len(),
            MAX_SPEAKERS,
        )?;

        let inputs = routing
            .inputs()
            .iter()
            .enumerate()
            .map(|(index, channel)| {
                Ok(ChannelProjection {
                    id: self.sensitive(channel.id(), marker("input-channel-id", index))?,
                    label: self.sensitive(channel.label(), marker("input-channel-label", index))?,
                })
            })
            .collect::<Result<Vec<_>, InspectionError>>()?;
        let outputs = routing
            .outputs()
            .iter()
            .enumerate()
            .map(|(index, channel)| {
                Ok(ChannelProjection {
                    id: self.sensitive(channel.id(), marker("output-channel-id", index))?,
                    label: self
                        .sensitive(channel.label(), marker("output-channel-label", index))?,
                })
            })
            .collect::<Result<Vec<_>, InspectionError>>()?;

        let routes = routing
            .routes()
            .iter()
            .map(|route| {
                Ok(RouteProjection {
                    input_id: map_reference(route.input_id(), routing.inputs(), &inputs)?,
                    output_id: map_reference(route.output_id(), routing.outputs(), &outputs)?,
                })
            })
            .collect::<Result<Vec<_>, InspectionError>>()?;
        let inactive_output_ids = routing
            .inactive_outputs()
            .iter()
            .map(|id| map_reference(id, routing.outputs(), &outputs))
            .collect::<Result<Vec<_>, _>>()?;

        let speakers = plan
            .topology()
            .layout()
            .speakers()
            .iter()
            .enumerate()
            .map(|(index, speaker)| {
                let position = speaker.position();
                let role = if self.reveal || speaker.channel_role().wav_channel_mask_bit().is_some()
                {
                    self.source_string(speaker.channel_role().as_str())?
                } else {
                    marker("custom-channel-role", index)
                };
                Ok(SpeakerProjection {
                    id: self.sensitive(speaker.id(), marker("speaker-id", index))?,
                    label: self.sensitive(speaker.label(), marker("speaker-label", index))?,
                    channel_role: role,
                    normalized_x: position.x,
                    normalized_y: position.y,
                    normalized_z: position.z,
                    active: speaker.is_active(),
                })
            })
            .collect::<Result<Vec<_>, InspectionError>>()?;

        let format = plan.execution().audio_format();
        let capacity = plan.capacity();
        let components = plan.execution().realtime_components();
        let renderer_component = components.renderer();
        let delay_component = components.realtime_delay();
        Ok(RuntimePlanProjection {
            metadata: MetadataProjection {
                configuration_schema_version: plan.metadata().configuration_schema_version(),
                contract_version: plan.metadata().contract_version(),
                evidence: debug_value(plan.metadata().evidence()),
            },
            requested_audio_format: AudioFormatProjection {
                requested_sample_rate: format.sample_rate(),
                requested_sample_format: debug_value(format.sample_format()),
                requested_input_channels: format.input_channel_count(),
                requested_output_channels: format.output_channel_count(),
                requested_callback_frames: format.callback_frames(),
                requested_fallback_policy: debug_value(format.fallback_policy()),
            },
            prepared_renderer: RendererProjection {
                kind: renderer_kind(plan.execution().renderer().kind()),
                horizontal_spread: plan.execution().renderer().horizontal_spread(),
            },
            prepared_dsp: dsp(plan.execution().dsp()),
            prepared_components: RealtimeComponentProjection {
                renderer: ComponentIdentityProjection {
                    implementation_id: self
                        .source_string(renderer_component.implementation_id())?,
                    implementation_version: self
                        .source_string(renderer_component.implementation_version())?,
                    contract_major: renderer_component.contract_major(),
                    contract_minor: renderer_component.contract_minor(),
                },
                realtime_delay: ComponentIdentityProjection {
                    implementation_id: self.source_string(delay_component.implementation_id())?,
                    implementation_version: self
                        .source_string(delay_component.implementation_version())?,
                    contract_major: delay_component.contract_major(),
                    contract_minor: delay_component.contract_minor(),
                },
            },
            prepared_topology: TopologyProjection {
                inputs,
                outputs,
                routes,
                inactive_output_ids,
                layout_kind: layout_kind(plan.topology().layout().kind()),
                speakers,
            },
            requested_devices: DeviceIntentProjection {
                input: self.device(plan.device_intent().input(), "input")?,
                output: self.device(plan.device_intent().output(), "output")?,
            },
            capacities: CapacityProjection {
                input_channels: capacity.input_channel_count(),
                output_channels: capacity.output_channel_count(),
                routes: capacity.route_count(),
                speakers: capacity.speaker_count(),
                callback_frames: capacity.callback_frame_count(),
                renderer_output_gain_width: capacity.renderer_output_gain_width(),
                deferred_until_setup: vec![
                    DeferredCapacityProjection::RendererScratch,
                    DeferredCapacityProjection::RendererHistory,
                    DeferredCapacityProjection::ImplementationTemporary,
                    DeferredCapacityProjection::DelayProcessor,
                    DeferredCapacityProjection::Asrc,
                    DeferredCapacityProjection::BackendRing,
                ],
            },
        })
    }

    fn device(
        &mut self,
        selector: Option<&PreparedDeviceSelectorIntent>,
        direction: &str,
    ) -> Result<Option<DeviceSelectorProjection>, InspectionError> {
        selector
            .map(|selector| {
                Ok(DeviceSelectorProjection {
                    stable_id: selector
                        .stable_id()
                        .map(|value| {
                            self.sensitive(
                                value,
                                format!("<redacted:{direction}-device-stable-id>"),
                            )
                        })
                        .transpose()?,
                    friendly_name: selector
                        .friendly_name()
                        .map(|value| {
                            self.sensitive(
                                value,
                                format!("<redacted:{direction}-device-friendly-name>"),
                            )
                        })
                        .transpose()?,
                    backend_component: {
                        let identity = selector.backend().identity();
                        ComponentIdentityProjection {
                            implementation_id: self.source_string(identity.implementation_id())?,
                            implementation_version: self
                                .source_string(identity.implementation_version())?,
                            contract_major: identity.contract_major(),
                            contract_minor: identity.contract_minor(),
                        }
                    },
                    backend_contract_kind: debug_value(selector.backend().contract_kind()),
                    backend_configuration_schema: selector.backend().configuration_schema(),
                    ambiguity_policy: debug_value(selector.ambiguity_policy()),
                })
            })
            .transpose()
    }

    fn setup(&mut self, plan: &PreparedSetupPlan) -> Result<SetupPlanProjection, InspectionError> {
        check_limit(
            InspectionLimit::SetupStages,
            plan.stages().len(),
            MAX_SETUP_STAGES,
        )?;
        check_limit(
            InspectionLimit::SetupDependencies,
            plan.dependencies().len(),
            MAX_SETUP_DEPENDENCIES,
        )?;
        let stages = plan.stages().iter().copied().map(stage).collect::<Vec<_>>();
        let dependencies = plan
            .dependencies()
            .iter()
            .copied()
            .map(dependency)
            .collect();
        Ok(SetupPlanProjection {
            descriptive_plan_complete: stages.contains(&SetupStageProjection::SetupPlanComplete),
            stages,
            dependencies,
            requested_input_backend: plan
                .backend()
                .requested_input_backend()
                .map(|backend| backend.identity().implementation_id().to_owned()),
            requested_output_backend: plan
                .backend()
                .requested_output_backend()
                .map(|backend| backend.identity().implementation_id().to_owned()),
        })
    }
}

fn validate_plan_pair(
    runtime: &PreparedRuntimePlan,
    setup: &PreparedSetupPlan,
) -> Result<(), InspectionError> {
    let checks = [
        (
            runtime.device_intent() == setup.device_intent(),
            PlanRelationship::DeviceIntent,
        ),
        (
            runtime.execution().audio_format() == setup.requested_audio_format(),
            PlanRelationship::RequestedAudioFormat,
        ),
        (
            runtime.execution().renderer() == setup.renderer().renderer(),
            PlanRelationship::RendererIntent,
        ),
        (
            runtime.topology() == setup.renderer().topology(),
            PlanRelationship::TopologyIntent,
        ),
        (
            runtime.execution().dsp() == setup.dsp().state(),
            PlanRelationship::DspIntent,
        ),
    ];
    checks
        .into_iter()
        .find_map(|(matches, relationship)| {
            (!matches).then_some(InspectionError::SourcePlansMismatch { relationship })
        })
        .map_or(Ok(()), Err)
}

fn check_limit(
    limit: InspectionLimit,
    actual: usize,
    maximum: usize,
) -> Result<(), InspectionError> {
    if actual > maximum {
        Err(InspectionError::LimitExceeded {
            limit,
            actual,
            maximum,
        })
    } else {
        Ok(())
    }
}

fn marker(category: &str, index: usize) -> String {
    format!("<redacted:{category}:{index}>")
}
fn debug_value(value: impl core::fmt::Debug) -> String {
    format!("{value:?}")
}
fn map_reference(
    source_id: &str,
    source: &[aurora_runtime_assembly::PreparedChannelIdentity],
    projected: &[ChannelProjection],
) -> Result<String, InspectionError> {
    source
        .iter()
        .position(|channel| channel.id() == source_id)
        .map(|index| projected[index].id.clone())
        .ok_or(InspectionError::SourcePlansMismatch {
            relationship: PlanRelationship::TopologyIntent,
        })
}
fn renderer_kind(value: PreparedRendererKind) -> RendererKindProjection {
    match value {
        PreparedRendererKind::BasicInverseDistance => RendererKindProjection::BasicInverseDistance,
        PreparedRendererKind::PointSourceHorizontalVbap => {
            RendererKindProjection::PointSourceHorizontalVbap
        }
        PreparedRendererKind::HorizontalSpreadVbap => RendererKindProjection::HorizontalSpreadVbap,
        PreparedRendererKind::ExternalObjectPcm => RendererKindProjection::ExternalObjectPcm,
    }
}
fn dsp(value: PreparedDspPlan) -> DspProjection {
    match value {
        PreparedDspPlan::None => DspProjection::None,
        PreparedDspPlan::DeferredByCurrentSchema => DspProjection::DeferredByCurrentSchema,
    }
}
fn layout_kind(value: PreparedLayoutKind) -> String {
    match value {
        PreparedLayoutKind::Standard(layout) => format!("Standard({layout:?})"),
        PreparedLayoutKind::CustomHorizontal => "CustomHorizontal".to_owned(),
    }
}
fn stage(value: SetupStage) -> SetupStageProjection {
    match value {
        SetupStage::DeviceSelectionIntent => SetupStageProjection::DeviceSelectionIntent,
        SetupStage::RequestedFormatPlanning => SetupStageProjection::RequestedFormatPlanning,
        SetupStage::RendererPreparationIntent => SetupStageProjection::RendererPreparationIntent,
        SetupStage::DspPreparationIntent => SetupStageProjection::DspPreparationIntent,
        SetupStage::BackendPreparationIntent => SetupStageProjection::BackendPreparationIntent,
        SetupStage::SetupPlanComplete => SetupStageProjection::SetupPlanComplete,
    }
}
fn dependency(value: SetupDependency) -> SetupDependencyProjection {
    SetupDependencyProjection {
        stage: stage(value.stage()),
        depends_on: stage(value.depends_on()),
    }
}

#[cfg(test)]
mod tests;
