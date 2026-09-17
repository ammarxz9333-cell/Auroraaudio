#![forbid(unsafe_code)]
//! Native Configuration v4 bridge for Aurora's proven libspatialaudio selector.
//!
//! Portable Aurora configuration selects a renderer component and its contract;
//! machine-specific deployment data such as the absolute shim path remains an
//! explicit runtime input. Native materialization can be driven directly by a
//! validated [`PreparedRuntimePlan`] so portable intent remains the source of
//! truth while deployment paths stay local to the machine.

use std::error::Error;
use std::fmt;
use std::path::PathBuf;

use aurora_config::{
    ComponentContractKind, LayoutKindV4, SampleFormatIntent, ValidatedConfigurationV4,
};
use aurora_core::{StandardLayout, Vector3};
use aurora_realtime_engine::{RealTimeEngine, RealTimeEngineConfig};
use aurora_runtime_assembly::{
    BackendComponentRegistry, PreparedLayoutKind, PreparedRendererKind, PreparedRuntimePlan,
    RendererComponentIssue, RendererComponentRegistration, RendererComponentRegistry,
    RuntimePreparationError,
};
use aurora_runtime_libspatialaudio_selector::{
    materialize_selected_libspatialaudio_engine, LibspatialaudioSelectionIntent, SelectionError,
    LIBSPATIALAUDIO_BLOCK_FRAMES, LIBSPATIALAUDIO_MEDIA_RATE_HZ,
    LIBSPATIALAUDIO_RENDERER_COMPONENT_ID, LIBSPATIALAUDIO_RENDERER_IMPLEMENTATION_VERSION,
    OBJECT_PCM_RENDERER_CONTRACT_MAJOR, OBJECT_PCM_RENDERER_CONTRACT_MINOR,
};
use aurora_scene::RenderScene;

/// Derives the already-proven libspatialaudio v1 selector from native Aurora
/// Configuration v4 plus a machine-local absolute shim path.
///
/// This compatibility bridge remains available for callers that still own a
/// Configuration v4 value at deployment time. New runtime deployment should
/// prefer [`materialize_libspatialaudio_engine_from_prepared_plan`] so the
/// prepared runtime plan is the sole portable source of renderer intent.
pub fn selection_from_configuration_v4(
    configuration: &ValidatedConfigurationV4,
    shim_path: impl Into<PathBuf>,
) -> Result<LibspatialaudioSelectionIntent, V4SelectionError> {
    validate_configuration_v4(configuration)?;

    let shim_path = shim_path.into();
    if shim_path.as_os_str().is_empty() || !shim_path.is_absolute() {
        return Err(V4SelectionError::InvalidDeploymentPath);
    }

    Ok(LibspatialaudioSelectionIntent::v1(shim_path))
}

/// Derives Aurora's passive runtime plan for the exact proven libspatialaudio
/// Configuration v4 contract.
///
/// This does not load libspatialaudio or the Aurora shim. It registers the
/// external object-to-PCM execution model with runtime assembly, validates the
/// same fail-closed contract used by [`selection_from_configuration_v4`], and
/// returns an immutable [`PreparedRuntimePlan`].
pub fn prepare_runtime_plan_from_configuration_v4(
    configuration: &ValidatedConfigurationV4,
) -> Result<PreparedRuntimePlan, V4RuntimePlanError> {
    validate_configuration_v4(configuration).map_err(V4RuntimePlanError::Selection)?;

    let renderer_registry = RendererComponentRegistry::builtin()
        .with_registration(RendererComponentRegistration::external_object_pcm(
            LIBSPATIALAUDIO_RENDERER_COMPONENT_ID,
            LIBSPATIALAUDIO_RENDERER_IMPLEMENTATION_VERSION,
            OBJECT_PCM_RENDERER_CONTRACT_MAJOR,
            OBJECT_PCM_RENDERER_CONTRACT_MINOR,
            1,
            validate_runtime_registration,
        ))
        .map_err(V4RuntimePlanError::Runtime)?;
    let backend_registry = BackendComponentRegistry::builtin();

    PreparedRuntimePlan::from_configuration_v4_with_registries(
        configuration,
        &renderer_registry,
        &backend_registry,
    )
    .map_err(V4RuntimePlanError::Runtime)
}

