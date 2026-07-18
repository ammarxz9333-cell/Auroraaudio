use std::{error::Error, fmt};

use aurora_config::BackendIntent;

use crate::{
    PreparedAudioFormatIntent, PreparedDeviceIntent, PreparedDspPlan, PreparedRendererPlan,
    PreparedRuntimePlan, PreparedTopologyPlan,
};

const SETUP_STAGE_COUNT: usize = 6;
const SETUP_DEPENDENCY_COUNT: usize = 9;

const CANONICAL_STAGES: [SetupStage; SETUP_STAGE_COUNT] = [
    SetupStage::DeviceSelectionIntent,
    SetupStage::RequestedFormatPlanning,
    SetupStage::RendererPreparationIntent,
    SetupStage::DspPreparationIntent,
    SetupStage::BackendPreparationIntent,
    SetupStage::SetupPlanComplete,
];

const CANONICAL_DEPENDENCIES: [SetupDependency; SETUP_DEPENDENCY_COUNT] = [
    SetupDependency::new(
        SetupStage::RendererPreparationIntent,
        SetupStage::RequestedFormatPlanning,
    ),
    SetupDependency::new(
        SetupStage::DspPreparationIntent,
        SetupStage::RequestedFormatPlanning,
    ),
    SetupDependency::new(
        SetupStage::BackendPreparationIntent,
        SetupStage::DeviceSelectionIntent,
    ),
    SetupDependency::new(
        SetupStage::BackendPreparationIntent,
        SetupStage::RequestedFormatPlanning,
    ),
    SetupDependency::new(
        SetupStage::SetupPlanComplete,
        SetupStage::DeviceSelectionIntent,
    ),
    SetupDependency::new(
        SetupStage::SetupPlanComplete,
        SetupStage::RequestedFormatPlanning,
    ),
    SetupDependency::new(
        SetupStage::SetupPlanComplete,
        SetupStage::RendererPreparationIntent,
    ),
    SetupDependency::new(
        SetupStage::SetupPlanComplete,
        SetupStage::DspPreparationIntent,
    ),
    SetupDependency::new(
        SetupStage::SetupPlanComplete,
        SetupStage::BackendPreparationIntent,
    ),
];

/// Derives deterministic, descriptive setup intent from a prepared runtime plan.
///
/// This function copies bounded control-plane values only. It does not resolve
/// devices, negotiate formats, construct components, perform I/O, or claim that
/// any setup stage ran.
///
/// # Errors
///
/// Returns [`SetupPlanningError`] if defensive validation finds a noncanonical
/// stage list, incomplete or cyclic dependency graph, or inconsistent prepared
/// components. A valid [`PreparedRuntimePlan`] normally produces a valid setup
/// plan.
pub fn prepare_setup_plan(
    runtime_plan: &PreparedRuntimePlan,
) -> Result<PreparedSetupPlan, SetupPlanningError> {
    let device_intent = runtime_plan.device_intent().clone();
    let requested_audio_format = runtime_plan.execution().audio_format().clone();
    let renderer = PreparedRendererSetupIntent {
        renderer: runtime_plan.execution().renderer().clone(),
        topology: runtime_plan.topology().clone(),
    };
    let dsp = PreparedDspSetupIntent {
        state: runtime_plan.execution().dsp(),
    };
    let backend = PreparedBackendSetupIntent::from_device_intent(&device_intent);

    PreparedSetupPlan::new(
        device_intent,
        requested_audio_format,
        renderer,
        dsp,
        backend,
        CANONICAL_STAGES,
        CANONICAL_DEPENDENCIES,
    )
}

