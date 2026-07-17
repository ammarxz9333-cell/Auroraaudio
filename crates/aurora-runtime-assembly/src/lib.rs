//! Immutable control-plane contracts and deterministic runtime-plan derivation.
//!
//! Checkpoint B derives passive Aurora-owned values from an already validated
//! configuration. This remains control-plane code with no I/O, hardware access,
//! device probing, renderer or DSP construction, real-time engine, callback
//! use, or physical claim.

#![forbid(unsafe_code)]

use std::{collections::BTreeSet, error::Error, fmt};

use aurora_config::{AmbiguityPolicy, BackendIntent, FormatFallbackPolicy, SampleFormatIntent};
use aurora_core::{ChannelRole, StandardLayout, Vector3};

mod derivation;

pub use derivation::prepare_runtime_plan;

/// Version of the runtime-plan contract defined by this crate.
pub const RUNTIME_PLAN_CONTRACT_VERSION: u16 = 1;

/// Immutable aggregate of normalized intent prepared for a future setup step.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedRuntimePlan {
    metadata: RuntimePlanMetadata,
    execution: PreparedExecutionPlan,
    topology: PreparedTopologyPlan,
    device_intent: PreparedDeviceIntent,
    capacity: RuntimeCapacityPlan,
}

impl PreparedRuntimePlan {
    /// Creates a passive plan from already prepared contract components.
    pub fn new(
        metadata: RuntimePlanMetadata,
        execution: PreparedExecutionPlan,
        topology: PreparedTopologyPlan,
        device_intent: PreparedDeviceIntent,
        capacity: RuntimeCapacityPlan,
    ) -> Result<Self, RuntimePreparationError> {
        if execution.audio_format().input_channel_count() != capacity.input_channel_count()
            || execution.audio_format().output_channel_count() != capacity.output_channel_count()
            || topology.routing().inputs().len() != capacity.input_channel_count()
            || topology.routing().outputs().len() != capacity.output_channel_count()
            || topology.routing().routes().len() != capacity.route_count()
            || topology.layout().speakers().len() != capacity.speaker_count()
            || usize::try_from(execution.audio_format().callback_frames()).ok()
                != Some(capacity.callback_frame_count())
        {
            return Err(RuntimePreparationError::InternalInvariantViolation {
                invariant: RuntimeInvariant::ComponentCapacityMismatch,
            });
        }

        Ok(Self {
            metadata,
            execution,
            topology,
            device_intent,
            capacity,
        })
    }

    /// Returns plan provenance and contract-version metadata.
    pub fn metadata(&self) -> &RuntimePlanMetadata {
        &self.metadata
    }

    /// Returns the prepared execution intent.
    pub fn execution(&self) -> &PreparedExecutionPlan {
        &self.execution
    }

    /// Returns the prepared topology intent.
    pub fn topology(&self) -> &PreparedTopologyPlan {
        &self.topology
    }

    /// Returns unresolved input and output device intent.
    pub fn device_intent(&self) -> &PreparedDeviceIntent {
        &self.device_intent
    }

    /// Returns plan-known and setup-deferred capacity information.
    pub fn capacity(&self) -> &RuntimeCapacityPlan {
        &self.capacity
    }
}

/// Execution-related components grouped for safe top-level construction.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedExecutionPlan {
    audio_format: PreparedAudioFormatIntent,
    renderer: PreparedRendererPlan,
    dsp: PreparedDspPlan,
}

impl PreparedExecutionPlan {
    /// Groups format, renderer, and honest DSP intent without constructing them.
    pub fn new(
        audio_format: PreparedAudioFormatIntent,
        renderer: PreparedRendererPlan,
        dsp: PreparedDspPlan,
    ) -> Self {
        Self {
            audio_format,
            renderer,
            dsp,
        }
    }

    /// Returns the requested, unnegotiated audio format.
    pub fn audio_format(&self) -> &PreparedAudioFormatIntent {
        &self.audio_format
    }

    /// Returns the renderer descriptor.
    pub fn renderer(&self) -> &PreparedRendererPlan {
        &self.renderer
    }

    /// Returns the explicitly absent or deferred DSP state.
    pub fn dsp(&self) -> PreparedDspPlan {
        self.dsp
    }
}

/// Routing and speaker-layout components grouped for top-level construction.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedTopologyPlan {
    routing: PreparedRoutingPlan,
    layout: PreparedLayoutPlan,
}

impl PreparedTopologyPlan {
    /// Groups prepared routing and layout intent without performing mapping.
    pub fn new(routing: PreparedRoutingPlan, layout: PreparedLayoutPlan) -> Self {
        Self { routing, layout }
    }

    /// Returns the routing descriptor.
    pub fn routing(&self) -> &PreparedRoutingPlan {
        &self.routing
    }

    /// Returns the speaker-layout descriptor.
    pub fn layout(&self) -> &PreparedLayoutPlan {
        &self.layout
    }
}

/// Requested audio format before backend negotiation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedAudioFormatIntent {
    sample_rate: u32,
    sample_format: SampleFormatIntent,
    input_channel_count: usize,
    output_channel_count: usize,
    callback_frames: u32,
    fallback_policy: FormatFallbackPolicy,
}

