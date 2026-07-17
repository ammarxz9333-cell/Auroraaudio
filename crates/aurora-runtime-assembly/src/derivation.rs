use aurora_config::{
    ChannelIdentity, DeviceDirection, DeviceSelectionIntent, LayoutKind, RendererConfiguration,
    RoutingConfiguration, SpeakerConfiguration, SpeakerLayoutConfiguration, ValidatedConfiguration,
};
use aurora_core::{ChannelRole, StandardLayout, Vector3};

use crate::{
    ArithmeticOperation, PreparedAudioFormatIntent, PreparedChannelIdentity, PreparedDeviceIntent,
    PreparedDeviceSelectorIntent, PreparedDspPlan, PreparedExecutionPlan, PreparedLayoutKind,
    PreparedLayoutPlan, PreparedRendererPlan, PreparedRoute, PreparedRoutingPlan,
    PreparedRuntimePlan, PreparedSpeaker, PreparedTopologyPlan, RuntimeCapacityPlan,
    RuntimeInvariant, RuntimePlanMetadata, RuntimePreparationError,
};

/// Derives an immutable runtime preparation plan from validated configuration.
///
/// The function reads only [`ValidatedConfiguration::config`], preserves its
/// normalized vector order, and performs no I/O, probing, negotiation, runtime
/// construction, time access, randomness, or global-state mutation.
///
/// Input channels come from `routing.inputs`, while output channels come from
/// `routing.outputs`. Accepted configuration validation guarantees that the
/// latter equals `audio_format.channel_count`; the equality is checked as a
/// runtime-assembly invariant rather than silently substituted.
pub fn prepare_runtime_plan(
    configuration: &ValidatedConfiguration,
) -> Result<PreparedRuntimePlan, RuntimePreparationError> {
    let config = configuration.config();
    let routing = prepare_routing(&config.routing)?;
    let input_channel_count = routing.inputs().len();
    let output_channel_count = routing.outputs().len();

    if usize::from(config.audio_format.channel_count) != output_channel_count {
        return Err(RuntimePreparationError::InternalInvariantViolation {
            invariant: RuntimeInvariant::AudioOutputCountMismatch,
        });
    }

    let callback_frame_count =
        usize::try_from(config.audio_format.callback_frames).map_err(|_| {
            RuntimePreparationError::ArithmeticOverflow {
                operation: ArithmeticOperation::CallbackFrameConversion,
            }
        })?;
    let layout = prepare_layout(&config.speaker_layout)?;
    let route_count = routing.routes().len();
    let speaker_count = layout.speakers().len();
    let format = PreparedAudioFormatIntent::new(
        config.audio_format.sample_rate,
        config.audio_format.sample_format,
        input_channel_count,
        output_channel_count,
        config.audio_format.callback_frames,
        config.audio_format.fallback_policy.clone(),
    )?;
    let renderer = prepare_renderer(&config.renderer)?;
    let device_intent = PreparedDeviceIntent::new(
        prepare_device_selector(config.input_device.as_ref(), DeviceDirection::Input)?,
        prepare_device_selector(config.output_device.as_ref(), DeviceDirection::Output)?,
    );
    let capacity = RuntimeCapacityPlan::new(
        input_channel_count,
        output_channel_count,
        route_count,
        speaker_count,
        callback_frame_count,
    )?;

    PreparedRuntimePlan::new(
        RuntimePlanMetadata::new(config.schema.schema_version),
        PreparedExecutionPlan::new(format, renderer, PreparedDspPlan::None),
        PreparedTopologyPlan::new(routing, layout),
        device_intent,
        capacity,
    )
}

fn prepare_routing(
    routing: &RoutingConfiguration,
) -> Result<PreparedRoutingPlan, RuntimePreparationError> {
    let inputs = routing
        .inputs
        .iter()
        .map(prepare_channel_identity)
        .collect::<Result<Vec<_>, _>>()?;
    let outputs = routing
        .outputs
        .iter()
        .map(prepare_channel_identity)
        .collect::<Result<Vec<_>, _>>()?;
    let routes = routing
        .routes
        .iter()
        .map(|route| PreparedRoute::new(route.input.clone(), route.output.clone()))
        .collect::<Result<Vec<_>, _>>()?;

    PreparedRoutingPlan::new(inputs, outputs, routes, routing.inactive_outputs.clone())
}

fn prepare_channel_identity(
    identity: &ChannelIdentity,
) -> Result<PreparedChannelIdentity, RuntimePreparationError> {
    PreparedChannelIdentity::new(identity.id.clone(), identity.label.clone())
}

