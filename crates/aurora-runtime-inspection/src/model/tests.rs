use aurora_config::{
    AmbiguityPolicy, BackendIntent, DeviceDirection, DeviceSelectionIntent, ValidatedConfiguration,
};
use aurora_runtime_assembly::{
    prepare_runtime_plan, prepare_setup_plan, PreparedExecutionPlan, PreparedRoute,
    PreparedRoutingPlan, PreparedRuntimePlan, PreparedTopologyPlan, RuntimeCapacityPlan,
};

use super::*;

const STEREO: &[u8] = include_bytes!("../../../../fixtures/config/stereo-basic-v1.json");

fn plans_with_devices() -> (PreparedRuntimePlan, PreparedSetupPlan) {
    let mut config = ValidatedConfiguration::from_json(STEREO)
        .expect("fixture must validate")
        .config()
        .clone();
    config.input_device = Some(DeviceSelectionIntent {
        stable_id: Some("local-input-id".to_owned()),
        friendly_name: Some("Local Input".to_owned()),
        backend: BackendIntent::Cpal,
        direction: DeviceDirection::Input,
        ambiguity_policy: AmbiguityPolicy::RequireStableIdentifier,
    });
    config.output_device = Some(DeviceSelectionIntent {
        stable_id: Some("local-output-id".to_owned()),
        friendly_name: Some("Local Output".to_owned()),
        backend: BackendIntent::Virtual,
        direction: DeviceDirection::Output,
        ambiguity_policy: AmbiguityPolicy::Reject,
    });
    let validated = ValidatedConfiguration::new(config).expect("modified fixture must validate");
    let runtime = prepare_runtime_plan(&validated).expect("runtime derivation must succeed");
    let setup = prepare_setup_plan(&runtime).expect("setup derivation must succeed");
    (runtime, setup)
}

fn report(options: InspectionOptions) -> InspectionReport {
    let (runtime, setup) = plans_with_devices();
    InspectionReport::project(&runtime, &setup, options).expect("projection must succeed")
}

#[test]
fn equal_plans_and_options_produce_equal_reports() {
    let (runtime, setup) = plans_with_devices();
    let options = InspectionOptions::default();
    assert_eq!(
        InspectionReport::project(&runtime, &setup, options),
        InspectionReport::project(&runtime, &setup, options)
    );
}

#[test]
fn canonical_source_order_is_preserved() {
    let report = report(InspectionOptions::unredacted_local());
    let topology = &report.runtime().prepared_topology;
    assert_eq!(
        topology
            .outputs
            .iter()
            .map(|value| value.id.as_str())
            .collect::<Vec<_>>(),
        ["out-fl", "out-fr"]
    );
    assert_eq!(
        topology
            .routes
            .iter()
            .map(|value| (value.input_id.as_str(), value.output_id.as_str()))
            .collect::<Vec<_>>(),
        [("in-fl", "out-fl"), ("in-fr", "out-fr")]
    );
    assert_eq!(report.setup().stages.len(), MAX_SETUP_STAGES);
    assert_eq!(report.setup().dependencies.len(), MAX_SETUP_DEPENDENCIES);
    assert_eq!(
        report.setup().stages.last(),
        Some(&SetupStageProjection::SetupPlanComplete)
    );
}

#[test]
fn default_projection_redacts_without_erasing_presence() {
    let report = report(InspectionOptions::default());
    assert_eq!(report.redaction(), RedactionState::Redacted);
    let input = report.runtime().requested_devices.input.as_ref().unwrap();
    assert_eq!(
        input.stable_id.as_deref(),
        Some("<redacted:input-device-stable-id>")
    );
    assert_eq!(
        input.friendly_name.as_deref(),
        Some("<redacted:input-device-friendly-name>")
    );
    assert_eq!(
        report.runtime().prepared_topology.outputs[0].id,
        "<redacted:output-channel-id:0>"
    );
    assert_eq!(
        report.runtime().prepared_topology.outputs[0].label,
        "<redacted:output-channel-label:0>"
    );
    assert_eq!(
        report.runtime().prepared_topology.speakers[0].label,
        "<redacted:speaker-label:0>"
    );
    assert!(report
        .findings()
        .contains(&InspectionFinding::SensitiveIdentifiersRedacted));
}