/// Materializes the exact libspatialaudio realtime path from prepared Aurora
/// intent plus a machine-local shim path.
///
/// The prepared plan is validated before the selector or dynamic loader is
/// touched. Renderer execution kind, exact component identity, media contract,
/// and canonical enabled 7.1.4 topology must all match the proven v1 path.
/// Speaker directions used by the runtime scene are then bound to the prepared
/// plan while retaining each scene speaker's listener-relative radius. This
/// makes prepared topology authoritative without pretending that normalized
/// configuration directions are physical room distances.
///
/// The shim path is intentionally supplied separately and is never persisted in
/// portable configuration or the prepared plan.
pub fn materialize_libspatialaudio_engine_from_prepared_plan(
    plan: &PreparedRuntimePlan,
    shim_path: impl Into<PathBuf>,
    scene: RenderScene,
    engine_config: RealTimeEngineConfig,
    estimated_device_latency_frames: usize,
) -> Result<RealTimeEngine, PreparedPlanMaterializationError> {
    validate_prepared_plan(plan)?;
    let scene = bind_scene_to_prepared_plan(plan, scene)?;
    let selection = LibspatialaudioSelectionIntent::v1(shim_path);
    materialize_selected_libspatialaudio_engine(
        &selection,
        scene,
        engine_config,
        estimated_device_latency_frames,
    )
    .map_err(PreparedPlanMaterializationError::Selection)
}

fn validate_prepared_plan(
    plan: &PreparedRuntimePlan,
) -> Result<(), PreparedPlanMaterializationError> {
    let renderer = plan.execution().renderer();
    if renderer.kind() != PreparedRendererKind::ExternalObjectPcm {
        return Err(PreparedPlanMaterializationError::RendererExecutionMismatch);
    }

    let identity = renderer.component_identity();
    if identity.implementation_id() != LIBSPATIALAUDIO_RENDERER_COMPONENT_ID
        || identity.implementation_version() != LIBSPATIALAUDIO_RENDERER_IMPLEMENTATION_VERSION
        || identity.contract_major() != OBJECT_PCM_RENDERER_CONTRACT_MAJOR
        || identity.contract_minor() != OBJECT_PCM_RENDERER_CONTRACT_MINOR
    {
        return Err(PreparedPlanMaterializationError::ComponentIdentityMismatch);
    }

    let format = plan.execution().audio_format();
    if format.sample_rate() != LIBSPATIALAUDIO_MEDIA_RATE_HZ
        || format.callback_frames() as usize != LIBSPATIALAUDIO_BLOCK_FRAMES
        || format.output_channel_count() != 12
        || format.sample_format() != SampleFormatIntent::Float32
    {
        return Err(PreparedPlanMaterializationError::MediaContractMismatch);
    }

    let layout = plan.topology().layout();
    if layout.kind() != PreparedLayoutKind::Standard(StandardLayout::SevenOneFour) {
        return Err(PreparedPlanMaterializationError::LayoutMismatch);
    }
    let speakers = layout.speakers();
    let expected_roles = StandardLayout::SevenOneFour.canonical_roles();
    if speakers.len() != expected_roles.len()
        || speakers
            .iter()
            .zip(expected_roles.iter())
            .any(|(speaker, role)| {
                !speaker.is_active() || speaker.channel_role().as_str() != role.as_str()
            })
    {
        return Err(PreparedPlanMaterializationError::LayoutMismatch);
    }

    Ok(())
}

/// Binds the runtime scene's speaker directions to prepared topology while
/// preserving scene-owned listener-relative radius, trims, delays and labels.
///
/// Configuration-v4 speaker geometry is a normalized direction. RenderScene
/// speaker positions are meter-space locations. Comparing or copying either
/// representation directly would conflate different units, so binding projects
/// each prepared direction onto the scene speaker's existing radius around the
/// listener. Missing/duplicate roles, inactive speakers, non-finite geometry or
/// a zero-radius scene speaker fail before any native library is loaded.
fn bind_scene_to_prepared_plan(
    plan: &PreparedRuntimePlan,
    mut scene: RenderScene,
) -> Result<RenderScene, PreparedPlanMaterializationError> {
    if scene.layout != StandardLayout::SevenOneFour || scene.speakers.len() != 12 {
        return Err(PreparedPlanMaterializationError::SceneTopologyMismatch);
    }

    let prepared = plan.topology().layout().speakers();
    if prepared.len() != 12 {
        return Err(PreparedPlanMaterializationError::LayoutMismatch);
    }

    for prepared_speaker in prepared {
        let matches = scene
            .speakers
            .iter()
            .filter(|speaker| speaker.channel_role == *prepared_speaker.channel_role())
            .count();
        if matches != 1 {
            return Err(PreparedPlanMaterializationError::SceneTopologyMismatch);
        }

        let scene_speaker = scene
            .speakers
            .iter_mut()
            .find(|speaker| speaker.channel_role == *prepared_speaker.channel_role())
            .expect("role count was exactly one");
        if scene_speaker.enabled != prepared_speaker.is_active() {
            return Err(PreparedPlanMaterializationError::SceneTopologyMismatch);
        }

        let relative = scene_speaker.position - scene.listener.position;
        let radius = relative.length();
        let prepared_direction = prepared_speaker.position();
        let prepared_length = prepared_direction.length();
        if !radius.is_finite()
            || radius <= f32::EPSILON
            || !prepared_length.is_finite()
            || prepared_length <= f32::EPSILON
        {
            return Err(PreparedPlanMaterializationError::SceneGeometryMismatch);
        }

        let scale = radius / prepared_length;
        scene_speaker.position = Vector3::new(
            scene.listener.position.x + prepared_direction.x * scale,
            scene.listener.position.y + prepared_direction.y * scale,
            scene.listener.position.z + prepared_direction.z * scale,
        );
    }

    Ok(scene)
}

