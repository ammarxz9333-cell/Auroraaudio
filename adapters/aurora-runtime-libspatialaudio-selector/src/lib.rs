//! Explicit control-plane selection for Aurora's libspatialaudio PCM renderer.
//!
//! This adapter deliberately lives outside Aurora's default workspace/runtime
//! selection. Nothing native is loaded unless a caller parses or constructs an
//! enabled selection whose component identity and fixed v1 media contract match
//! exactly. The selected renderer is then handed to Aurora's already-proven
//! object-PCM realtime materialization path.

#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;
use std::path::PathBuf;

use aurora_core::StandardLayout;
use aurora_realtime_engine::{RealTimeEngine, RealTimeEngineConfig, RealTimeEngineError};
use aurora_renderer_libspatialaudio::{
    LibspatialaudioLoadError, LibspatialaudioRenderer, LibspatialaudioRuntimeConfig,
};
use aurora_runtime_materialization::materialize_realtime_engine_with_pcm_renderer;
use aurora_scene::RenderScene;
use serde::{Deserialize, Serialize};

/// Selector schema understood by this adapter.
pub const SELECTION_SCHEMA_VERSION: u16 = 1;
/// Stable component identity for the opt-in libspatialaudio renderer.
pub const LIBSPATIALAUDIO_RENDERER_COMPONENT_ID: &str = "org.aurora.renderer.libspatialaudio";
/// Version of the Aurora adapter selected by this package.
pub const LIBSPATIALAUDIO_RENDERER_IMPLEMENTATION_VERSION: &str = "0.1.0";
/// Aurora object-PCM renderer contract major used by the selector.
pub const OBJECT_PCM_RENDERER_CONTRACT_MAJOR: u16 = 1;
/// Aurora object-PCM renderer contract minor used by the selector.
pub const OBJECT_PCM_RENDERER_CONTRACT_MINOR: u16 = 0;
/// Fixed media rate proven for libspatialaudio v1.
pub const LIBSPATIALAUDIO_MEDIA_RATE_HZ: u32 = 48_000;
/// Fixed callback/render block proven for libspatialaudio v1.
pub const LIBSPATIALAUDIO_BLOCK_FRAMES: usize = 256;

/// Plugin-owned, explicit control-plane selection intent.
///
/// The generic Aurora configuration schema is intentionally not widened by
/// this package. This value is a narrow opt-in bridge while Aurora's generic
/// control-plane layout schema remains horizontal-only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibspatialaudioSelectionIntent {
    /// Selector schema version.
    pub schema_version: u16,
    /// Stable renderer component identity.
    pub component_id: String,
    /// Exact Aurora adapter implementation version.
    pub implementation_version: String,
    /// Required Aurora object-PCM contract major.
    pub contract_major: u16,
    /// Required Aurora object-PCM contract minor.
    pub contract_minor: u16,
    /// Explicit activation bit. False means the adapter must not load native code.
    pub enabled: bool,
    /// Absolute path to the Aurora-owned libspatialaudio shim shared library.
    pub shim_path: PathBuf,
    /// Requested media sample rate.
    pub sample_rate: u32,
    /// Requested fixed processing block.
    pub block_frames: usize,
    /// Requested speaker layout.
    pub layout: StandardLayout,
}

impl LibspatialaudioSelectionIntent {
    /// Returns the canonical opt-in intent for one shim path.
    pub fn v1(shim_path: impl Into<PathBuf>) -> Self {
        Self {
            schema_version: SELECTION_SCHEMA_VERSION,
            component_id: LIBSPATIALAUDIO_RENDERER_COMPONENT_ID.to_owned(),
            implementation_version: LIBSPATIALAUDIO_RENDERER_IMPLEMENTATION_VERSION.to_owned(),
            contract_major: OBJECT_PCM_RENDERER_CONTRACT_MAJOR,
            contract_minor: OBJECT_PCM_RENDERER_CONTRACT_MINOR,
            enabled: true,
            shim_path: shim_path.into(),
            sample_rate: LIBSPATIALAUDIO_MEDIA_RATE_HZ,
            block_frames: LIBSPATIALAUDIO_BLOCK_FRAMES,
            layout: StandardLayout::SevenOneFour,
        }
    }

    /// Parses a bounded-size JSON selection document.
    pub fn from_json(bytes: &[u8]) -> Result<Self, SelectionError> {
        // This plugin-owned document is intentionally tiny. Keep a local hard
        // bound so an adapter selection can never become an unbounded config bag.
        if bytes.len() > 16 * 1024 {
            return Err(SelectionError::SelectionDocumentTooLarge);
        }
        serde_json::from_slice(bytes).map_err(|_| SelectionError::InvalidSelectionJson)
    }

