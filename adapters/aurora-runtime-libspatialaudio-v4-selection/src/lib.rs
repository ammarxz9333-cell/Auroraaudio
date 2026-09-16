#![forbid(unsafe_code)]
//! Native Configuration v4 bridge for Aurora's proven libspatialaudio selector.
//!
//! Portable Aurora configuration selects a renderer component and its contract;
//! machine-specific deployment data such as the absolute shim path remains an
//! explicit runtime input. This crate performs control-plane validation only and
//! does not load native code.

use std::error::Error;
use std::fmt;
use std::path::PathBuf;

use aurora_config::{
    ComponentContractKind, LayoutKindV4, SampleFormatIntent, ValidatedConfigurationV4,
};
use aurora_runtime_libspatialaudio_selector::{
    LibspatialaudioSelectionIntent, LIBSPATIALAUDIO_BLOCK_FRAMES, LIBSPATIALAUDIO_MEDIA_RATE_HZ,
    LIBSPATIALAUDIO_RENDERER_COMPONENT_ID, LIBSPATIALAUDIO_RENDERER_IMPLEMENTATION_VERSION,
    OBJECT_PCM_RENDERER_CONTRACT_MAJOR, OBJECT_PCM_RENDERER_CONTRACT_MINOR,
};

/// Derives the already-proven libspatialaudio v1 selector from native Aurora
/// Configuration v4 plus a machine-local absolute shim path.
///
/// The configuration must explicitly select the exact libspatialaudio adapter
/// implementation and the exact media contract already proven by Aurora. The
/// deployment path is intentionally not persisted into portable configuration.
pub fn selection_from_configuration_v4(
    configuration: &ValidatedConfigurationV4,
    shim_path: impl Into<PathBuf>,
) -> Result<LibspatialaudioSelectionIntent, V4SelectionError> {
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
        || renderer.compatible_minor.minimum > OBJECT_PCM_RENDERER_CONTRACT_MINOR
        || renderer.compatible_minor.maximum < OBJECT_PCM_RENDERER_CONTRACT_MINOR
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

    let shim_path = shim_path.into();
    if shim_path.as_os_str().is_empty() || !shim_path.is_absolute() {
        return Err(V4SelectionError::InvalidDeploymentPath);
    }

    Ok(LibspatialaudioSelectionIntent::v1(shim_path))
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
    /// Machine-local shim path is empty or non-absolute.
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

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_config::{AuroraConfigurationV4, CompatibleMinorRange, ComponentReference};

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

    #[test]
    fn canonical_v4_derives_exact_proven_selector() {
        let selected = selection_from_configuration_v4(
            &selected_configuration(),
            PathBuf::from("/opt/aurora/libaurora_libspatialaudio_shim.so"),
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
    fn basic_renderer_fixture_is_not_silently_promoted() {
        let config = ValidatedConfigurationV4::from_json(SURROUND_714).unwrap();
        assert_eq!(
            selection_from_configuration_v4(&config, "/opt/aurora/shim.so").unwrap_err(),
            V4SelectionError::ComponentIdentityMismatch
        );
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