fn validate_configuration_v4(
    configuration: &ValidatedConfigurationV4,
) -> Result<(), V4SelectionError> {
    let config = configuration.config();
    let renderer = &config.renderer;

    if config.speaker_layout.kind != LayoutKindV4::Surround714
        || !config.speaker_layout.elevation_rendering
        || config.audio_format.channel_count != 12
    {
        return Err(V4SelectionError::UnsupportedLayout);
    }
    if config.audio_format.sample_rate != LIBSPATIALAUDIO_MEDIA_RATE_HZ
        || config.audio_format.callback_frames as usize != LIBSPATIALAUDIO_BLOCK_FRAMES
        || config.audio_format.sample_format != SampleFormatIntent::Float32
    {
        return Err(V4SelectionError::UnsupportedMediaContract);
    }
    if renderer.component_id != LIBSPATIALAUDIO_RENDERER_COMPONENT_ID {
        return Err(V4SelectionError::ComponentIdentityMismatch);
    }
    if renderer.contract_kind != ComponentContractKind::Renderer {
        return Err(V4SelectionError::ContractKindMismatch);
    }
    if renderer.contract_major != OBJECT_PCM_RENDERER_CONTRACT_MAJOR
        || renderer.compatible_minor.minimum != OBJECT_PCM_RENDERER_CONTRACT_MINOR
    {
        return Err(V4SelectionError::RendererContractMismatch);
    }
    if renderer.implementation_version_pin.as_deref()
        != Some(LIBSPATIALAUDIO_RENDERER_IMPLEMENTATION_VERSION)
    {
        return Err(V4SelectionError::ImplementationVersionPinRequired);
    }
    if renderer.configuration_schema != 1
        || !renderer
            .configuration
            .as_object()
            .is_some_and(|payload| payload.is_empty())
    {
        return Err(V4SelectionError::InvalidComponentConfiguration);
    }

    Ok(())
}

fn validate_runtime_registration(
    payload: &serde_json::Value,
    active_speakers: usize,
) -> Result<(), RendererComponentIssue> {
    if active_speakers != 12 {
        return Err(RendererComponentIssue::LayoutCapabilityMismatch);
    }
    match payload.as_object() {
        Some(values) if values.is_empty() => Ok(()),
        _ => Err(RendererComponentIssue::InvalidConfiguration),
    }
}

/// Fail-closed native-v4 selection failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V4SelectionError {
    /// Configuration is not canonical elevation-enabled 7.1.4.
    UnsupportedLayout,
    /// Sample rate, block size, channel count, or sample type is outside v1 evidence.
    UnsupportedMediaContract,
    /// The renderer component ID is not Aurora's libspatialaudio adapter.
    ComponentIdentityMismatch,
    /// The component reference does not target the renderer contract family.
    ContractKindMismatch,
    /// The requested object-PCM renderer contract does not include 1.0.
    RendererContractMismatch,
    /// Native external selection requires the exact proven Aurora adapter version pin.
    ImplementationVersionPinRequired,
    /// Component schema/payload is outside the proven empty v1 configuration.
    InvalidComponentConfiguration,
    /// Machine-local shim path is empty or not absolute.
    InvalidDeploymentPath,
}