/// Immutable aggregate describing future setup intent without executing it.
///
/// Owned strings and vectors are bounded copies made on the setup thread. The
/// stage and dependency graph itself has a fixed size. This value contains no
/// handles, callbacks, trait objects, processes, streams, or live resources.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedSetupPlan {
    device_intent: PreparedDeviceIntent,
    requested_audio_format: PreparedAudioFormatIntent,
    renderer: PreparedRendererSetupIntent,
    dsp: PreparedDspSetupIntent,
    backend: PreparedBackendSetupIntent,
    stages: [SetupStage; SETUP_STAGE_COUNT],
    dependencies: [SetupDependency; SETUP_DEPENDENCY_COUNT],
}

impl PreparedSetupPlan {
    #[allow(clippy::too_many_arguments)]
    fn new(
        device_intent: PreparedDeviceIntent,
        requested_audio_format: PreparedAudioFormatIntent,
        renderer: PreparedRendererSetupIntent,
        dsp: PreparedDspSetupIntent,
        backend: PreparedBackendSetupIntent,
        stages: [SetupStage; SETUP_STAGE_COUNT],
        dependencies: [SetupDependency; SETUP_DEPENDENCY_COUNT],
    ) -> Result<Self, SetupPlanningError> {
        validate_stages(&stages)?;
        validate_dependencies(&dependencies)?;

        if requested_audio_format.output_channel_count()
            != renderer.topology().routing().outputs().len()
        {
            return Err(SetupPlanningError::InvariantViolation {
                invariant: SetupPlanInvariant::FormatTopologyMismatch,
            });
        }
        if backend.requested_input_backend()
            != device_intent.input().map(|selector| selector.backend())
            || backend.requested_output_backend()
                != device_intent.output().map(|selector| selector.backend())
        {
            return Err(SetupPlanningError::InvariantViolation {
                invariant: SetupPlanInvariant::BackendDeviceIntentMismatch,
            });
        }

        Ok(Self {
            device_intent,
            requested_audio_format,
            renderer,
            dsp,
            backend,
            stages,
            dependencies,
        })
    }

    /// Returns unresolved device selectors copied from prepared intent.
    pub fn device_intent(&self) -> &PreparedDeviceIntent {
        &self.device_intent
    }

    /// Returns the requested, never negotiated, audio format description.
    pub fn requested_audio_format(&self) -> &PreparedAudioFormatIntent {
        &self.requested_audio_format
    }

    /// Returns passive renderer and topology setup intent.
    pub fn renderer(&self) -> &PreparedRendererSetupIntent {
        &self.renderer
    }

    /// Returns the DSP state representable by the current accepted schema.
    pub fn dsp(&self) -> &PreparedDspSetupIntent {
        &self.dsp
    }

    /// Returns requested backend families without backend instances or handles.
    pub fn backend(&self) -> &PreparedBackendSetupIntent {
        &self.backend
    }

    /// Returns all descriptive setup stages in canonical order.
    pub fn stages(&self) -> &[SetupStage] {
        &self.stages
    }

    /// Returns the complete bounded dependency graph in canonical order.
    pub fn dependencies(&self) -> &[SetupDependency] {
        &self.dependencies
    }
}

/// One canonical descriptive setup stage.
///
/// Variants describe intent only. They do not imply execution, readiness,
/// backend acceptance, host observation, or physical evidence.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SetupStage {
    /// Preserve unresolved input and output device-selection intent.
    DeviceSelectionIntent,
    /// Preserve requested audio-format intent before negotiation.
    RequestedFormatPlanning,
    /// Describe renderer and topology preparation requirements.
    RendererPreparationIntent,
    /// Describe the DSP state available from the current schema.
    DspPreparationIntent,
    /// Describe requested backend intent without selecting an implementation.
    BackendPreparationIntent,
    /// Record that the immutable description contains every required stage.
    ///
    /// This does not mean that setup ran or that a runtime is ready.
    SetupPlanComplete,
}

impl SetupStage {
    const fn index(self) -> usize {
        match self {
            Self::DeviceSelectionIntent => 0,
            Self::RequestedFormatPlanning => 1,
            Self::RendererPreparationIntent => 2,
            Self::DspPreparationIntent => 3,
            Self::BackendPreparationIntent => 4,
            Self::SetupPlanComplete => 5,
        }
    }
}