    /// Validates selection and scene/media compatibility without loading the shim.
    pub fn validate_for(
        &self,
        scene: &RenderScene,
        engine_config: &RealTimeEngineConfig,
    ) -> Result<(), SelectionError> {
        if !self.enabled {
            return Err(SelectionError::NotSelected);
        }
        if self.schema_version != SELECTION_SCHEMA_VERSION {
            return Err(SelectionError::UnsupportedSelectionSchema {
                actual: self.schema_version,
            });
        }
        if self.component_id != LIBSPATIALAUDIO_RENDERER_COMPONENT_ID {
            return Err(SelectionError::ComponentIdentityMismatch);
        }
        if self.implementation_version != LIBSPATIALAUDIO_RENDERER_IMPLEMENTATION_VERSION {
            return Err(SelectionError::ImplementationVersionMismatch);
        }
        if self.contract_major != OBJECT_PCM_RENDERER_CONTRACT_MAJOR
            || self.contract_minor != OBJECT_PCM_RENDERER_CONTRACT_MINOR
        {
            return Err(SelectionError::RendererContractMismatch);
        }
        if self.shim_path.as_os_str().is_empty() || !self.shim_path.is_absolute() {
            return Err(SelectionError::InvalidShimPath);
        }
        if self.sample_rate != LIBSPATIALAUDIO_MEDIA_RATE_HZ
            || self.block_frames != LIBSPATIALAUDIO_BLOCK_FRAMES
            || self.layout != StandardLayout::SevenOneFour
        {
            return Err(SelectionError::UnsupportedMediaContract);
        }
        if engine_config.sample_rate != self.sample_rate
            || engine_config.block_size != self.block_frames
        {
            return Err(SelectionError::EngineMediaContractMismatch);
        }
        if scene.layout != StandardLayout::SevenOneFour {
            return Err(SelectionError::SceneLayoutMismatch);
        }
        let speakers = scene
            .ordered_speakers()
            .map_err(|_| SelectionError::SceneLayoutMismatch)?;
        let expected = StandardLayout::SevenOneFour.canonical_roles();
        if speakers.len() != expected.len()
            || speakers
                .iter()
                .zip(expected.iter())
                .any(|(speaker, role)| {
                    !speaker.enabled || speaker.channel_role.as_str() != role.as_str()
                })
        {
            return Err(SelectionError::SceneLayoutMismatch);
        }
        Ok(())
    }
}

/// Materializes the explicitly selected libspatialaudio renderer into Aurora.
///
/// Validation happens before the dynamic loader is touched. Therefore disabled,
/// mismatched, malformed, or incompatible selections fail closed without loading
/// native code. Basic/VBAP remain Aurora's ordinary/default paths.
pub fn materialize_selected_libspatialaudio_engine(
    selection: &LibspatialaudioSelectionIntent,
    scene: RenderScene,
    engine_config: RealTimeEngineConfig,
    estimated_device_latency_frames: usize,
) -> Result<RealTimeEngine, SelectionError> {
    selection.validate_for(&scene, &engine_config)?;
    let renderer = LibspatialaudioRenderer::load(LibspatialaudioRuntimeConfig::new(
        selection.shim_path.clone(),
    ))
    .map_err(SelectionError::RendererLoad)?;
    materialize_realtime_engine_with_pcm_renderer(
        scene,
        engine_config,
        Box::new(renderer),
        estimated_device_latency_frames,
    )
    .map_err(SelectionError::Engine)
}

/// Fail-closed control-plane selection failures.
#[derive(Debug)]
pub enum SelectionError {
    /// The plugin-owned JSON exceeded its fixed control-plane size bound.
    SelectionDocumentTooLarge,
    /// JSON could not be parsed into the exact selector schema.
    InvalidSelectionJson,
    /// The selection exists but is explicitly disabled.
    NotSelected,
    /// Selector schema is not supported by this adapter.
    UnsupportedSelectionSchema { actual: u16 },
    /// Component ID is not the libspatialaudio renderer ID.
    ComponentIdentityMismatch,
    /// The requested Aurora adapter version is not this exact implementation.
    ImplementationVersionMismatch,
    /// Aurora object-PCM renderer contract version does not match.
    RendererContractMismatch,
    /// Shim path is empty or not absolute.
    InvalidShimPath,
    /// Requested layout/rate/block is outside the proven v1 contract.
    UnsupportedMediaContract,
    /// Realtime engine rate/block differs from the selection intent.
    EngineMediaContractMismatch,
    /// Scene is not canonical enabled Aurora 7.1.4 speaker order.
    SceneLayoutMismatch,
    /// Native Aurora-owned shim could not be loaded or validated.
    RendererLoad(LibspatialaudioLoadError),
    /// Aurora realtime materialization rejected the configured renderer.
    Engine(RealTimeEngineError),
}