impl PreparedAudioFormatIntent {
    /// Creates a nonzero format intent without negotiating a backend format.
    pub fn new(
        sample_rate: u32,
        sample_format: SampleFormatIntent,
        input_channel_count: usize,
        output_channel_count: usize,
        callback_frames: u32,
        fallback_policy: FormatFallbackPolicy,
    ) -> Result<Self, RuntimePreparationError> {
        for (field, value) in [
            (CapacityField::SampleRate, sample_rate as usize),
            (CapacityField::OutputChannelCount, output_channel_count),
            (CapacityField::CallbackFrameCount, callback_frames as usize),
        ] {
            if value == 0 {
                return Err(RuntimePreparationError::InvalidCapacity { field });
            }
        }

        Ok(Self {
            sample_rate,
            sample_format,
            input_channel_count,
            output_channel_count,
            callback_frames,
            fallback_policy,
        })
    }

    /// Returns the requested sample rate in hertz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Returns the requested PCM sample representation.
    pub fn sample_format(&self) -> SampleFormatIntent {
        self.sample_format
    }

    /// Returns the plan-known input identity count, which may be zero.
    pub fn input_channel_count(&self) -> usize {
        self.input_channel_count
    }

    /// Returns the plan-known output channel count.
    pub fn output_channel_count(&self) -> usize {
        self.output_channel_count
    }

    /// Returns the requested callback frame count.
    pub fn callback_frames(&self) -> u32 {
        self.callback_frames
    }

    /// Returns the explicit format fallback policy.
    pub fn fallback_policy(&self) -> &FormatFallbackPolicy {
        &self.fallback_policy
    }
}

/// Immutable routing descriptor preserving normalized configuration order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedRoutingPlan {
    inputs: Vec<PreparedChannelIdentity>,
    outputs: Vec<PreparedChannelIdentity>,
    routes: Vec<PreparedRoute>,
    inactive_outputs: Vec<String>,
}

impl PreparedRoutingPlan {
    /// Creates a self-consistent routing descriptor without sorting or mapping it.
    pub fn new(
        inputs: Vec<PreparedChannelIdentity>,
        outputs: Vec<PreparedChannelIdentity>,
        routes: Vec<PreparedRoute>,
        inactive_outputs: Vec<String>,
    ) -> Result<Self, RuntimePreparationError> {
        let input_ids = unique_identity_ids(&inputs, RoutingIssue::DuplicateInputIdentity)?;
        let output_ids = unique_identity_ids(&outputs, RoutingIssue::DuplicateOutputIdentity)?;
        let inactive_ids = unique_strings(
            &inactive_outputs,
            RoutingIssue::DuplicateInactiveOutputIdentity,
        )?;

        if routes
            .iter()
            .any(|route| !input_ids.contains(route.input_id()))
        {
            return Err(RuntimePreparationError::IncompatibleRoutingIntent {
                issue: RoutingIssue::UnknownInputReference,
            });
        }
        if routes
            .iter()
            .any(|route| !output_ids.contains(route.output_id()))
        {
            return Err(RuntimePreparationError::IncompatibleRoutingIntent {
                issue: RoutingIssue::UnknownOutputReference,
            });
        }
        if inactive_ids.iter().any(|id| !output_ids.contains(id)) {
            return Err(RuntimePreparationError::IncompatibleRoutingIntent {
                issue: RoutingIssue::UnknownInactiveOutputReference,
            });
        }

        Ok(Self {
            inputs,
            outputs,
            routes,
            inactive_outputs,
        })
    }

    /// Returns input identities in their supplied normalized order.
    pub fn inputs(&self) -> &[PreparedChannelIdentity] {
        &self.inputs
    }

    /// Returns output identities in their supplied normalized order.
    pub fn outputs(&self) -> &[PreparedChannelIdentity] {
        &self.outputs
    }

    /// Returns routes in their supplied normalized order.
    pub fn routes(&self) -> &[PreparedRoute] {
        &self.routes
    }

    /// Returns intentionally inactive output IDs in supplied normalized order.
    pub fn inactive_outputs(&self) -> &[String] {
        &self.inactive_outputs
    }
}

/// Stable channel identity stored by a prepared routing plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedChannelIdentity {
    id: String,
    label: String,
}

impl PreparedChannelIdentity {
    /// Creates a nonempty channel identity.
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
    ) -> Result<Self, RuntimePreparationError> {
        let id = nonempty(id.into(), RuntimeInvariant::EmptyChannelIdentifier)?;
        let label = nonempty(label.into(), RuntimeInvariant::EmptyChannelLabel)?;
        Ok(Self { id, label })
    }

    /// Returns the stable identity.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the human-readable label.
    pub fn label(&self) -> &str {
        &self.label
    }
}

/// One explicit input-to-output assignment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedRoute {
    input_id: String,
    output_id: String,
}

impl PreparedRoute {
    /// Creates a route between two nonempty logical channel identifiers.
    pub fn new(
        input_id: impl Into<String>,
        output_id: impl Into<String>,
    ) -> Result<Self, RuntimePreparationError> {
        Ok(Self {
            input_id: nonempty(input_id.into(), RuntimeInvariant::EmptyRouteInput)?,
            output_id: nonempty(output_id.into(), RuntimeInvariant::EmptyRouteOutput)?,
        })
    }