/// Directed dependency stating that `stage` requires `depends_on` first.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SetupDependency {
    stage: SetupStage,
    depends_on: SetupStage,
}

impl SetupDependency {
    const fn new(stage: SetupStage, depends_on: SetupStage) -> Self {
        Self { stage, depends_on }
    }

    /// Returns the dependent stage.
    pub fn stage(self) -> SetupStage {
        self.stage
    }

    /// Returns the prerequisite stage.
    pub fn depends_on(self) -> SetupStage {
        self.depends_on
    }
}

/// Passive renderer selection and prepared topology required by later setup.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedRendererSetupIntent {
    renderer: PreparedRendererPlan,
    topology: PreparedTopologyPlan,
}

impl PreparedRendererSetupIntent {
    /// Returns the selected renderer family and accepted scalar intent.
    pub fn renderer(&self) -> &PreparedRendererPlan {
        &self.renderer
    }

    /// Returns canonical routing and normalized layout intent.
    pub fn topology(&self) -> &PreparedTopologyPlan {
        &self.topology
    }
}

/// Passive DSP setup state available from the current accepted schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreparedDspSetupIntent {
    state: PreparedDspPlan,
}

impl PreparedDspSetupIntent {
    /// Returns explicit absent or schema-deferred DSP intent.
    pub fn state(self) -> PreparedDspPlan {
        self.state
    }
}

/// Requested backend families copied from unresolved device selectors.
///
/// Absence remains absence. This type does not select, construct, probe, or
/// confirm a backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreparedBackendSetupIntent {
    requested_input_backend: Option<BackendIntent>,
    requested_output_backend: Option<BackendIntent>,
}

impl PreparedBackendSetupIntent {
    fn from_device_intent(device_intent: &PreparedDeviceIntent) -> Self {
        Self {
            requested_input_backend: device_intent.input().map(|selector| selector.backend()),
            requested_output_backend: device_intent.output().map(|selector| selector.backend()),
        }
    }

    /// Returns the requested input backend family, if one was configured.
    pub fn requested_input_backend(self) -> Option<BackendIntent> {
        self.requested_input_backend
    }

    /// Returns the requested output backend family, if one was configured.
    pub fn requested_output_backend(self) -> Option<BackendIntent> {
        self.requested_output_backend
    }
}

/// Structured setup-planning failures detected without executing setup.
///
/// Errors describe local representation defects only. They do not report host,
/// negotiation, runtime, callback, or physical failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SetupPlanningError {
    /// A supplied descriptive plan violates a local setup invariant.
    InvariantViolation { invariant: SetupPlanInvariant },
}

/// Defensive invariants for the immutable setup-stage description.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SetupPlanInvariant {
    /// A stage occurs more than once, necessarily displacing another stage.
    DuplicateStage,
    /// Every stage exists, but their order is not canonical.
    NonCanonicalStageOrder,
    /// A dependency references the same stage as both ends.
    SelfDependency,
    /// The dependency graph contains a cycle.
    CyclicDependencyGraph,
    /// A dependency occurs more than once.
    DuplicateDependency,
    /// A required canonical dependency is absent.
    MissingRequiredDependency,
    /// The required dependency set is not in canonical order.
    NonCanonicalDependencyOrder,
    /// Requested format output count disagrees with prepared routing topology.
    FormatTopologyMismatch,
    /// Requested backend families disagree with unresolved device intent.
    BackendDeviceIntentMismatch,
}

impl fmt::Display for SetupPlanningError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvariantViolation { invariant } => {
                write!(formatter, "setup plan invariant violated: {invariant:?}")
            }
        }
    }
}

impl Error for SetupPlanningError {}