fn prepare_layout(
    layout: &SpeakerLayoutConfiguration,
) -> Result<PreparedLayoutPlan, RuntimePreparationError> {
    let kind = match layout.kind {
        LayoutKind::Stereo => PreparedLayoutKind::Standard(StandardLayout::Stereo),
        LayoutKind::Surround51 => PreparedLayoutKind::Standard(StandardLayout::FiveOne),
        LayoutKind::Surround71 => PreparedLayoutKind::Standard(StandardLayout::SevenOne),
        LayoutKind::CustomHorizontal => PreparedLayoutKind::CustomHorizontal,
    };
    let speakers = layout
        .speakers
        .iter()
        .map(prepare_speaker)
        .collect::<Result<Vec<_>, _>>()?;

    PreparedLayoutPlan::new(kind, speakers)
}

fn prepare_speaker(
    speaker: &SpeakerConfiguration,
) -> Result<PreparedSpeaker, RuntimePreparationError> {
    PreparedSpeaker::new(
        speaker.id.clone(),
        speaker.label.clone(),
        prepare_role(&speaker.role),
        horizontal_unit_direction(speaker.azimuth_degrees)?,
        speaker.active,
    )
}

fn prepare_role(role: &str) -> ChannelRole {
    match role {
        "FL" | "front-left" => ChannelRole::FrontLeft,
        "FR" | "front-right" => ChannelRole::FrontRight,
        "FC" | "front-center" => ChannelRole::FrontCenter,
        "LFE" | "lfe" | "low-frequency-effects" => ChannelRole::LowFrequencyEffects,
        "SL" | "surround-left" => ChannelRole::SurroundLeft,
        "SR" | "surround-right" => ChannelRole::SurroundRight,
        "SBL" | "surround-back-left" => ChannelRole::SurroundBackLeft,
        "SBR" | "surround-back-right" => ChannelRole::SurroundBackRight,
        "TFL" | "top-front-left" => ChannelRole::TopFrontLeft,
        "TFR" | "top-front-right" => ChannelRole::TopFrontRight,
        custom => ChannelRole::Custom(custom.to_owned()),
    }
}

/// Converts clockwise-positive azimuth into a horizontal unit direction.
///
/// Zero degrees is front (`+Y`), positive 90 degrees is right (`+X`), negative
/// 90 degrees is left (`-X`), and either 180-degree representation is rear
/// (`-Y`). The accepted schema's elevation value is reserved metadata and does
/// not activate elevation rendering, so `Z` remains zero. The vector is
/// dimensionless and carries no radius or physical distance.
fn horizontal_unit_direction(azimuth_degrees: f32) -> Result<Vector3, RuntimePreparationError> {
    if !azimuth_degrees.is_finite() {
        return Err(RuntimePreparationError::InternalInvariantViolation {
            invariant: RuntimeInvariant::NonFiniteSpeakerGeometry,
        });
    }
    let radians = azimuth_degrees.to_radians();
    let (sin, cos) = radians.sin_cos();
    Ok(Vector3::new(canonical_axis(sin), canonical_axis(cos), 0.0))
}

fn canonical_axis(value: f32) -> f32 {
    if value.abs() < 1.0e-6 {
        0.0
    } else if (value - 1.0).abs() < 1.0e-6 {
        1.0
    } else if (value + 1.0).abs() < 1.0e-6 {
        -1.0
    } else {
        value
    }
}

fn prepare_renderer(
    renderer: &RendererConfiguration,
) -> Result<PreparedRendererPlan, RuntimePreparationError> {
    match renderer {
        RendererConfiguration::Basic => Ok(PreparedRendererPlan::basic_inverse_distance()),
        RendererConfiguration::PointSourceVbap => {
            Ok(PreparedRendererPlan::point_source_horizontal_vbap())
        }
        RendererConfiguration::HorizontalSpread { spread } => {
            PreparedRendererPlan::horizontal_spread_vbap(*spread)
        }
        RendererConfiguration::Unsupported => {
            Err(RuntimePreparationError::UnsupportedRendererIntent)
        }
    }
}