    /// Returns the referenced input identity.
    pub fn input_id(&self) -> &str {
        &self.input_id
    }

    /// Returns the referenced output identity.
    pub fn output_id(&self) -> &str {
        &self.output_id
    }
}

/// Immutable speaker-layout descriptor.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedLayoutPlan {
    kind: PreparedLayoutKind,
    speakers: Vec<PreparedSpeaker>,
}

impl PreparedLayoutPlan {
    /// Creates a nonempty layout without sorting speakers or deriving geometry.
    pub fn new(
        kind: PreparedLayoutKind,
        speakers: Vec<PreparedSpeaker>,
    ) -> Result<Self, RuntimePreparationError> {
        if speakers.is_empty() {
            return Err(RuntimePreparationError::InvalidCapacity {
                field: CapacityField::SpeakerCount,
            });
        }
        let mut ids = BTreeSet::new();
        if speakers.iter().any(|speaker| !ids.insert(speaker.id())) {
            return Err(RuntimePreparationError::InternalInvariantViolation {
                invariant: RuntimeInvariant::DuplicateSpeakerIdentifier,
            });
        }
        Ok(Self { kind, speakers })
    }

    /// Returns the standard or custom-horizontal layout classification.
    pub fn kind(&self) -> PreparedLayoutKind {
        self.kind
    }

    /// Returns speakers in their supplied normalized order.
    pub fn speakers(&self) -> &[PreparedSpeaker] {
        &self.speakers
    }
}

/// Layout classification understood by the Checkpoint A contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreparedLayoutKind {
    /// A canonical Aurora standard layout.
    Standard(StandardLayout),
    /// An explicitly configured custom horizontal layout.
    CustomHorizontal,
}

/// Immutable speaker identity, role, participation, and normalized position.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedSpeaker {
    id: String,
    label: String,
    channel_role: ChannelRole,
    position: Vector3,
    active: bool,
}

impl PreparedSpeaker {
    /// Creates a speaker descriptor with finite geometry and nonempty identity.
    ///
    /// `active` means participation in the intended layout. It does not claim
    /// that a physical speaker is present or available.
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        channel_role: ChannelRole,
        position: Vector3,
        active: bool,
    ) -> Result<Self, RuntimePreparationError> {
        if !position.x.is_finite() || !position.y.is_finite() || !position.z.is_finite() {
            return Err(RuntimePreparationError::InternalInvariantViolation {
                invariant: RuntimeInvariant::NonFiniteSpeakerGeometry,
            });
        }
        Ok(Self {
            id: nonempty(id.into(), RuntimeInvariant::EmptySpeakerIdentifier)?,
            label: nonempty(label.into(), RuntimeInvariant::EmptySpeakerLabel)?,
            channel_role,
            position,
            active,
        })
    }

    /// Returns the stable speaker identifier.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the human-readable speaker label.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns the canonical or custom Aurora channel role.
    pub fn channel_role(&self) -> &ChannelRole {
        &self.channel_role
    }

    /// Returns the normalized, dimensionless speaker direction.
    ///
    /// No radius, distance, room coordinate, or physical presence is implied.
    pub fn position(&self) -> Vector3 {
        self.position
    }

    /// Returns whether the speaker participates in the intended layout.
    ///
    /// This is configuration intent, not observed physical availability.
    pub fn is_active(&self) -> bool {
        self.active
    }
}

/// Renderer family selected by a prepared descriptor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreparedRendererKind {
    /// Existing inverse-distance basic renderer.
    BasicInverseDistance,
    /// Existing point-source horizontal VBAP renderer.
    PointSourceHorizontalVbap,
    /// Existing horizontal spread VBAP renderer.
    HorizontalSpreadVbap,
}

/// Passive renderer selection with no renderer instance or backend handle.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedRendererPlan {
    kind: PreparedRendererKind,
    horizontal_spread: Option<f32>,
}

impl PreparedRendererPlan {
    /// Describes the existing inverse-distance renderer.
    pub const fn basic_inverse_distance() -> Self {
        Self {
            kind: PreparedRendererKind::BasicInverseDistance,
            horizontal_spread: None,
        }
    }

    /// Describes the existing point-source horizontal VBAP renderer.
    pub const fn point_source_horizontal_vbap() -> Self {
        Self {
            kind: PreparedRendererKind::PointSourceHorizontalVbap,
            horizontal_spread: None,
        }
    }

    /// Describes horizontal spread VBAP with a normalized spread value.
    pub fn horizontal_spread_vbap(spread: f32) -> Result<Self, RuntimePreparationError> {
        if !spread.is_finite() || !(0.0..=1.0).contains(&spread) {
            return Err(RuntimePreparationError::InternalInvariantViolation {
                invariant: RuntimeInvariant::InvalidHorizontalSpread,
            });
        }
        Ok(Self {
            kind: PreparedRendererKind::HorizontalSpreadVbap,
            horizontal_spread: Some(spread),
        })
    }

    /// Returns the selected renderer family.
    pub fn kind(&self) -> PreparedRendererKind {
        self.kind
    }