fn validate_stages(stages: &[SetupStage; SETUP_STAGE_COUNT]) -> Result<(), SetupPlanningError> {
    let mut seen = [false; SETUP_STAGE_COUNT];
    for stage in stages {
        if seen[stage.index()] {
            return Err(SetupPlanningError::InvariantViolation {
                invariant: SetupPlanInvariant::DuplicateStage,
            });
        }
        seen[stage.index()] = true;
    }
    if stages != &CANONICAL_STAGES {
        return Err(SetupPlanningError::InvariantViolation {
            invariant: SetupPlanInvariant::NonCanonicalStageOrder,
        });
    }
    Ok(())
}

fn validate_dependencies(
    dependencies: &[SetupDependency; SETUP_DEPENDENCY_COUNT],
) -> Result<(), SetupPlanningError> {
    for (index, dependency) in dependencies.iter().enumerate() {
        if dependency.stage == dependency.depends_on {
            return Err(SetupPlanningError::InvariantViolation {
                invariant: SetupPlanInvariant::SelfDependency,
            });
        }
        if dependencies[..index].contains(dependency) {
            return Err(SetupPlanningError::InvariantViolation {
                invariant: SetupPlanInvariant::DuplicateDependency,
            });
        }
    }
    validate_acyclic(dependencies)?;

    if CANONICAL_DEPENDENCIES
        .iter()
        .any(|required| !dependencies.contains(required))
    {
        return Err(SetupPlanningError::InvariantViolation {
            invariant: SetupPlanInvariant::MissingRequiredDependency,
        });
    }
    if dependencies != &CANONICAL_DEPENDENCIES {
        return Err(SetupPlanningError::InvariantViolation {
            invariant: SetupPlanInvariant::NonCanonicalDependencyOrder,
        });
    }
    Ok(())
}

