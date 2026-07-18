use aurora_config::{
    AmbiguityPolicy, BackendIntent, DeviceDirection, DeviceSelectionIntent, ValidatedConfiguration,
};
use aurora_runtime_assembly::{
    prepare_runtime_plan, prepare_setup_plan, PreparedDspPlan, PreparedRuntimePlan,
    RuntimeCapacityPlan, RuntimeInvariant, RuntimePreparationError, SetupStage,
};

const STEREO: &[u8] = include_bytes!("../../../fixtures/config/stereo-basic-v1.json");
const FIVE_ONE: &[u8] = include_bytes!("../../../fixtures/config/surround-5-1-v1.json");
const SEVEN_ONE: &[u8] = include_bytes!("../../../fixtures/config/surround-7-1-v1.json");
const SPREAD: &[u8] = include_bytes!("../../../fixtures/config/phase-3b-spread-v1.json");

fn validated(bytes: &[u8]) -> ValidatedConfiguration {
    ValidatedConfiguration::from_json(bytes).unwrap()
}

fn runtime(bytes: &[u8]) -> PreparedRuntimePlan {
    prepare_runtime_plan(&validated(bytes)).unwrap()
}

#[test]
fn equal_semantic_inputs_produce_equal_runtime_and_setup_plans() {
    let first = validated(STEREO);
    let mut reordered = first.config().clone();
    reordered.routing.inputs.reverse();
    reordered.routing.outputs.reverse();
    reordered.routing.routes.reverse();
    reordered.speaker_layout.speakers.reverse();
    let second = ValidatedConfiguration::new(reordered).unwrap();

    assert!(first.semantically_eq(&second));
    let first_runtime = prepare_runtime_plan(&first).unwrap();
    let second_runtime = prepare_runtime_plan(&second).unwrap();
    assert_eq!(first_runtime, second_runtime);
    assert_eq!(
        prepare_setup_plan(&first_runtime),
        prepare_setup_plan(&second_runtime)
    );
}

#[test]
fn setup_stage_order_and_dependency_graph_are_complete_and_acyclic() {
    let setup = prepare_setup_plan(&runtime(STEREO)).unwrap();
    let expected = [
        SetupStage::DeviceSelectionIntent,
        SetupStage::RequestedFormatPlanning,
        SetupStage::RendererPreparationIntent,
        SetupStage::DspPreparationIntent,
        SetupStage::BackendPreparationIntent,
        SetupStage::SetupPlanComplete,
    ];
    assert_eq!(setup.stages(), expected);
    assert_eq!(setup.dependencies().len(), 9);

    for dependency in setup.dependencies() {
        let prerequisite = setup
            .stages()
            .iter()
            .position(|stage| *stage == dependency.depends_on())
            .unwrap();
        let dependent = setup
            .stages()
            .iter()
            .position(|stage| *stage == dependency.stage())
            .unwrap();
        assert!(prerequisite < dependent);
    }
    assert_eq!(
        setup
            .dependencies()
            .iter()
            .filter(|dependency| dependency.stage() == SetupStage::SetupPlanComplete)
            .count(),
        expected.len() - 1
    );
}

#[test]
fn requested_device_format_and_backend_values_remain_unresolved_intent() {
    let mut config = validated(STEREO).config().clone();
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
    let prepared = prepare_runtime_plan(&ValidatedConfiguration::new(config).unwrap()).unwrap();
    let setup = prepare_setup_plan(&prepared).unwrap();

    assert_eq!(
        setup.device_intent().input().unwrap().stable_id(),
        Some("requested-input")
    );
    assert_eq!(
        setup.device_intent().output().unwrap().friendly_name(),
        Some("requested-output")
    );
    assert_eq!(
        setup.backend().requested_input_backend(),
        Some(BackendIntent::Cpal)
    );
    assert_eq!(
        setup.backend().requested_output_backend(),
        Some(BackendIntent::Virtual)
    );
    assert_eq!(
        setup.requested_audio_format(),
        prepared.execution().audio_format()
    );
}

#[test]
fn absent_device_and_dsp_configuration_remain_absent() {
    let setup = prepare_setup_plan(&runtime(STEREO)).unwrap();
    assert!(setup.device_intent().input().is_none());
    assert!(setup.device_intent().output().is_none());
    assert_eq!(setup.backend().requested_input_backend(), None);
    assert_eq!(setup.backend().requested_output_backend(), None);
    assert_eq!(setup.dsp().state(), PreparedDspPlan::None);
}

#[test]
fn inconsistent_public_runtime_components_return_a_typed_invariant_error() {
    let plan = runtime(STEREO);
    let invalid_capacity = RuntimeCapacityPlan::new(
        plan.capacity().input_channel_count(),
        plan.capacity().output_channel_count(),
        plan.capacity().route_count() + 1,
        plan.capacity().speaker_count(),
        plan.capacity().callback_frame_count(),
    )
    .unwrap();

    assert_eq!(
        PreparedRuntimePlan::new(
            *plan.metadata(),
            plan.execution().clone(),
            plan.topology().clone(),
            plan.device_intent().clone(),
            invalid_capacity,
        ),
        Err(RuntimePreparationError::InternalInvariantViolation {
            invariant: RuntimeInvariant::ComponentCapacityMismatch,
        })
    );
}

#[test]
fn every_supported_fixture_prepares_without_panic_or_external_state() {
    for fixture in [STEREO, FIVE_ONE, SEVEN_ONE, SPREAD] {
        let prepared = runtime(fixture);
        let setup = prepare_setup_plan(&prepared).unwrap();
        assert_eq!(setup.stages().last(), Some(&SetupStage::SetupPlanComplete));
    }
}