    /// Returns spread only when horizontal spread VBAP is selected.
    pub fn horizontal_spread(&self) -> Option<f32> {
        self.horizontal_spread
    }
}

/// Honest DSP state available from the current configuration schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreparedDspPlan {
    /// No DSP graph was requested.
    None,
    /// DSP assembly is deferred because the accepted schema is incomplete.
    DeferredByCurrentSchema,
}

/// Immutable unresolved input and output selector intent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedDeviceIntent {
    input: Option<PreparedDeviceSelectorIntent>,
    output: Option<PreparedDeviceSelectorIntent>,
}

impl PreparedDeviceIntent {
    /// Creates device intent without enumerating or opening any device.
    pub const fn new(
        input: Option<PreparedDeviceSelectorIntent>,
        output: Option<PreparedDeviceSelectorIntent>,
    ) -> Self {
        Self { input, output }
    }

    /// Returns unresolved input selector intent, if requested.
    pub fn input(&self) -> Option<&PreparedDeviceSelectorIntent> {
        self.input.as_ref()
    }

    /// Returns unresolved output selector intent, if requested.
    pub fn output(&self) -> Option<&PreparedDeviceSelectorIntent> {
        self.output.as_ref()
    }
}

/// Passive selector copied from validated configuration intent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedDeviceSelectorIntent {
    stable_id: Option<String>,
    friendly_name: Option<String>,
    backend: BackendIntent,
    ambiguity_policy: AmbiguityPolicy,
}

impl PreparedDeviceSelectorIntent {
    /// Creates a selector without resolving it against a platform backend.
    pub fn new(
        stable_id: Option<String>,
        friendly_name: Option<String>,
        backend: BackendIntent,
        ambiguity_policy: AmbiguityPolicy,
    ) -> Result<Self, RuntimePreparationError> {
        let stable_id = optional_nonempty(stable_id, RuntimeInvariant::EmptyDeviceSelector)?;
        let friendly_name =
            optional_nonempty(friendly_name, RuntimeInvariant::EmptyDeviceSelector)?;
        if stable_id.is_none() && friendly_name.is_none() {
            return Err(RuntimePreparationError::InternalInvariantViolation {
                invariant: RuntimeInvariant::MissingDeviceSelector,
            });
        }
        Ok(Self {
            stable_id,
            friendly_name,
            backend,
            ambiguity_policy,
        })
    }

    /// Returns the requested stable backend identifier.
    pub fn stable_id(&self) -> Option<&str> {
        self.stable_id.as_deref()
    }

    /// Returns the optional human-readable selector.
    pub fn friendly_name(&self) -> Option<&str> {
        self.friendly_name.as_deref()
    }

    /// Returns the requested backend family.
    pub fn backend(&self) -> BackendIntent {
        self.backend
    }

    /// Returns the explicit ambiguity policy.
    pub fn ambiguity_policy(&self) -> AmbiguityPolicy {
        self.ambiguity_policy
    }
}

/// Plan-known capacities and explicit setup-derived capacity state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeCapacityPlan {
    input_channel_count: usize,
    output_channel_count: usize,
    route_count: usize,
    speaker_count: usize,
    callback_frame_count: usize,
    renderer_output_gain_width: usize,
    renderer_scratch: SetupDerivedCapacity,
    renderer_history: SetupDerivedCapacity,
    implementation_temporary: SetupDerivedCapacity,
    delay_processor: SetupDerivedCapacity,
    asrc: SetupDerivedCapacity,
    backend_ring: SetupDerivedCapacity,
}

impl RuntimeCapacityPlan {
    /// Creates capacity intent for values known before runtime setup.
    pub fn new(
        input_channel_count: usize,
        output_channel_count: usize,
        route_count: usize,
        speaker_count: usize,
        callback_frame_count: usize,
    ) -> Result<Self, RuntimePreparationError> {
        for (field, value) in [
            (CapacityField::OutputChannelCount, output_channel_count),
            (CapacityField::SpeakerCount, speaker_count),
            (CapacityField::CallbackFrameCount, callback_frame_count),
        ] {
            if value == 0 {
                return Err(RuntimePreparationError::InvalidCapacity { field });
            }
        }
        let renderer_output_gain_width = output_channel_count;
        Ok(Self {
            input_channel_count,
            output_channel_count,
            route_count,
            speaker_count,
            callback_frame_count,
            renderer_output_gain_width,
            renderer_scratch: SetupDerivedCapacity::DeferredUntilSetup,
            renderer_history: SetupDerivedCapacity::DeferredUntilSetup,
            implementation_temporary: SetupDerivedCapacity::DeferredUntilSetup,
            delay_processor: SetupDerivedCapacity::DeferredUntilSetup,
            asrc: SetupDerivedCapacity::DeferredUntilSetup,
            backend_ring: SetupDerivedCapacity::DeferredUntilSetup,
        })
    }