fn prepare_device_selector(
    selector: Option<&DeviceSelectionIntent>,
    expected_direction: DeviceDirection,
) -> Result<Option<PreparedDeviceSelectorIntent>, RuntimePreparationError> {
    let Some(selector) = selector else {
        return Ok(None);
    };
    if selector.direction != expected_direction {
        let invariant = match expected_direction {
            DeviceDirection::Input => RuntimeInvariant::InputDeviceDirectionMismatch,
            DeviceDirection::Output => RuntimeInvariant::OutputDeviceDirectionMismatch,
        };
        return Err(RuntimePreparationError::InternalInvariantViolation { invariant });
    }

    PreparedDeviceSelectorIntent::new(
        selector.stable_id.clone(),
        selector.friendly_name.clone(),
        selector.backend,
        selector.ambiguity_policy,
    )
    .map(Some)
}

#[cfg(test)]
mod tests {
    use aurora_config::{
        AmbiguityPolicy, BackendIntent, DeviceDirection, DeviceSelectionIntent,
        FormatFallbackPolicy, RendererConfiguration, SampleFormatIntent, ValidatedConfiguration,
    };

    use super::*;
    use crate::{
        PreparedDspPlan, PreparedLayoutKind, PreparedRendererKind, RuntimePlanEvidence,
        SetupDerivedCapacity, RUNTIME_PLAN_CONTRACT_VERSION,
    };

    const STEREO: &[u8] = include_bytes!("../../../fixtures/config/stereo-basic-v1.json");
    const FIVE_ONE: &[u8] = include_bytes!("../../../fixtures/config/surround-5-1-v1.json");
    const SEVEN_ONE: &[u8] = include_bytes!("../../../fixtures/config/surround-7-1-v1.json");
    const POINT: &[u8] = include_bytes!("../../../fixtures/config/phase-3a-point-source-v1.json");
    const SPREAD: &[u8] = include_bytes!("../../../fixtures/config/phase-3b-spread-v1.json");
    const IRREGULAR: &[u8] =
        include_bytes!("../../../fixtures/config/irregular-horizontal-v1.json");

    fn validated(bytes: &[u8]) -> ValidatedConfiguration {
        ValidatedConfiguration::from_json(bytes).unwrap()
    }

    fn plan(bytes: &[u8]) -> PreparedRuntimePlan {
        prepare_runtime_plan(&validated(bytes)).unwrap()
    }

    #[test]
    fn stereo_configuration_derives_expected_plan() {
        let plan = plan(STEREO);
        assert_eq!(plan.execution().audio_format().sample_rate(), 48_000);
        assert_eq!(plan.execution().audio_format().input_channel_count(), 2);
        assert_eq!(plan.execution().audio_format().output_channel_count(), 2);
        assert_eq!(
            plan.execution().audio_format().sample_format(),
            SampleFormatIntent::Float32
        );
        assert_eq!(plan.execution().audio_format().callback_frames(), 256);
        assert_eq!(
            plan.execution().audio_format().fallback_policy(),
            &FormatFallbackPolicy::Reject
        );
        assert_eq!(
            plan.topology().layout().kind(),
            PreparedLayoutKind::Standard(StandardLayout::Stereo)
        );
        assert_eq!(plan.execution().dsp(), PreparedDspPlan::None);
    }

    #[test]
    fn five_one_derives_expected_layout_and_roles() {
        let plan = plan(FIVE_ONE);
        assert_eq!(
            plan.topology().layout().kind(),
            PreparedLayoutKind::Standard(StandardLayout::FiveOne)
        );
        let roles = plan
            .topology()
            .layout()
            .speakers()
            .iter()
            .map(PreparedSpeaker::channel_role)
            .collect::<Vec<_>>();
        for role in StandardLayout::FiveOne.canonical_roles() {
            assert!(roles.contains(&role));
        }
    }

    #[test]
    fn seven_one_derives_expected_layout_and_roles() {
        let plan = plan(SEVEN_ONE);
        assert_eq!(
            plan.topology().layout().kind(),
            PreparedLayoutKind::Standard(StandardLayout::SevenOne)
        );
        let roles = plan
            .topology()
            .layout()
            .speakers()
            .iter()
            .map(PreparedSpeaker::channel_role)
            .collect::<Vec<_>>();
        for role in StandardLayout::SevenOne.canonical_roles() {
            assert!(roles.contains(&role));
        }
    }

    #[test]
    fn custom_horizontal_preserves_normalized_speaker_order_and_roles() {
        let plan = plan(IRREGULAR);
        let speakers = plan.topology().layout().speakers();
        assert_eq!(
            speakers.iter().map(PreparedSpeaker::id).collect::<Vec<_>>(),
            vec!["a", "b", "c", "d"]
        );
        assert_eq!(
            speakers[0].channel_role(),
            &ChannelRole::Custom("west".to_owned())
        );
        assert_eq!(
            speakers[3].channel_role(),
            &ChannelRole::Custom("south-east".to_owned())
        );
    }