impl fmt::Display for V4SelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::UnsupportedLayout => "libspatialaudio v4 selection requires canonical elevation-enabled 7.1.4",
            Self::UnsupportedMediaContract => "libspatialaudio v4 selection requires float32 12-channel 48 kHz / 256-frame media",
            Self::ComponentIdentityMismatch => "Configuration v4 does not select the libspatialaudio renderer component",
            Self::ContractKindMismatch => "libspatialaudio component reference is not a renderer contract",
            Self::RendererContractMismatch => "libspatialaudio object-PCM renderer contract does not include 1.0",
            Self::ImplementationVersionPinRequired => "libspatialaudio Configuration v4 selection requires the exact proven adapter version pin",
            Self::InvalidComponentConfiguration => "libspatialaudio component configuration must use schema 1 with an empty payload",
            Self::InvalidDeploymentPath => "libspatialaudio shim deployment path must be absolute and nonempty",
        };
        formatter.write_str(message)
    }
}

impl Error for V4SelectionError {}

/// Failure to derive the passive native-v4 runtime plan.
#[derive(Debug)]
pub enum V4RuntimePlanError {
    /// The Configuration v4 request is outside the proven libspatialaudio contract.
    Selection(V4SelectionError),
    /// Runtime assembly rejected the validated component or plan invariants.
    Runtime(RuntimePreparationError),
}

impl fmt::Display for V4RuntimePlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Selection(error) => {
                write!(formatter, "libspatialaudio v4 selection rejected: {error}")
            }
            Self::Runtime(error) => write!(
                formatter,
                "libspatialaudio v4 runtime plan rejected: {error}"
            ),
        }
    }
}

impl Error for V4RuntimePlanError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Selection(error) => Some(error),
            Self::Runtime(error) => Some(error),
        }
    }
}

/// Failure to deploy libspatialaudio from a prepared runtime plan.
#[derive(Debug)]
pub enum PreparedPlanMaterializationError {
    /// The prepared renderer is not explicitly an external object-to-PCM renderer.
    RendererExecutionMismatch,
    /// The prepared component identity/version/contract is not the exact proven adapter.
    ComponentIdentityMismatch,
    /// Prepared rate, block size, sample representation, or output width is unsupported.
    MediaContractMismatch,
    /// Prepared topology is not canonical enabled Aurora 7.1.4.
    LayoutMismatch,
    /// Runtime scene roles/activation do not match the prepared 7.1.4 topology.
    SceneTopologyMismatch,
    /// Runtime scene cannot preserve a finite nonzero radius while applying prepared direction.
    SceneGeometryMismatch,
    /// Deployment selection or native/realtime materialization failed after plan validation.
    Selection(SelectionError),
}

impl fmt::Display for PreparedPlanMaterializationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RendererExecutionMismatch => {
                formatter.write_str("prepared renderer is not an external object-to-PCM renderer")
            }
            Self::ComponentIdentityMismatch => formatter
                .write_str("prepared renderer identity does not match exact libspatialaudio v1"),
            Self::MediaContractMismatch => {
                formatter.write_str("prepared media contract does not match libspatialaudio v1")
            }
            Self::LayoutMismatch => {
                formatter.write_str("prepared topology is not canonical enabled Aurora 7.1.4")
            }
            Self::SceneTopologyMismatch => formatter
                .write_str("render scene roles/activation do not match prepared Aurora 7.1.4"),
            Self::SceneGeometryMismatch => formatter.write_str(
                "render scene cannot bind prepared direction while preserving a finite nonzero radius",
            ),
            Self::Selection(error) => {
                write!(
                    formatter,
                    "libspatialaudio deployment materialization failed: {error}"
                )
            }
        }
    }
}