    /// Returns the plan-known input identity count, which may be zero.
    pub fn input_channel_count(&self) -> usize {
        self.input_channel_count
    }
    /// Returns the plan-known output channel count.
    pub fn output_channel_count(&self) -> usize {
        self.output_channel_count
    }
    /// Returns the plan-known route count, which may be zero.
    pub fn route_count(&self) -> usize {
        self.route_count
    }
    /// Returns the total prepared speaker descriptor count.
    ///
    /// The count includes active and inactive speaker descriptors.
    pub fn speaker_count(&self) -> usize {
        self.speaker_count
    }
    /// Returns the plan-known callback frame intent.
    pub fn callback_frame_count(&self) -> usize {
        self.callback_frame_count
    }
    /// Returns renderer output-gain width derived from output channels.
    pub fn renderer_output_gain_width(&self) -> usize {
        self.renderer_output_gain_width
    }
    /// Returns the setup state of renderer scratch capacity.
    pub fn renderer_scratch(&self) -> SetupDerivedCapacity {
        self.renderer_scratch
    }
    /// Returns the setup state of renderer history capacity.
    pub fn renderer_history(&self) -> SetupDerivedCapacity {
        self.renderer_history
    }
    /// Returns the setup state of implementation temporary capacity.
    pub fn implementation_temporary(&self) -> SetupDerivedCapacity {
        self.implementation_temporary
    }
    /// Returns the setup state of delay-processor capacity.
    pub fn delay_processor(&self) -> SetupDerivedCapacity {
        self.delay_processor
    }
    /// Returns the setup state of ASRC capacity.
    pub fn asrc(&self) -> SetupDerivedCapacity {
        self.asrc
    }
    /// Returns the setup state of backend ring capacity.
    pub fn backend_ring(&self) -> SetupDerivedCapacity {
        self.backend_ring
    }
}

/// State of a capacity that can only be computed during later setup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SetupDerivedCapacity {
    /// The value is intentionally absent until concrete setup components exist.
    DeferredUntilSetup,
}

/// Metadata carried by a passive prepared plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimePlanMetadata {
    configuration_schema_version: u16,
    contract_version: u16,
    evidence: RuntimePlanEvidence,
}

impl RuntimePlanMetadata {
    /// Creates deterministic plan metadata without timestamps or fingerprints.
    pub const fn new(configuration_schema_version: u16) -> Self {
        Self {
            configuration_schema_version,
            contract_version: RUNTIME_PLAN_CONTRACT_VERSION,
            evidence: RuntimePlanEvidence::DeterministicControlPlaneIntent,
        }
    }

    /// Returns the source configuration schema version.
    pub fn configuration_schema_version(&self) -> u16 {
        self.configuration_schema_version
    }
    /// Returns this crate's contract version.
    pub fn contract_version(&self) -> u16 {
        self.contract_version
    }
    /// Returns the classification of what this plan proves.
    pub fn evidence(&self) -> RuntimePlanEvidence {
        self.evidence
    }
}

/// Truth classification for a Checkpoint A plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimePlanEvidence {
    /// Deterministic control-plane intent only; no runtime readiness is claimed.
    DeterministicControlPlaneIntent,
}

/// Stable categories for locally detectable plan-construction failures.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimePreparationError {
    /// A renderer intent cannot be represented by this contract.
    UnsupportedRendererIntent,
    /// A DSP intent cannot be represented by the accepted schema.
    UnsupportedDspIntent,
    /// A required plan-known count is zero or otherwise invalid.
    InvalidCapacity { field: CapacityField },
    /// A checked capacity calculation overflowed.
    ArithmeticOverflow { operation: ArithmeticOperation },
    /// Routing components contradict one another locally.
    IncompatibleRoutingIntent { issue: RoutingIssue },
    /// An adapter is intentionally excluded by accepted policy.
    PolicyLimitedAdapter { adapter: PolicyLimitedAdapter },
    /// Caller-supplied prepared components violate a local invariant.
    InternalInvariantViolation { invariant: RuntimeInvariant },
}

/// Fields whose plan-known capacities may be reported as invalid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapacityField {
    /// Requested sample rate.
    SampleRate,
    /// Input channel count.
    InputChannelCount,
    /// Output channel count.
    OutputChannelCount,
    /// Explicit route count.
    RouteCount,
    /// Speaker count.
    SpeakerCount,
    /// Callback frame count.
    CallbackFrameCount,
}

/// Capacity calculations that may report checked arithmetic overflow.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArithmeticOperation {
    /// Conversion of callback frame intent into the platform count type.
    CallbackFrameConversion,
    /// Future frame-by-channel storage sizing.
    FrameChannelStorage,
}

/// Local routing contradictions detected without runtime mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutingIssue {
    /// Input identities contain a duplicate.
    DuplicateInputIdentity,
    /// Output identities contain a duplicate.
    DuplicateOutputIdentity,
    /// Inactive output identities contain a duplicate.
    DuplicateInactiveOutputIdentity,
    /// A route references an unknown input identity.
    UnknownInputReference,
    /// A route references an unknown output identity.
    UnknownOutputReference,
    /// An inactive output references an unknown output identity.
    UnknownInactiveOutputReference,
}

/// Adapter families constrained by accepted policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyLimitedAdapter {
    /// An external DSP adapter not representable by current schema intent.
    ExternalDsp,
}