    #[test]
    fn inactive_speakers_are_preserved_without_filtering() {
        let mut config = validated(STEREO).config().clone();
        config.speaker_layout.speakers[0].active = false;
        config.speaker_layout.kind = LayoutKind::CustomHorizontal;
        let validated = ValidatedConfiguration::new(config).unwrap();
        let plan = prepare_runtime_plan(&validated).unwrap();
        assert_eq!(plan.topology().layout().speakers().len(), 2);
        assert!(!plan.topology().layout().speakers()[0].is_active());
        assert!(plan.topology().layout().speakers()[1].is_active());
    }

    #[test]
    fn routing_and_inactive_output_order_are_preserved() {
        let mut config = validated(STEREO).config().clone();
        config.routing.inputs.clear();
        config.routing.routes.clear();
        config.routing.inactive_outputs = vec!["out-fr".to_owned(), "out-fl".to_owned()];
        let validated = ValidatedConfiguration::new(config).unwrap();
        let plan = prepare_runtime_plan(&validated).unwrap();
        let routing = plan.topology().routing();
        assert!(routing.inputs().is_empty());
        assert!(routing.routes().is_empty());
        assert_eq!(
            routing
                .outputs()
                .iter()
                .map(PreparedChannelIdentity::id)
                .collect::<Vec<_>>(),
            vec!["out-fl", "out-fr"]
        );
        assert_eq!(
            routing.inactive_outputs(),
            &["out-fl".to_owned(), "out-fr".to_owned()]
        );
    }

    #[test]
    fn renderer_intents_map_without_construction() {
        assert_eq!(
            plan(STEREO).execution().renderer().kind(),
            PreparedRendererKind::BasicInverseDistance
        );
        assert_eq!(
            plan(POINT).execution().renderer().kind(),
            PreparedRendererKind::PointSourceHorizontalVbap
        );
        let spread = plan(SPREAD);
        assert_eq!(
            spread.execution().renderer().kind(),
            PreparedRendererKind::HorizontalSpreadVbap
        );
        assert_eq!(spread.execution().renderer().horizontal_spread(), Some(0.0));
    }

    #[test]
    fn device_selectors_remain_unresolved_intent() {
        let mut config = validated(STEREO).config().clone();
        config.input_device = Some(DeviceSelectionIntent {
            stable_id: Some("input-1".to_owned()),
            friendly_name: Some("Requested Input".to_owned()),
            backend: BackendIntent::Cpal,
            direction: DeviceDirection::Input,
            ambiguity_policy: AmbiguityPolicy::RequireStableIdentifier,
        });
        config.output_device = Some(DeviceSelectionIntent {
            stable_id: None,
            friendly_name: Some("Requested Output".to_owned()),
            backend: BackendIntent::Virtual,
            direction: DeviceDirection::Output,
            ambiguity_policy: AmbiguityPolicy::Reject,
        });
        let plan = prepare_runtime_plan(&ValidatedConfiguration::new(config).unwrap()).unwrap();
        assert_eq!(
            plan.device_intent().input().unwrap().stable_id(),
            Some("input-1")
        );
        assert_eq!(
            plan.device_intent().output().unwrap().friendly_name(),
            Some("Requested Output")
        );
        assert_eq!(
            plan.device_intent().output().unwrap().backend(),
            BackendIntent::Virtual
        );
    }

    #[test]
    fn metadata_and_capacities_use_authoritative_sources() {
        let plan = plan(SEVEN_ONE);
        assert_eq!(plan.metadata().configuration_schema_version(), 1);
        assert_eq!(
            plan.metadata().contract_version(),
            RUNTIME_PLAN_CONTRACT_VERSION
        );
        assert_eq!(
            plan.metadata().evidence(),
            RuntimePlanEvidence::DeterministicControlPlaneIntent
        );
        assert_eq!(plan.capacity().input_channel_count(), 8);
        assert_eq!(plan.capacity().output_channel_count(), 8);
        assert_eq!(plan.capacity().route_count(), 8);
        assert_eq!(plan.capacity().speaker_count(), 8);
        assert_eq!(plan.capacity().callback_frame_count(), 256);
        assert_eq!(plan.capacity().renderer_output_gain_width(), 8);
    }