impl fmt::Display for SelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SelectionDocumentTooLarge => {
                formatter.write_str("libspatialaudio selection document exceeds 16 KiB")
            }
            Self::InvalidSelectionJson => {
                formatter.write_str("invalid libspatialaudio selection JSON")
            }
            Self::NotSelected => formatter.write_str("libspatialaudio renderer is not selected"),
            Self::UnsupportedSelectionSchema { actual } => write!(
                formatter,
                "unsupported libspatialaudio selection schema {actual}"
            ),
            Self::ComponentIdentityMismatch => {
                formatter.write_str("libspatialaudio renderer component identity mismatch")
            }
            Self::ImplementationVersionMismatch => {
                formatter.write_str("libspatialaudio adapter implementation version mismatch")
            }
            Self::RendererContractMismatch => {
                formatter.write_str("libspatialaudio object-PCM renderer contract mismatch")
            }
            Self::InvalidShimPath => {
                formatter.write_str("libspatialaudio shim path must be absolute and nonempty")
            }
            Self::UnsupportedMediaContract => formatter.write_str(
                "libspatialaudio v1 requires canonical 7.1.4 at 48 kHz with 256-frame blocks",
            ),
            Self::EngineMediaContractMismatch => formatter.write_str(
                "realtime engine rate/block does not match libspatialaudio selection",
            ),
            Self::SceneLayoutMismatch => {
                formatter.write_str("scene is not canonical enabled Aurora 7.1.4")
            }
            Self::RendererLoad(error) => write!(formatter, "renderer load failed: {error}"),
            Self::Engine(error) => write!(formatter, "realtime materialization failed: {error}"),
        }
    }
}

impl Error for SelectionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RendererLoad(error) => Some(error),
            Self::Engine(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use aurora_realtime_engine::TestSignal;

    use super::*;

    fn scene() -> RenderScene {
        serde_json::from_str(include_str!("../../../fixtures/scenes/7_1_4_reference.json"))
            .expect("7.1.4 fixture scene")
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

    fn intent() -> LibspatialaudioSelectionIntent {
        LibspatialaudioSelectionIntent::v1(PathBuf::from("/definitely/not/a/real/shim.so"))
    }

    fn materialization_error(
        intent: &LibspatialaudioSelectionIntent,
        scene: RenderScene,
    ) -> SelectionError {
        materialize_selected_libspatialaudio_engine(intent, scene, engine_config(), 0)
            .err()
            .expect("selection must fail before producing an engine")
    }

    #[test]
    fn canonical_intent_round_trips_exact_schema() {
        let intent = intent();
        let json = serde_json::to_vec(&intent).unwrap();
        let parsed = LibspatialaudioSelectionIntent::from_json(&json).unwrap();
        assert_eq!(parsed, intent);
        parsed.validate_for(&scene(), &engine_config()).unwrap();
    }

    #[test]
    fn disabled_selection_fails_before_native_load() {
        let mut intent = intent();
        intent.enabled = false;
        let error = materialization_error(&intent, scene());
        assert!(matches!(error, SelectionError::NotSelected));
    }

    #[test]
    fn wrong_component_identity_fails_before_native_load() {
        let mut intent = intent();
        intent.component_id = "org.aurora.renderer.other".to_owned();
        let error = materialization_error(&intent, scene());
        assert!(matches!(error, SelectionError::ComponentIdentityMismatch));
    }

    #[test]
    fn wrong_media_contract_fails_before_native_load() {
        let mut intent = intent();
        intent.block_frames = 128;
        let error = materialization_error(&intent, scene());
        assert!(matches!(error, SelectionError::UnsupportedMediaContract));
    }

    #[test]
    fn wrong_scene_layout_fails_before_native_load() {
        let stereo: RenderScene = serde_json::from_str(include_str!(
            "../../../fixtures/scenes/stereo_circle.json"
        ))
        .unwrap();
        let error = materialization_error(&intent(), stereo);
        assert!(matches!(error, SelectionError::SceneLayoutMismatch));
    }

    #[test]
    fn unknown_json_field_is_rejected() {
        let mut value = serde_json::to_value(intent()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("surprise".to_owned(), serde_json::json!(true));
        let bytes = serde_json::to_vec(&value).unwrap();
        assert!(matches!(
            LibspatialaudioSelectionIntent::from_json(&bytes),
            Err(SelectionError::InvalidSelectionJson)
        ));
    }
}