/// Local invariants enforced by passive contract constructors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeInvariant {
    /// A channel identity is empty.
    EmptyChannelIdentifier,
    /// A channel label is empty.
    EmptyChannelLabel,
    /// A route input identity is empty.
    EmptyRouteInput,
    /// A route output identity is empty.
    EmptyRouteOutput,
    /// A speaker identity is empty.
    EmptySpeakerIdentifier,
    /// A speaker label is empty.
    EmptySpeakerLabel,
    /// Speaker geometry contains a non-finite component.
    NonFiniteSpeakerGeometry,
    /// Speaker identities are duplicated.
    DuplicateSpeakerIdentifier,
    /// Horizontal spread is non-finite or outside `0.0..=1.0`.
    InvalidHorizontalSpread,
    /// A supplied selector string is empty.
    EmptyDeviceSelector,
    /// A device selector has neither a stable ID nor friendly name.
    MissingDeviceSelector,
    /// The configured output count disagrees with the validated format intent.
    AudioOutputCountMismatch,
    /// Input selector direction disagrees with its configuration field.
    InputDeviceDirectionMismatch,
    /// Output selector direction disagrees with its configuration field.
    OutputDeviceDirectionMismatch,
    /// Top-level components disagree with the capacity plan.
    ComponentCapacityMismatch,
}

impl fmt::Display for RuntimePreparationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedRendererIntent => formatter.write_str("unsupported renderer intent"),
            Self::UnsupportedDspIntent => formatter.write_str("unsupported DSP intent"),
            Self::InvalidCapacity { field } => write!(formatter, "invalid capacity: {field:?}"),
            Self::ArithmeticOverflow { operation } => {
                write!(formatter, "capacity arithmetic overflow: {operation:?}")
            }
            Self::IncompatibleRoutingIntent { issue } => {
                write!(formatter, "incompatible routing intent: {issue:?}")
            }
            Self::PolicyLimitedAdapter { adapter } => {
                write!(formatter, "adapter limited by policy: {adapter:?}")
            }
            Self::InternalInvariantViolation { invariant } => {
                write!(formatter, "runtime plan invariant violated: {invariant:?}")
            }
        }
    }
}

impl Error for RuntimePreparationError {}

fn nonempty(value: String, invariant: RuntimeInvariant) -> Result<String, RuntimePreparationError> {
    if value.trim().is_empty() {
        return Err(RuntimePreparationError::InternalInvariantViolation { invariant });
    }
    Ok(value)
}

fn optional_nonempty(
    value: Option<String>,
    invariant: RuntimeInvariant,
) -> Result<Option<String>, RuntimePreparationError> {
    value.map(|value| nonempty(value, invariant)).transpose()
}

fn unique_identity_ids(
    values: &[PreparedChannelIdentity],
    issue: RoutingIssue,
) -> Result<BTreeSet<&str>, RuntimePreparationError> {
    let mut ids = BTreeSet::new();
    for value in values {
        if !ids.insert(value.id()) {
            return Err(RuntimePreparationError::IncompatibleRoutingIntent { issue });
        }
    }
    Ok(ids)
}