    #[test]
    fn setup_derived_capacities_remain_deferred() {
        let capacity = plan(STEREO).capacity().clone();
        let deferred = SetupDerivedCapacity::DeferredUntilSetup;
        assert_eq!(capacity.renderer_scratch(), deferred);
        assert_eq!(capacity.renderer_history(), deferred);
        assert_eq!(capacity.implementation_temporary(), deferred);
        assert_eq!(capacity.delay_processor(), deferred);
        assert_eq!(capacity.asrc(), deferred);
        assert_eq!(capacity.backend_ring(), deferred);
    }

    #[test]
    fn repeated_and_semantically_equal_derivations_are_equal() {
        let first = validated(STEREO);
        let mut reordered = first.config().clone();
        reordered.routing.inputs.reverse();
        reordered.routing.outputs.reverse();
        reordered.routing.routes.reverse();
        reordered.speaker_layout.speakers.reverse();
        let second = ValidatedConfiguration::new(reordered).unwrap();
        assert!(first.semantically_eq(&second));
        assert_eq!(prepare_runtime_plan(&first), prepare_runtime_plan(&first));
        assert_eq!(prepare_runtime_plan(&first), prepare_runtime_plan(&second));
    }

    #[test]
    fn generated_by_provenance_does_not_change_plan() {
        let first = validated(STEREO);
        let mut different_provenance = first.config().clone();
        different_provenance.schema.generated_by = Some("checkpoint-b-test".to_owned());
        let second = ValidatedConfiguration::new(different_provenance).unwrap();
        assert!(first.semantically_eq(&second));
        assert_eq!(prepare_runtime_plan(&first), prepare_runtime_plan(&second));
    }

    #[test]
    fn unsupported_renderer_returns_structured_error() {
        assert_eq!(
            prepare_renderer(&RendererConfiguration::Unsupported),
            Err(RuntimePreparationError::UnsupportedRendererIntent)
        );
    }

    #[test]
    fn mismatched_device_direction_returns_structured_invariant() {
        let mut config = validated(STEREO).config().clone();
        config.input_device = Some(DeviceSelectionIntent {
            stable_id: Some("wrong-direction".to_owned()),
            friendly_name: None,
            backend: BackendIntent::Virtual,
            direction: DeviceDirection::Output,
            ambiguity_policy: AmbiguityPolicy::Reject,
        });
        let validated = ValidatedConfiguration::new(config).unwrap();
        assert_eq!(
            prepare_runtime_plan(&validated),
            Err(RuntimePreparationError::InternalInvariantViolation {
                invariant: RuntimeInvariant::InputDeviceDirectionMismatch
            })
        );
    }

    #[test]
    fn cardinal_azimuths_follow_documented_unit_direction_convention() {
        assert_eq!(
            horizontal_unit_direction(0.0),
            Ok(Vector3::new(0.0, 1.0, 0.0))
        );
        assert_eq!(
            horizontal_unit_direction(90.0),
            Ok(Vector3::new(1.0, 0.0, 0.0))
        );
        assert_eq!(
            horizontal_unit_direction(-90.0),
            Ok(Vector3::new(-1.0, 0.0, 0.0))
        );
        assert_eq!(
            horizontal_unit_direction(180.0),
            Ok(Vector3::new(0.0, -1.0, 0.0))
        );
        assert_eq!(
            horizontal_unit_direction(-180.0),
            Ok(Vector3::new(0.0, -1.0, 0.0))
        );
    }

    #[test]
    fn prepared_geometry_is_direction_only_and_ignores_reserved_elevation_metadata() {
        let mut config = validated(STEREO).config().clone();
        config.speaker_layout.speakers[0].azimuth_degrees = 90.0;
        config.speaker_layout.speakers[0].elevation_degrees = Some(45.0);
        let plan = prepare_runtime_plan(&ValidatedConfiguration::new(config).unwrap()).unwrap();
        assert_eq!(
            plan.topology().layout().speakers()[0].position(),
            Vector3::new(1.0, 0.0, 0.0)
        );
    }

    #[test]
    fn output_only_validated_configuration_preserves_zero_input_and_route_counts() {
        let mut config = validated(STEREO).config().clone();
        config.routing.inputs.clear();
        config.routing.routes.clear();
        config.routing.inactive_outputs = vec!["out-fl".to_owned(), "out-fr".to_owned()];
        let plan = prepare_runtime_plan(&ValidatedConfiguration::new(config).unwrap()).unwrap();
        assert_eq!(plan.capacity().input_channel_count(), 0);
        assert_eq!(plan.capacity().route_count(), 0);
        assert_eq!(plan.capacity().speaker_count(), 2);
    }
}