#[test]
fn explicit_local_projection_retains_identifiers() {
    let report = report(InspectionOptions::unredacted_local());
    assert_eq!(report.redaction(), RedactionState::UnredactedLocal);
    let output = report.runtime().requested_devices.output.as_ref().unwrap();
    assert_eq!(output.stable_id.as_deref(), Some("local-output-id"));
    assert_eq!(output.friendly_name.as_deref(), Some("Local Output"));
    assert_eq!(report.runtime().prepared_topology.outputs[0].label, "FL");
}

#[test]
fn fact_semantics_exclude_unrepresented_runtime_evidence() {
    assert_eq!(
        report(InspectionOptions::default()).represented_semantics(),
        [
            InspectionFactSemantics::Requested,
            InspectionFactSemantics::Prepared,
            InspectionFactSemantics::Deferred,
        ]
    );
}

#[test]
fn oversized_source_string_returns_structured_error_without_truncation() {
    let (runtime, _) = plans_with_devices();
    let source_routing = runtime.topology().routing();
    let mut inputs = source_routing.inputs().to_vec();
    inputs[0] = aurora_runtime_assembly::PreparedChannelIdentity::new(
        "x".repeat(MAX_STRING_BYTES + 1),
        inputs[0].label(),
    )
    .unwrap();
    let routes = vec![
        PreparedRoute::new(inputs[0].id(), source_routing.outputs()[0].id()).unwrap(),
        source_routing.routes()[1].clone(),
    ];
    let routing = PreparedRoutingPlan::new(
        inputs,
        source_routing.outputs().to_vec(),
        routes,
        source_routing.inactive_outputs().to_vec(),
    )
    .unwrap();
    let oversized = rebuild_runtime(&runtime, routing);
    let setup = prepare_setup_plan(&oversized).unwrap();
    assert_eq!(
        InspectionReport::project(&oversized, &setup, InspectionOptions::default()),
        Err(InspectionError::LimitExceeded {
            limit: InspectionLimit::StringBytes,
            actual: MAX_STRING_BYTES + 1,
            maximum: MAX_STRING_BYTES,
        })
    );
}

#[test]
fn too_many_routes_returns_structured_error_without_silent_truncation() {
    let (runtime, _) = plans_with_devices();
    let source = runtime.topology().routing();
    let routes = (0..=MAX_ROUTES)
        .map(|_| PreparedRoute::new(source.inputs()[0].id(), source.outputs()[0].id()).unwrap())
        .collect::<Vec<_>>();
    let routing = PreparedRoutingPlan::new(
        source.inputs().to_vec(),
        source.outputs().to_vec(),
        routes,
        source.inactive_outputs().to_vec(),
    )
    .unwrap();
    let oversized = rebuild_runtime(&runtime, routing);
    let setup = prepare_setup_plan(&oversized).unwrap();
    assert_eq!(
        InspectionReport::project(&oversized, &setup, InspectionOptions::default()),
        Err(InspectionError::LimitExceeded {
            limit: InspectionLimit::Routes,
            actual: MAX_ROUTES + 1,
            maximum: MAX_ROUTES,
        })
    );
}

#[test]
fn mismatched_plan_pair_returns_structured_error() {
    let (runtime, _) = plans_with_devices();
    let other_runtime = prepare_runtime_plan(
        &ValidatedConfiguration::from_json(STEREO).expect("fixture must validate"),
    )
    .unwrap();
    let other_setup = prepare_setup_plan(&other_runtime).unwrap();
    assert_eq!(
        InspectionReport::project(&runtime, &other_setup, InspectionOptions::default()),
        Err(InspectionError::SourcePlansMismatch {
            relationship: PlanRelationship::DeviceIntent,
        })
    );
}

fn rebuild_runtime(
    source: &PreparedRuntimePlan,
    routing: PreparedRoutingPlan,
) -> PreparedRuntimePlan {
    let topology = PreparedTopologyPlan::new(routing, source.topology().layout().clone());
    let execution = PreparedExecutionPlan::new(
        source.execution().audio_format().clone(),
        source.execution().renderer().clone(),
        source.execution().dsp(),
    );
    let capacity = RuntimeCapacityPlan::new(
        topology.routing().inputs().len(),
        topology.routing().outputs().len(),
        topology.routing().routes().len(),
        topology.layout().speakers().len(),
        source.capacity().callback_frame_count(),
    )
    .unwrap();
    PreparedRuntimePlan::new(
        *source.metadata(),
        execution,
        topology,
        source.device_intent().clone(),
        capacity,
    )
    .unwrap()
}
