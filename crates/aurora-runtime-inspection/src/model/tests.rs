use aurora_config::{
    AmbiguityPolicy, CompatibleMinorRange, ComponentContractKind, ComponentReference,
    DeviceDirection, DeviceSelectionIntent, ValidatedConfiguration,
};
use aurora_runtime_assembly::{
    prepare_runtime_plan, prepare_setup_plan, PreparedAudioFormatIntent, PreparedChannelIdentity,
    PreparedDspPlan, PreparedExecutionPlan, PreparedLayoutKind, PreparedLayoutPlan, PreparedRoute,
    PreparedRoutingPlan, PreparedRuntimePlan, PreparedSpeaker, PreparedTopologyPlan,
    RuntimeCapacityPlan,
};

use super::*;
use crate::{
    JsonFormatter, TextFormatter, MAX_JSON_BYTES, MAX_SERIALIZED_COLLECTION_ENTRIES, MAX_TEXT_BYTES,
};

const STEREO: &[u8] = include_bytes!("../../../../fixtures/config/stereo-basic-v3.json");
const POINT: &[u8] = include_bytes!("../../../../fixtures/config/phase-3a-point-source-v3.json");
const SPREAD: &[u8] = include_bytes!("../../../../fixtures/config/phase-3b-spread-v3.json");
const IRREGULAR: &[u8] = include_bytes!("../../../../fixtures/config/irregular-horizontal-v3.json");