fn unique_strings(
    values: &[String],
    issue: RoutingIssue,
) -> Result<BTreeSet<&str>, RuntimePreparationError> {
    let mut ids = BTreeSet::new();
    for value in values {
        if !ids.insert(value.as_str()) {
            return Err(RuntimePreparationError::IncompatibleRoutingIntent { issue });
        }
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(id: &str) -> PreparedChannelIdentity {
        PreparedChannelIdentity::new(id, id.to_uppercase()).unwrap()
    }

    fn route(input: &str, output: &str) -> PreparedRoute {
        PreparedRoute::new(input, output).unwrap()
    }

    fn speaker(id: &str, role: ChannelRole, x: f32) -> PreparedSpeaker {
        speaker_with_active(id, role, x, true)
    }

    fn speaker_with_active(id: &str, role: ChannelRole, x: f32, active: bool) -> PreparedSpeaker {
        PreparedSpeaker::new(
            id,
            id.to_uppercase(),
            role,
            Vector3::new(x, 1.0, 0.0),
            active,
        )
        .unwrap()
    }

    fn stereo_plan() -> PreparedRuntimePlan {
        let format = PreparedAudioFormatIntent::new(
            48_000,
            SampleFormatIntent::Float32,
            2,
            2,
            256,
            FormatFallbackPolicy::Reject,
        )
        .unwrap();
        let routing = PreparedRoutingPlan::new(
            vec![identity("in-b"), identity("in-a")],
            vec![identity("out-r"), identity("out-l")],
            vec![route("in-b", "out-r"), route("in-a", "out-l")],
            vec![],
        )
        .unwrap();
        let layout = PreparedLayoutPlan::new(
            PreparedLayoutKind::Standard(StandardLayout::Stereo),
            vec![
                speaker("right", ChannelRole::FrontRight, 1.0),
                speaker("left", ChannelRole::FrontLeft, -1.0),
            ],
        )
        .unwrap();
        PreparedRuntimePlan::new(
            RuntimePlanMetadata::new(1),
            PreparedExecutionPlan::new(
                format,
                PreparedRendererPlan::point_source_horizontal_vbap(),
                PreparedDspPlan::DeferredByCurrentSchema,
            ),
            PreparedTopologyPlan::new(routing, layout),
            PreparedDeviceIntent::new(None, None),
            RuntimeCapacityPlan::new(2, 2, 2, 2, 256).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn top_level_plan_preserves_all_components() {
        let plan = stereo_plan();
        assert_eq!(
            plan.metadata().contract_version(),
            RUNTIME_PLAN_CONTRACT_VERSION
        );
        assert_eq!(plan.execution().audio_format().sample_rate(), 48_000);
        assert_eq!(
            plan.execution().renderer().kind(),
            PreparedRendererKind::PointSourceHorizontalVbap
        );
        assert_eq!(
            plan.execution().dsp(),
            PreparedDspPlan::DeferredByCurrentSchema
        );
        assert_eq!(plan.topology().routing().routes().len(), 2);
        assert_eq!(plan.topology().layout().speakers().len(), 2);
        assert_eq!(plan.capacity().callback_frame_count(), 256);
    }

    #[test]
    fn routing_order_is_not_silently_sorted() {
        let plan = stereo_plan();
        let routing = plan.topology().routing();
        assert_eq!(routing.inputs()[0].id(), "in-b");
        assert_eq!(routing.outputs()[0].id(), "out-r");
        assert_eq!(routing.routes()[0].output_id(), "out-r");
        assert_eq!(plan.topology().layout().speakers()[0].id(), "right");
    }

    #[test]
    fn duplicate_routing_identity_is_rejected_without_deduplication() {
        let result = PreparedRoutingPlan::new(
            vec![identity("in"), identity("in")],
            vec![identity("out")],
            vec![route("in", "out")],
            vec![],
        );
        assert_eq!(
            result,
            Err(RuntimePreparationError::IncompatibleRoutingIntent {
                issue: RoutingIssue::DuplicateInputIdentity,
            })
        );
    }

    #[test]
    fn route_with_unknown_identity_is_rejected() {
        let result = PreparedRoutingPlan::new(
            vec![identity("in")],
            vec![identity("out")],
            vec![route("missing", "out")],
            vec![],
        );
        assert_eq!(
            result,
            Err(RuntimePreparationError::IncompatibleRoutingIntent {
                issue: RoutingIssue::UnknownInputReference,
            })
        );
    }

    #[test]
    fn zero_capacity_is_rejected() {
        assert_eq!(
            RuntimeCapacityPlan::new(2, 0, 2, 2, 256),
            Err(RuntimePreparationError::InvalidCapacity {
                field: CapacityField::OutputChannelCount,
            })
        );
    }

    #[test]
    fn setup_derived_capacities_are_explicitly_deferred() {
        let capacity = RuntimeCapacityPlan::new(2, 2, 2, 2, 256).unwrap();
        let deferred = SetupDerivedCapacity::DeferredUntilSetup;
        assert_eq!(capacity.renderer_scratch(), deferred);
        assert_eq!(capacity.renderer_history(), deferred);
        assert_eq!(capacity.implementation_temporary(), deferred);
        assert_eq!(capacity.delay_processor(), deferred);
        assert_eq!(capacity.asrc(), deferred);
        assert_eq!(capacity.backend_ring(), deferred);
        assert_eq!(capacity.renderer_output_gain_width(), 2);
    }

    #[test]
    fn renderer_descriptors_reject_invalid_spread() {
        assert_eq!(
            PreparedRendererPlan::horizontal_spread_vbap(f32::NAN),
            Err(RuntimePreparationError::InternalInvariantViolation {
                invariant: RuntimeInvariant::InvalidHorizontalSpread,
            })
        );
        assert_eq!(
            PreparedRendererPlan::horizontal_spread_vbap(1.1),
            Err(RuntimePreparationError::InternalInvariantViolation {
                invariant: RuntimeInvariant::InvalidHorizontalSpread,
            })
        );
    }

    #[test]
    fn device_intent_remains_unresolved() {
        let selector = PreparedDeviceSelectorIntent::new(
            Some("stable-output".to_owned()),
            None,
            BackendIntent::Virtual,
            AmbiguityPolicy::RequireStableIdentifier,
        )
        .unwrap();
        let intent = PreparedDeviceIntent::new(None, Some(selector));
        assert_eq!(intent.output().unwrap().stable_id(), Some("stable-output"));
        assert!(intent.input().is_none());
    }

    #[test]
    fn selector_without_identity_is_rejected() {
        assert_eq!(
            PreparedDeviceSelectorIntent::new(
                None,
                None,
                BackendIntent::Offline,
                AmbiguityPolicy::Reject,
            ),
            Err(RuntimePreparationError::InternalInvariantViolation {
                invariant: RuntimeInvariant::MissingDeviceSelector,
            })
        );
    }

    #[test]
    fn component_capacity_mismatch_is_rejected() {
        let mut plan = stereo_plan();
        plan.capacity = RuntimeCapacityPlan::new(2, 2, 1, 2, 256).unwrap();
        let result = PreparedRuntimePlan::new(
            plan.metadata,
            plan.execution,
            plan.topology,
            plan.device_intent,
            plan.capacity,
        );
        assert_eq!(
            result,
            Err(RuntimePreparationError::InternalInvariantViolation {
                invariant: RuntimeInvariant::ComponentCapacityMismatch,
            })
        );
    }

    #[test]
    fn error_display_is_stable_and_structured() {
        let error = RuntimePreparationError::InvalidCapacity {
            field: CapacityField::RouteCount,
        };
        assert_eq!(error.to_string(), "invalid capacity: RouteCount");
        assert!(Error::source(&error).is_none());
    }

    #[test]
    fn identical_values_compare_equal_deterministically() {
        assert_eq!(stereo_plan(), stereo_plan());
    }

    #[test]
    fn renderer_families_are_passive_semantic_values() {
        let basic = PreparedRendererPlan::basic_inverse_distance();
        let point = PreparedRendererPlan::point_source_horizontal_vbap();
        let spread = PreparedRendererPlan::horizontal_spread_vbap(0.25).unwrap();
        assert_eq!(basic.kind(), PreparedRendererKind::BasicInverseDistance);
        assert_eq!(
            point.kind(),
            PreparedRendererKind::PointSourceHorizontalVbap
        );
        assert_eq!(spread.kind(), PreparedRendererKind::HorizontalSpreadVbap);
        assert_eq!(spread.horizontal_spread(), Some(0.25));
    }

    #[test]
    fn dsp_state_cannot_represent_an_execution_graph() {
        assert_ne!(
            PreparedDspPlan::None,
            PreparedDspPlan::DeferredByCurrentSchema
        );
    }

    #[test]
    fn prepared_speaker_preserves_active_state() {
        let active = speaker_with_active("active", ChannelRole::FrontLeft, -1.0, true);
        let inactive = speaker_with_active("inactive", ChannelRole::FrontRight, 1.0, false);
        assert!(active.is_active());
        assert!(!inactive.is_active());
    }

    #[test]
    fn layout_preserves_active_and_inactive_speakers_in_order() {
        let layout = PreparedLayoutPlan::new(
            PreparedLayoutKind::Standard(StandardLayout::Stereo),
            vec![
                speaker_with_active("inactive", ChannelRole::FrontRight, 1.0, false),
                speaker_with_active("active", ChannelRole::FrontLeft, -1.0, true),
            ],
        )
        .unwrap();

        assert_eq!(layout.speakers()[0].id(), "inactive");
        assert!(!layout.speakers()[0].is_active());
        assert_eq!(layout.speakers()[1].id(), "active");
        assert!(layout.speakers()[1].is_active());
    }

    #[test]
    fn topology_allows_total_speaker_count_to_differ_from_output_count() {
        let routing = PreparedRoutingPlan::new(
            vec![identity("in")],
            vec![identity("out")],
            vec![route("in", "out")],
            vec![],
        )
        .unwrap();
        let layout = PreparedLayoutPlan::new(
            PreparedLayoutKind::CustomHorizontal,
            vec![
                speaker_with_active("active", ChannelRole::FrontLeft, -1.0, true),
                speaker_with_active("inactive", ChannelRole::FrontRight, 1.0, false),
            ],
        )
        .unwrap();

        let topology = PreparedTopologyPlan::new(routing, layout);
        assert_eq!(topology.routing().outputs().len(), 1);
        assert_eq!(topology.layout().speakers().len(), 2);
    }

    #[test]
    fn top_level_capacity_still_matches_total_prepared_speaker_descriptor_count() {
        let format = PreparedAudioFormatIntent::new(
            48_000,
            SampleFormatIntent::Float32,
            1,
            1,
            256,
            FormatFallbackPolicy::Reject,
        )
        .unwrap();
        let routing = PreparedRoutingPlan::new(
            vec![identity("in")],
            vec![identity("out")],
            vec![route("in", "out")],
            vec![],
        )
        .unwrap();
        let layout = PreparedLayoutPlan::new(
            PreparedLayoutKind::CustomHorizontal,
            vec![
                speaker_with_active("active", ChannelRole::FrontLeft, -1.0, true),
                speaker_with_active("inactive", ChannelRole::FrontRight, 1.0, false),
            ],
        )
        .unwrap();
        let plan = PreparedRuntimePlan::new(
            RuntimePlanMetadata::new(1),
            PreparedExecutionPlan::new(
                format,
                PreparedRendererPlan::basic_inverse_distance(),
                PreparedDspPlan::None,
            ),
            PreparedTopologyPlan::new(routing, layout),
            PreparedDeviceIntent::new(None, None),
            RuntimeCapacityPlan::new(1, 1, 1, 2, 256).unwrap(),
        )
        .unwrap();

        assert_eq!(plan.capacity().speaker_count(), 2);
        assert_eq!(plan.topology().layout().speakers().len(), 2);
    }

    #[test]
    fn no_silent_filtering_of_inactive_speakers() {
        let layout = PreparedLayoutPlan::new(
            PreparedLayoutKind::CustomHorizontal,
            vec![
                speaker_with_active("inactive-a", ChannelRole::FrontLeft, -1.0, false),
                speaker_with_active("active", ChannelRole::FrontCenter, 0.0, true),
                speaker_with_active("inactive-b", ChannelRole::FrontRight, 1.0, false),
            ],
        )
        .unwrap();

        assert_eq!(layout.speakers().len(), 3);
        assert_eq!(layout.speakers()[0].id(), "inactive-a");
        assert_eq!(layout.speakers()[1].id(), "active");
        assert_eq!(layout.speakers()[2].id(), "inactive-b");
    }
}