impl Error for PreparedPlanMaterializationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Selection(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_config::{AuroraConfigurationV4, CompatibleMinorRange, ComponentReference};
    use aurora_realtime_engine::TestSignal;

    const SURROUND_714: &[u8] = include_bytes!("../../../fixtures/config/surround-7-1-4-v4.json");

    fn selected_configuration() -> ValidatedConfigurationV4 {
        let mut config: AuroraConfigurationV4 = serde_json::from_slice(SURROUND_714).unwrap();
        config.renderer = ComponentReference {
            component_id: LIBSPATIALAUDIO_RENDERER_COMPONENT_ID.to_owned(),
            contract_kind: ComponentContractKind::Renderer,
            contract_major: OBJECT_PCM_RENDERER_CONTRACT_MAJOR,
            compatible_minor: CompatibleMinorRange {
                minimum: OBJECT_PCM_RENDERER_CONTRACT_MINOR,
                maximum: OBJECT_PCM_RENDERER_CONTRACT_MINOR,
            },
            implementation_version_pin: Some(
                LIBSPATIALAUDIO_RENDERER_IMPLEMENTATION_VERSION.to_owned(),
            ),
            configuration_schema: 1,
            configuration: serde_json::json!({}),
        };
        ValidatedConfigurationV4::new(config).unwrap()
    }

    fn scene() -> RenderScene {
        serde_json::from_str(include_str!(
            "../../../fixtures/scenes/7_1_4_reference.json"
        ))
        .unwrap()
    }

    fn engine_config() -> RealTimeEngineConfig {
        RealTimeEngineConfig {
            sample_rate: LIBSPATIALAUDIO_MEDIA_RATE_HZ,
            block_size: LIBSPATIALAUDIO_BLOCK_FRAMES,
            input_channels: 0,
            apply_geometric_delay: false,
            speed_of_sound: 343.0,
            test_signal: TestSignal::Sine,
        }
    }

    #[test]
    fn canonical_v4_derives_exact_proven_selector() {
        let selected = selection_from_configuration_v4(
            &selected_configuration(),
            std::env::temp_dir().join("libaurora_libspatialaudio_shim.so"),
        )
        .unwrap();

        assert_eq!(selected.component_id, LIBSPATIALAUDIO_RENDERER_COMPONENT_ID);
        assert_eq!(
            selected.implementation_version,
            LIBSPATIALAUDIO_RENDERER_IMPLEMENTATION_VERSION
        );
        assert_eq!(selected.sample_rate, LIBSPATIALAUDIO_MEDIA_RATE_HZ);
        assert_eq!(selected.block_frames, LIBSPATIALAUDIO_BLOCK_FRAMES);
        assert!(selected.enabled);
    }

    #[test]
    fn canonical_v4_derives_external_object_pcm_runtime_plan() {
        let plan = prepare_runtime_plan_from_configuration_v4(&selected_configuration()).unwrap();
        let renderer = plan.execution().renderer();
        let identity = renderer.component_identity();

        assert_eq!(renderer.kind(), PreparedRendererKind::ExternalObjectPcm);
        assert_eq!(renderer.horizontal_spread(), None);
        assert_eq!(
            identity.implementation_id(),
            LIBSPATIALAUDIO_RENDERER_COMPONENT_ID
        );
        assert_eq!(
            identity.implementation_version(),
            LIBSPATIALAUDIO_RENDERER_IMPLEMENTATION_VERSION
        );
        assert_eq!(
            identity.contract_major(),
            OBJECT_PCM_RENDERER_CONTRACT_MAJOR
        );
        assert_eq!(
            identity.contract_minor(),
            OBJECT_PCM_RENDERER_CONTRACT_MINOR
        );
        assert_eq!(plan.execution().audio_format().output_channel_count(), 12);
        assert_eq!(plan.execution().audio_format().sample_rate(), 48_000);
        assert_eq!(plan.execution().audio_format().callback_frames(), 256);
        validate_prepared_plan(&plan).unwrap();
    }

    #[test]
    fn scene_binding_uses_prepared_directions_and_preserves_scene_radii() {
        let plan = prepare_runtime_plan_from_configuration_v4(&selected_configuration()).unwrap();
        let source = scene();
        let listener = source.listener.position;
        let original_radii: Vec<f32> = StandardLayout::SevenOneFour
            .canonical_roles()
            .iter()
            .map(|role| {
                source
                    .speakers
                    .iter()
                    .find(|speaker| speaker.channel_role == *role)
                    .unwrap()
                    .position
                    .distance_to(listener)
            })
            .collect();

        let bound = bind_scene_to_prepared_plan(&plan, source).unwrap();
        let ordered = bound.ordered_speakers().unwrap();
        for (((scene_speaker, prepared_speaker), original_radius), role) in ordered
            .iter()
            .zip(plan.topology().layout().speakers())
            .zip(original_radii.iter())
            .zip(StandardLayout::SevenOneFour.canonical_roles())
        {
            assert_eq!(&scene_speaker.channel_role, role);
            assert_eq!(scene_speaker.channel_role, *prepared_speaker.channel_role());
            let relative = scene_speaker.position - bound.listener.position;
            let radius = relative.length();
            assert!((radius - original_radius).abs() < 1.0e-5);
            let actual = Vector3::new(relative.x / radius, relative.y / radius, relative.z / radius);
            let expected = prepared_speaker.position();
            let expected_length = expected.length();
            let expected = Vector3::new(
                expected.x / expected_length,
                expected.y / expected_length,
                expected.z / expected_length,
            );
            assert!((actual.x - expected.x).abs() < 1.0e-5);
            assert!((actual.y - expected.y).abs() < 1.0e-5);
            assert!((actual.z - expected.z).abs() < 1.0e-5);
        }
    }

    #[test]
    fn zero_radius_scene_speaker_fails_before_native_load() {
        let plan = prepare_runtime_plan_from_configuration_v4(&selected_configuration()).unwrap();
        let mut render_scene = scene();
        render_scene.speakers[0].position = render_scene.listener.position;
        let error = materialize_libspatialaudio_engine_from_prepared_plan(
            &plan,
            "/definitely/not/a/real/shim.so",
            render_scene,
            engine_config(),
            0,
        )
        .err()
        .expect("zero-radius scene must fail before native load");
        assert!(matches!(
            error,
            PreparedPlanMaterializationError::SceneGeometryMismatch
        ));
    }

    #[test]
    fn duplicate_scene_role_fails_before_native_load() {
        let plan = prepare_runtime_plan_from_configuration_v4(&selected_configuration()).unwrap();
        let mut render_scene = scene();
        render_scene.speakers[1].channel_role = render_scene.speakers[0].channel_role.clone();
        let error = materialize_libspatialaudio_engine_from_prepared_plan(
            &plan,
            "/definitely/not/a/real/shim.so",
            render_scene,
            engine_config(),
            0,
        )
        .err()
        .expect("duplicate scene role must fail before native load");
        assert!(matches!(
            error,
            PreparedPlanMaterializationError::SceneTopologyMismatch
        ));
    }

    #[test]
    fn basic_renderer_fixture_is_not_silently_promoted() {
        let config = ValidatedConfigurationV4::from_json(SURROUND_714).unwrap();
        assert_eq!(
            selection_from_configuration_v4(&config, "/opt/aurora/shim.so").unwrap_err(),
            V4SelectionError::ComponentIdentityMismatch
        );
        assert!(matches!(
            prepare_runtime_plan_from_configuration_v4(&config),
            Err(V4RuntimePlanError::Selection(
                V4SelectionError::ComponentIdentityMismatch
            ))
        ));
    }

    #[test]
    fn prepared_basic_renderer_fails_before_native_load() {
        let config = ValidatedConfigurationV4::from_json(SURROUND_714).unwrap();
        let plan = PreparedRuntimePlan::from_configuration_v4(&config).unwrap();
        let error = materialize_libspatialaudio_engine_from_prepared_plan(
            &plan,
            "/definitely/not/a/real/shim.so",
            scene(),
            engine_config(),
            0,
        )
        .err()
        .expect("incompatible prepared plan must be rejected");
        assert!(matches!(
            error,
            PreparedPlanMaterializationError::RendererExecutionMismatch
        ));
    }

    #[test]
    fn unpinned_external_renderer_fails_closed() {
        let mut config = selected_configuration().config().clone();
        config.renderer.implementation_version_pin = None;
        let config = ValidatedConfigurationV4::new(config).unwrap();
        assert_eq!(
            selection_from_configuration_v4(&config, "/opt/aurora/shim.so").unwrap_err(),
            V4SelectionError::ImplementationVersionPinRequired
        );
    }

    #[test]
    fn incompatible_block_size_fails_closed() {
        let mut config = selected_configuration().config().clone();
        config.audio_format.callback_frames = 128;
        let config = ValidatedConfigurationV4::new(config).unwrap();
        assert_eq!(
            selection_from_configuration_v4(&config, "/opt/aurora/shim.so").unwrap_err(),
            V4SelectionError::UnsupportedMediaContract
        );
        assert!(matches!(
            prepare_runtime_plan_from_configuration_v4(&config),
            Err(V4RuntimePlanError::Selection(
                V4SelectionError::UnsupportedMediaContract
            ))
        ));
    }

    #[test]
    fn runtime_registration_rejects_non_twelve_active_speakers() {
        assert!(matches!(
            validate_runtime_registration(&serde_json::json!({}), 11),
            Err(RendererComponentIssue::LayoutCapabilityMismatch)
        ));
    }

    #[test]
    fn deployment_path_stays_external_and_must_be_absolute() {
        assert_eq!(
            selection_from_configuration_v4(&selected_configuration(), "relative/shim.so")
                .unwrap_err(),
            V4SelectionError::InvalidDeploymentPath
        );
    }
}