fn plans_with_devices() -> (PreparedRuntimePlan, PreparedSetupPlan) {
    let mut config = ValidatedConfiguration::from_json(STEREO)
        .expect("fixture must validate")
        .config()
        .clone();
    config.input_device = Some(DeviceSelectionIntent {
        stable_id: Some("local-input-id".to_owned()),
        friendly_name: Some("Local Input".to_owned()),
        backend: common_backend_reference("org.aurora.backend.cpal", DeviceDirection::Input),
        direction: DeviceDirection::Input,
        ambiguity_policy: AmbiguityPolicy::RequireStableIdentifier,
    });
    config.output_device = Some(DeviceSelectionIntent {
        stable_id: Some("local-output-id".to_owned()),
        friendly_name: Some("Local Output".to_owned()),
        backend: common_backend_reference("org.aurora.backend.virtual", DeviceDirection::Output),
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

fn fixture_report(bytes: &[u8], options: InspectionOptions) -> InspectionReport {
    let validated = ValidatedConfiguration::from_json(bytes).expect("fixture must validate");
    let runtime = prepare_runtime_plan(&validated).expect("runtime derivation must succeed");
    let setup = prepare_setup_plan(&runtime).expect("setup derivation must succeed");
    InspectionReport::project(&runtime, &setup, options).expect("projection must succeed")
}

fn common_backend_reference(component_id: &str, direction: DeviceDirection) -> ComponentReference {
    ComponentReference {
        component_id: component_id.to_owned(),
        contract_kind: match direction {
            DeviceDirection::Input => ComponentContractKind::AudioInputBackend,
            DeviceDirection::Output => ComponentContractKind::AudioOutputBackend,
        },
        contract_major: 1,
        compatible_minor: CompatibleMinorRange {
            minimum: 0,
            maximum: 0,
        },
        implementation_version_pin: None,
        configuration_schema: 1,
        configuration: serde_json::json!({}),
    }
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
fn prepared_component_identities_are_reported_as_control_plane_intent() {
    let report = report(InspectionOptions::default());
    assert_eq!(report.inspection_schema_version(), 4);
    assert_eq!(
        report
            .runtime()
            .prepared_components
            .renderer
            .implementation_id,
        "org.aurora.renderer.basic"
    );
    assert_eq!(
        report.runtime().prepared_components.renderer.contract_major,
        1
    );
    assert_eq!(
        report
            .runtime()
            .prepared_components
            .renderer
            .implementation_version,
        "0.1.0"
    );
    assert_eq!(
        report.runtime().prepared_components.renderer.contract_minor,
        0
    );
    assert_eq!(
        report
            .runtime()
            .prepared_components
            .realtime_delay
            .implementation_id,
        "org.aurora.dsp.basic-delay"
    );
    assert_eq!(
        report
            .runtime()
            .prepared_components
            .realtime_delay
            .contract_major,
        1
    );
    assert_eq!(
        report
            .runtime()
            .prepared_components
            .realtime_delay
            .implementation_version,
        "0.1.0"
    );
    assert_eq!(
        report
            .runtime()
            .prepared_components
            .realtime_delay
            .contract_minor,
        0
    );
}

#[test]
fn vbap_intent_uses_vbap_component_identity() {
    let report = fixture_report(POINT, InspectionOptions::default());
    assert_eq!(
        report
            .runtime()
            .prepared_components
            .renderer
            .implementation_id,
        "org.aurora.renderer.vbap"
    );
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

#[test]
fn exact_redacted_and_unredacted_json_fixtures_are_stable() {
    let redacted = JsonFormatter::format(&report(InspectionOptions::redacted())).unwrap();
    let unredacted = JsonFormatter::format(&report(InspectionOptions::unredacted_local())).unwrap();
    assert_eq!(
        redacted,
        include_str!("expected/stereo-redacted.json").trim_end()
    );
    assert_eq!(
        unredacted,
        include_str!("expected/stereo-unredacted.json").trim_end()
    );
}

#[test]
fn exact_redacted_and_unredacted_text_fixtures_are_stable() {
    let redacted = TextFormatter::format(&report(InspectionOptions::redacted())).unwrap();
    let unredacted = TextFormatter::format(&report(InspectionOptions::unredacted_local())).unwrap();
    assert_eq!(
        redacted,
        include_str!("expected/stereo-redacted.txt").replace("\r\n", "\n")
    );
    assert_eq!(
        unredacted,
        include_str!("expected/stereo-unredacted.txt").replace("\r\n", "\n")
    );
}

#[test]
fn repeated_formatting_is_byte_identical() {
    let report = report(InspectionOptions::default());
    let first_json = JsonFormatter::format(&report).unwrap();
    let first_text = TextFormatter::format(&report).unwrap();
    for _ in 0..3 {
        assert_eq!(JsonFormatter::format(&report).unwrap(), first_json);
        assert_eq!(TextFormatter::format(&report).unwrap(), first_text);
    }
}

#[test]
fn json_field_collection_enum_and_float_order_is_exact() {
    let spread = fixture_report(SPREAD, InspectionOptions::unredacted_local());
    let json = JsonFormatter::format(&spread).unwrap();
    let ordered = [
        "\"inspection_schema_version\"",
        "\"source_plan_schema_version\"",
        "\"runtime\"",
        "\"setup\"",
        "\"findings\"",
        "\"redaction\"",
        "\"represented_semantics\"",
    ];
    let positions = ordered
        .iter()
        .map(|field| json.find(field).expect("field must exist"))
        .collect::<Vec<_>>();
    assert!(positions.windows(2).all(|window| window[0] < window[1]));
    assert!(json.contains("\"kind\":\"horizontal_spread_vbap\""));
    assert!(json.contains("\"horizontal_spread\":0.0"));
    assert!(json.contains("\"normalized_x\":-0.5"));
    assert!(json.find("\"out-fl\"").unwrap() < json.find("\"out-fr\"").unwrap());
    for forbidden in [
        "\"negotiated\"",
        "\"observed\"",
        "\"simulated\"",
        "\"measured\"",
        "runtime_ready",
        "\"latency\"",
        "\"physical\"",
    ] {
        assert!(
            !json.contains(forbidden),
            "unexpected JSON vocabulary: {forbidden}"
        );
    }
}

#[test]
fn json_and_text_escaping_is_exact() {
    let special = "quote\" slash/ backslash\\ newline\n tab\t control\u{0001} unicode-\u{03bb}";
    let mut config = ValidatedConfiguration::from_json(STEREO)
        .expect("fixture must validate")
        .config()
        .clone();
    config.output_device = Some(DeviceSelectionIntent {
        stable_id: Some("device/special".to_owned()),
        friendly_name: Some(special.to_owned()),
        backend: common_backend_reference("org.aurora.backend.virtual", DeviceDirection::Output),
        direction: DeviceDirection::Output,
        ambiguity_policy: AmbiguityPolicy::Reject,
    });
    let validated = ValidatedConfiguration::new(config).expect("modified fixture must validate");
    let runtime = prepare_runtime_plan(&validated).expect("runtime derivation must succeed");
    let setup = prepare_setup_plan(&runtime).expect("setup derivation must succeed");
    let report = InspectionReport::project(&runtime, &setup, InspectionOptions::unredacted_local())
        .expect("projection must succeed");

    let escaped =
        "\"quote\\\" slash/ backslash\\\\ newline\\n tab\\t control\\u0001 unicode-\u{03bb}\"";
    let json = JsonFormatter::format(&report).expect("JSON formatting must succeed");
    let text = TextFormatter::format(&report).expect("text formatting must succeed");

    assert!(json.contains(&format!("\"friendly_name\":{escaped}")));
    assert!(text.contains(&format!("output_device.friendly_name: {escaped}")));
    assert!(json.contains("\"stable_id\":\"device/special\""));
    assert!(!json.contains("device\\/special"));
}

#[test]
fn text_sections_and_truth_wording_are_exact() {
    let text = TextFormatter::format(&report(InspectionOptions::default())).unwrap();
    let sections = [
        "[requested]",
        "[prepared]",
        "[deferred]",
        "[setup]",
        "[findings]",
    ];
    let positions = sections
        .iter()
        .map(|section| text.find(section).expect("section must exist"))
        .collect::<Vec<_>>();
    assert!(positions.windows(2).all(|window| window[0] < window[1]));
    assert!(text.contains("redaction_state: redacted"));
    assert!(text.contains("represented_semantics: requested, prepared, deferred"));
    assert!(text.contains("evidence_disclaimer: no negotiated, observed, simulated, measured, runtime-ready, latency, or physical evidence"));
}

#[test]
fn every_renderer_and_custom_layout_format_deterministically() {
    for (bytes, expected_renderer) in [
        (STEREO, "basic_inverse_distance"),
        (POINT, "point_source_horizontal_vbap"),
        (SPREAD, "horizontal_spread_vbap"),
    ] {
        let report = fixture_report(bytes, InspectionOptions::default());
        assert!(JsonFormatter::format(&report)
            .unwrap()
            .contains(expected_renderer));
        assert!(TextFormatter::format(&report)
            .unwrap()
            .contains(expected_renderer));
    }
    let custom = fixture_report(IRREGULAR, InspectionOptions::default());
    assert!(custom
        .findings()
        .contains(&InspectionFinding::CustomSpeakerRolesPresent));
    assert!(JsonFormatter::format(&custom)
        .unwrap()
        .contains("CustomHorizontal"));
}

#[test]
fn inactive_outputs_and_both_dsp_states_produce_fixed_findings() {
    let mut config = ValidatedConfiguration::from_json(STEREO)
        .unwrap()
        .config()
        .clone();
    config.routing.inputs.clear();
    config.routing.routes.clear();
    config.routing.inactive_outputs = vec!["out-fl".to_owned(), "out-fr".to_owned()];
    let runtime = prepare_runtime_plan(&ValidatedConfiguration::new(config).unwrap()).unwrap();
    let setup = prepare_setup_plan(&runtime).unwrap();
    let inactive =
        InspectionReport::project(&runtime, &setup, InspectionOptions::default()).unwrap();
    assert!(inactive
        .findings()
        .contains(&InspectionFinding::InactiveOutputsPresent));
    assert!(inactive.findings().contains(&InspectionFinding::DspAbsent));

    let deferred_runtime =
        rebuild_runtime_with_dsp(&runtime, PreparedDspPlan::DeferredByCurrentSchema);
    let deferred_setup = prepare_setup_plan(&deferred_runtime).unwrap();
    let deferred = InspectionReport::project(
        &deferred_runtime,
        &deferred_setup,
        InspectionOptions::default(),
    )
    .unwrap();
    assert!(deferred
        .findings()
        .contains(&InspectionFinding::DspSetupDeferredByCurrentSchema));
}

#[test]
fn maximum_upstream_shape_formats_within_published_bounds() {
    let report = maximum_report();
    let json = JsonFormatter::format(&report).unwrap();
    let text = TextFormatter::format(&report).unwrap();
    assert!(json.len() <= MAX_JSON_BYTES);
    assert!(text.len() <= MAX_TEXT_BYTES);
    assert_eq!(
        report.runtime().prepared_topology.inputs.len(),
        MAX_CHANNELS
    );
    assert_eq!(
        report.runtime().prepared_topology.outputs.len(),
        MAX_CHANNELS
    );
    assert_eq!(report.runtime().prepared_topology.routes.len(), MAX_ROUTES);
    assert_eq!(
        report.runtime().prepared_topology.speakers.len(),
        MAX_SPEAKERS
    );
    assert_eq!(
        report
            .runtime()
            .prepared_topology
            .speakers
            .iter()
            .filter(|speaker| !speaker.active)
            .count(),
        MAX_SPEAKERS / 2
    );
}

#[test]
fn formatter_bounds_fail_without_returning_truncated_output() {
    let mut json_report = report(InspectionOptions::unredacted_local());
    json_report.runtime.metadata.evidence = "x".repeat(MAX_JSON_BYTES);
    assert!(matches!(
        JsonFormatter::format(&json_report),
        Err(InspectionError::SerializedOutputTooLarge {
            format: crate::InspectionOutputFormat::Json,
            ..
        })
    ));

    let mut text_report = report(InspectionOptions::unredacted_local());
    text_report.runtime.metadata.evidence = "x".repeat(MAX_TEXT_BYTES);
    assert!(matches!(
        TextFormatter::format(&text_report),
        Err(InspectionError::SerializedOutputTooLarge {
            format: crate::InspectionOutputFormat::Text,
            ..
        })
    ));

    let mut collection_report = report(InspectionOptions::default());
    collection_report.findings =
        vec![InspectionFinding::DspAbsent; MAX_SERIALIZED_COLLECTION_ENTRIES];
    assert!(matches!(
        JsonFormatter::format(&collection_report),
        Err(InspectionError::SerializedCollectionEntriesExceeded { .. })
    ));
}

#[test]
fn non_finite_projection_copy_is_rejected_by_both_formatters() {
    let mut report = report(InspectionOptions::default());
    report.runtime.prepared_topology.speakers[0].normalized_x = f32::NAN;
    let expected = InspectionError::NonFiniteFloat {
        field: crate::InspectionFloatField::SpeakerNormalizedX,
    };
    assert_eq!(JsonFormatter::format(&report), Err(expected.clone()));
    assert_eq!(TextFormatter::format(&report), Err(expected));
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

fn rebuild_runtime_with_dsp(
    source: &PreparedRuntimePlan,
    dsp: PreparedDspPlan,
) -> PreparedRuntimePlan {
    let execution = PreparedExecutionPlan::new(
        source.execution().audio_format().clone(),
        source.execution().renderer().clone(),
        dsp,
    );
    PreparedRuntimePlan::new(
        *source.metadata(),
        execution,
        source.topology().clone(),
        source.device_intent().clone(),
        source.capacity().clone(),
    )
    .unwrap()
}

fn maximum_report() -> InspectionReport {
    let (source, _) = plans_with_devices();
    let input = source.topology().routing().inputs()[0].clone();
    let output = source.topology().routing().outputs()[0].clone();
    let speaker = source.topology().layout().speakers()[0].clone();
    let inputs = (0..MAX_CHANNELS)
        .map(|index| PreparedChannelIdentity::new(format!("input-{index}"), input.label()).unwrap())
        .collect::<Vec<_>>();
    let outputs = (0..MAX_CHANNELS)
        .map(|index| {
            PreparedChannelIdentity::new(format!("output-{index}"), output.label()).unwrap()
        })
        .collect::<Vec<_>>();
    let routes = (0..MAX_ROUTES)
        .map(|index| {
            PreparedRoute::new(
                inputs[index % MAX_CHANNELS].id(),
                outputs[index % MAX_CHANNELS].id(),
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let inactive = outputs
        .iter()
        .map(|value| value.id().to_owned())
        .collect::<Vec<_>>();
    let routing = PreparedRoutingPlan::new(inputs, outputs, routes, inactive).unwrap();
    let speakers = (0..MAX_SPEAKERS)
        .map(|index| {
            PreparedSpeaker::new(
                format!("speaker-{index}"),
                speaker.label(),
                speaker.channel_role().clone(),
                speaker.position(),
                index % 2 == 0,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let layout = PreparedLayoutPlan::new(PreparedLayoutKind::CustomHorizontal, speakers).unwrap();
    let topology = PreparedTopologyPlan::new(routing, layout);
    let source_format = source.execution().audio_format();
    let format = PreparedAudioFormatIntent::new(
        source_format.sample_rate(),
        source_format.sample_format(),
        MAX_CHANNELS,
        MAX_CHANNELS,
        source_format.callback_frames(),
        source_format.fallback_policy().clone(),
    )
    .unwrap();
    let execution = PreparedExecutionPlan::new(
        format,
        source.execution().renderer().clone(),
        source.execution().dsp(),
    );
    let capacity = RuntimeCapacityPlan::new(
        MAX_CHANNELS,
        MAX_CHANNELS,
        MAX_ROUTES,
        MAX_SPEAKERS,
        source.capacity().callback_frame_count(),
    )
    .unwrap();
    let runtime = PreparedRuntimePlan::new(
        *source.metadata(),
        execution,
        topology,
        source.device_intent().clone(),
        capacity,
    )
    .unwrap();
    let setup = prepare_setup_plan(&runtime).unwrap();
    InspectionReport::project(&runtime, &setup, InspectionOptions::unredacted_local()).unwrap()
}