fn validate_acyclic(
    dependencies: &[SetupDependency; SETUP_DEPENDENCY_COUNT],
) -> Result<(), SetupPlanningError> {
    let mut indegree = [0_u8; SETUP_STAGE_COUNT];
    for dependency in dependencies {
        indegree[dependency.stage.index()] += 1;
    }

    let mut removed = [false; SETUP_STAGE_COUNT];
    let mut removed_count = 0;
    loop {
        let next = CANONICAL_STAGES
            .iter()
            .copied()
            .find(|stage| !removed[stage.index()] && indegree[stage.index()] == 0);
        let Some(stage) = next else {
            break;
        };
        removed[stage.index()] = true;
        removed_count += 1;
        for dependency in dependencies {
            if dependency.depends_on == stage {
                indegree[dependency.stage.index()] -= 1;
            }
        }
    }

    if removed_count != SETUP_STAGE_COUNT {
        return Err(SetupPlanningError::InvariantViolation {
            invariant: SetupPlanInvariant::CyclicDependencyGraph,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use aurora_config::{
        AmbiguityPolicy, BackendIntent, DeviceDirection, DeviceSelectionIntent,
        ValidatedConfiguration,
    };

    use super::*;
    use crate::{prepare_runtime_plan, PreparedRendererKind};

    const STEREO: &[u8] = include_bytes!("../../../fixtures/config/stereo-basic-v1.json");
    const SPREAD: &[u8] = include_bytes!("../../../fixtures/config/phase-3b-spread-v1.json");

    fn runtime_plan(bytes: &[u8]) -> PreparedRuntimePlan {
        let validated = ValidatedConfiguration::from_json(bytes).unwrap();
        prepare_runtime_plan(&validated).unwrap()
    }

    fn setup_plan(bytes: &[u8]) -> PreparedSetupPlan {
        prepare_setup_plan(&runtime_plan(bytes)).unwrap()
    }

    fn from_parts(
        plan: &PreparedSetupPlan,
        stages: [SetupStage; SETUP_STAGE_COUNT],
        dependencies: [SetupDependency; SETUP_DEPENDENCY_COUNT],
    ) -> Result<PreparedSetupPlan, SetupPlanningError> {
        PreparedSetupPlan::new(
            plan.device_intent.clone(),
            plan.requested_audio_format.clone(),
            plan.renderer.clone(),
            plan.dsp,
            plan.backend,
            stages,
            dependencies,
        )
    }

    #[test]
    fn repeated_setup_derivation_is_deterministic() {
        let runtime = runtime_plan(STEREO);
        assert_eq!(prepare_setup_plan(&runtime), prepare_setup_plan(&runtime));
    }

    #[test]
    fn stages_and_dependencies_use_canonical_order() {
        let plan = setup_plan(STEREO);
        assert_eq!(plan.stages(), &CANONICAL_STAGES);
        assert_eq!(plan.dependencies(), &CANONICAL_DEPENDENCIES);
        assert_eq!(plan.stages().last(), Some(&SetupStage::SetupPlanComplete));
        assert_eq!(
            plan.dependencies()
                .iter()
                .filter(|dependency| dependency.stage() == SetupStage::SetupPlanComplete)
                .count(),
            SETUP_STAGE_COUNT - 1
        );
    }

    #[test]
    fn canonical_dependency_graph_is_acyclic() {
        assert_eq!(validate_acyclic(&CANONICAL_DEPENDENCIES), Ok(()));
        for dependency in CANONICAL_DEPENDENCIES {
            let prerequisite = CANONICAL_STAGES
                .iter()
                .position(|stage| *stage == dependency.depends_on())
                .unwrap();
            let dependent = CANONICAL_STAGES
                .iter()
                .position(|stage| *stage == dependency.stage())
                .unwrap();
            assert!(prerequisite < dependent);
        }
    }

    #[test]
    fn unresolved_devices_and_requested_backends_are_preserved() {
        let mut config = ValidatedConfiguration::from_json(STEREO)
            .unwrap()
            .config()
            .clone();
        config.input_device = Some(DeviceSelectionIntent {
            stable_id: Some("requested-input".to_owned()),
            friendly_name: None,
            backend: BackendIntent::Cpal,
            direction: DeviceDirection::Input,
            ambiguity_policy: AmbiguityPolicy::RequireStableIdentifier,
        });
        config.output_device = Some(DeviceSelectionIntent {
            stable_id: None,
            friendly_name: Some("requested-output".to_owned()),
            backend: BackendIntent::Virtual,
            direction: DeviceDirection::Output,
            ambiguity_policy: AmbiguityPolicy::Reject,
        });
        let runtime = prepare_runtime_plan(&ValidatedConfiguration::new(config).unwrap()).unwrap();
        let plan = prepare_setup_plan(&runtime).unwrap();

        assert_eq!(
            plan.device_intent().input().unwrap().stable_id(),
            Some("requested-input")
        );
        assert_eq!(
            plan.device_intent().output().unwrap().friendly_name(),
            Some("requested-output")
        );
        assert_eq!(
            plan.backend().requested_input_backend(),
            Some(BackendIntent::Cpal)
        );
        assert_eq!(
            plan.backend().requested_output_backend(),
            Some(BackendIntent::Virtual)
        );
    }

    #[test]
    fn requested_format_and_renderer_topology_are_preserved() {
        let plan = setup_plan(SPREAD);
        assert_eq!(plan.requested_audio_format().sample_rate(), 48_000);
        assert_eq!(
            plan.renderer().renderer().kind(),
            PreparedRendererKind::HorizontalSpreadVbap
        );
        assert_eq!(plan.renderer().renderer().horizontal_spread(), Some(0.0));
        assert_eq!(
            plan.renderer().topology().routing().outputs().len(),
            plan.requested_audio_format().output_channel_count()
        );
    }

    #[test]
    fn current_schema_dsp_state_is_preserved_without_synthesis() {
        let plan = setup_plan(STEREO);
        assert_eq!(plan.dsp().state(), PreparedDspPlan::None);
    }

    #[test]
    fn duplicate_and_noncanonical_stages_are_rejected() {
        let plan = setup_plan(STEREO);
        let mut duplicate = CANONICAL_STAGES;
        duplicate[1] = duplicate[0];
        assert_eq!(
            from_parts(&plan, duplicate, CANONICAL_DEPENDENCIES),
            Err(SetupPlanningError::InvariantViolation {
                invariant: SetupPlanInvariant::DuplicateStage
            })
        );

        let mut reordered = CANONICAL_STAGES;
        reordered.swap(0, 1);
        assert_eq!(
            from_parts(&plan, reordered, CANONICAL_DEPENDENCIES),
            Err(SetupPlanningError::InvariantViolation {
                invariant: SetupPlanInvariant::NonCanonicalStageOrder
            })
        );
    }

    #[test]
    fn cyclic_dependency_is_rejected() {
        let plan = setup_plan(STEREO);
        let mut dependencies = CANONICAL_DEPENDENCIES;
        dependencies[2] = SetupDependency::new(
            SetupStage::RequestedFormatPlanning,
            SetupStage::RendererPreparationIntent,
        );
        assert_eq!(
            from_parts(&plan, CANONICAL_STAGES, dependencies),
            Err(SetupPlanningError::InvariantViolation {
                invariant: SetupPlanInvariant::CyclicDependencyGraph
            })
        );
    }

    #[test]
    fn missing_or_reordered_dependency_is_rejected() {
        let plan = setup_plan(STEREO);
        let mut missing = CANONICAL_DEPENDENCIES;
        missing[0] = SetupDependency::new(
            SetupStage::RendererPreparationIntent,
            SetupStage::DeviceSelectionIntent,
        );
        assert_eq!(
            from_parts(&plan, CANONICAL_STAGES, missing),
            Err(SetupPlanningError::InvariantViolation {
                invariant: SetupPlanInvariant::MissingRequiredDependency
            })
        );

        let mut reordered = CANONICAL_DEPENDENCIES;
        reordered.swap(0, 1);
        assert_eq!(
            from_parts(&plan, CANONICAL_STAGES, reordered),
            Err(SetupPlanningError::InvariantViolation {
                invariant: SetupPlanInvariant::NonCanonicalDependencyOrder
            })
        );
    }

    #[test]
    fn backend_device_mismatch_is_rejected() {
        let plan = setup_plan(STEREO);
        let mismatched_backend = PreparedBackendSetupIntent {
            requested_input_backend: None,
            requested_output_backend: Some(BackendIntent::Virtual),
        };
        assert_eq!(
            PreparedSetupPlan::new(
                plan.device_intent.clone(),
                plan.requested_audio_format.clone(),
                plan.renderer.clone(),
                plan.dsp,
                mismatched_backend,
                CANONICAL_STAGES,
                CANONICAL_DEPENDENCIES,
            ),
            Err(SetupPlanningError::InvariantViolation {
                invariant: SetupPlanInvariant::BackendDeviceIntentMismatch
            })
        );
    }

    #[test]
    fn requested_format_topology_mismatch_is_rejected() {
        let plan = setup_plan(STEREO);
        let mismatched_format = PreparedAudioFormatIntent::new(
            plan.requested_audio_format.sample_rate(),
            plan.requested_audio_format.sample_format(),
            plan.requested_audio_format.input_channel_count(),
            1,
            plan.requested_audio_format.callback_frames(),
            plan.requested_audio_format.fallback_policy().clone(),
        )
        .unwrap();
        assert_eq!(
            PreparedSetupPlan::new(
                plan.device_intent.clone(),
                mismatched_format,
                plan.renderer.clone(),
                plan.dsp,
                plan.backend,
                CANONICAL_STAGES,
                CANONICAL_DEPENDENCIES,
            ),
            Err(SetupPlanningError::InvariantViolation {
                invariant: SetupPlanInvariant::FormatTopologyMismatch
            })
        );
    }
}
