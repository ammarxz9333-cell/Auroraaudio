//! Stable, versioned contract for Aurora application plugins.
//!
//! This crate intentionally contains control-plane metadata only. Plugins are
//! out-of-process and must never receive pointers into Aurora realtime state or
//! execute on the audio callback. Realtime decoders, renderers, DSP engines,
//! and hardware backends continue to use their dedicated Aurora API crates.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Current manifest schema understood by this Aurora source revision.
pub const PLUGIN_MANIFEST_SCHEMA_VERSION: u16 = 1;

/// Current host/plugin protocol API supported by this source revision.
pub const HOST_PLUGIN_API_VERSION: ApiVersion = ApiVersion { major: 1, minor: 0 };

/// Version of the stable host/plugin contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ApiVersion {
    /// Breaking contract generation.
    pub major: u16,
    /// Backward-compatible feature level within one major generation.
    pub minor: u16,
}

/// Host API range accepted by a plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiCompatibility {
    /// Required API major generation.
    pub major: u16,
    /// Oldest accepted host minor version.
    pub min_minor: u16,
    /// Newest accepted host minor version.
    pub max_minor: u16,
}

impl ApiCompatibility {
    /// Returns whether this plugin declares compatibility with a host API.
    pub const fn accepts(self, host: ApiVersion) -> bool {
        self.major == host.major
            && host.minor >= self.min_minor
            && host.minor <= self.max_minor
    }
}

/// Stable application-level capability exposed by a plugin.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum PluginCapability {
    /// Produces a selectable media source or stream for Aurora playback.
    MediaSource,
    /// Browses a provider or local catalog.
    Browse,
    /// Searches a provider or local catalog.
    Search,
    /// Manages or contributes to the Aurora playback queue.
    Queue,
    /// Exposes transport operations such as play/pause/next/seek.
    PlaybackControl,
    /// Provides metadata such as title, artist, album, or identifiers.
    MetadataProvider,
    /// Provides synchronized or plain lyrics.
    LyricsProvider,
    /// Provides artwork references or artwork assets through the host broker.
    ArtworkProvider,
    /// Resolves user-supplied URLs into lawful provider/media references.
    UrlResolver,
    /// Integrates an official streaming-service control surface.
    StreamingControl,
    /// Integrates local music-library browsing and management.
    LocalLibrary,
    /// Provides a non-realtime user/control surface.
    ControlSurface,
}

/// Permission requested from the Aurora plugin host.
///
/// Deliberately absent are permissions for realtime callback access, raw
/// amplifier control, direct MCU access, and unrestricted hardware access.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum PluginPermission {
    /// Make outbound network connections through the plugin sandbox policy.
    NetworkClient,
    /// Read Aurora library metadata exposed by the host.
    MediaLibraryRead,
    /// Mutate Aurora library metadata through validated host APIs.
    MediaLibraryWrite,
    /// Submit playback/control requests through the Source Manager/control API.
    PlaybackControl,
    /// Read local media files selected or exposed by Aurora.
    LocalMediaRead,
    /// Write into host-approved plugin data/cache locations.
    PluginStorageWrite,
    /// Request provider credentials through the host credential broker.
    CredentialBroker,
    /// Publish user-visible notifications through Aurora.
    Notifications,
}

/// Transport used between the Aurora plugin host and a plugin process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginProtocol {
    /// One UTF-8 JSON object per line over stdin/stdout.
    JsonLinesStdioV1,
}

/// Lifecycle state reported by the plugin host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginRuntimeState {
    /// Installed but not enabled.
    Disabled,
    /// Host is starting the isolated plugin process.
    Starting,
    /// Plugin passed handshake and health checks.
    Running,
    /// Plugin is alive but one or more optional capabilities are impaired.
    Degraded,
    /// Plugin exited or failed a required health/handshake check.
    Failed,
    /// Restart budget was exhausted or policy isolated the plugin.
    Quarantined,
    /// Plugin package is being atomically replaced or rolled back.
    Updating,
}

/// Versioned plugin package manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Manifest schema generation.
    pub schema_version: u16,
    /// Stable reverse-DNS-like identifier, e.g. `org.aurora.spotify`.
    pub id: String,
    /// Human-readable plugin name.
    pub name: String,
    /// Plugin package/application version.
    pub version: String,
    /// Aurora host API compatibility range.
    pub api: ApiCompatibility,
    /// Application capabilities this plugin exposes.
    pub capabilities: Vec<PluginCapability>,
    /// Host permissions requested by the plugin.
    pub permissions: Vec<PluginPermission>,
    /// Process entrypoint relative to the installed plugin package root.
    pub entrypoint: String,
    /// IPC transport contract.
    pub protocol: PluginProtocol,
}

impl PluginManifest {
    /// Validates the manifest against one host API version.
    pub fn validate_for_host(&self, host: ApiVersion) -> Result<(), PluginManifestError> {
        if self.schema_version != PLUGIN_MANIFEST_SCHEMA_VERSION {
            return Err(PluginManifestError::UnsupportedSchemaVersion {
                actual: self.schema_version,
                expected: PLUGIN_MANIFEST_SCHEMA_VERSION,
            });
        }
        if !is_canonical_plugin_id(&self.id) {
            return Err(PluginManifestError::InvalidPluginId(self.id.clone()));
        }
        if self.name.trim().is_empty() {
            return Err(PluginManifestError::EmptyName);
        }
        if self.version.trim().is_empty() {
            return Err(PluginManifestError::EmptyVersion);
        }
        if self.entrypoint.trim().is_empty() || self.entrypoint.starts_with('/') {
            return Err(PluginManifestError::InvalidEntrypoint(
                self.entrypoint.clone(),
            ));
        }
        if self.entrypoint.split('/').any(|part| part == ".." || part.is_empty()) {
            return Err(PluginManifestError::InvalidEntrypoint(
                self.entrypoint.clone(),
            ));
        }
        if self.api.min_minor > self.api.max_minor {
            return Err(PluginManifestError::InvalidApiRange);
        }
        if !self.api.accepts(host) {
            return Err(PluginManifestError::IncompatibleHostApi {
                plugin_major: self.api.major,
                min_minor: self.api.min_minor,
                max_minor: self.api.max_minor,
                host,
            });
        }
        if self.capabilities.is_empty() {
            return Err(PluginManifestError::NoCapabilities);
        }
        ensure_unique(&self.capabilities, PluginManifestError::DuplicateCapability)?;
        ensure_unique(&self.permissions, PluginManifestError::DuplicatePermission)?;
        Ok(())
    }
}

/// Initial handshake sent by the Aurora plugin host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostHello {
    /// Host API offered to the plugin.
    pub api: ApiVersion,
}

/// Initial handshake returned by the plugin process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginHello {
    /// Must exactly match the installed manifest ID.
    pub plugin_id: String,
    /// Concrete API version selected by the plugin.
    pub api: ApiVersion,
    /// Runtime capabilities actually available for this launch.
    pub capabilities: Vec<PluginCapability>,
}

/// Minimal process health record consumed by the future plugin manager.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginHealth {
    /// Current lifecycle state.
    pub state: PluginRuntimeState,
    /// Number of process restarts in the current host-defined budget window.
    pub restart_count: u32,
    /// Optional bounded human-readable diagnostic.
    pub detail: Option<String>,
}

/// Fail-closed manifest validation errors.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PluginManifestError {
    /// Manifest schema is unknown to this host.
    #[error("unsupported plugin manifest schema {actual}; expected {expected}")]
    UnsupportedSchemaVersion { actual: u16, expected: u16 },
    /// Plugin ID is not canonical.
    #[error("invalid plugin id `{0}`")]
    InvalidPluginId(String),
    /// Name must be non-empty.
    #[error("plugin name is empty")]
    EmptyName,
    /// Version must be non-empty.
    #[error("plugin version is empty")]
    EmptyVersion,
    /// Entrypoints must remain relative to the package root.
    #[error("invalid plugin entrypoint `{0}`")]
    InvalidEntrypoint(String),
    /// API range is internally contradictory.
    #[error("plugin API min_minor is greater than max_minor")]
    InvalidApiRange,
    /// Host API is outside the declared range.
    #[error(
        "plugin requires API {plugin_major}.{min_minor}..={plugin_major}.{max_minor}, host is {host_major}.{host_minor}",
        host_major = .host.major,
        host_minor = .host.minor
    )]
    IncompatibleHostApi {
        plugin_major: u16,
        min_minor: u16,
        max_minor: u16,
        host: ApiVersion,
    },
    /// At least one capability must be declared.
    #[error("plugin declares no capabilities")]
    NoCapabilities,
    /// Capability list must not contain duplicates.
    #[error("plugin declares duplicate capability {0:?}")]
    DuplicateCapability(PluginCapability),
    /// Permission list must not contain duplicates.
    #[error("plugin requests duplicate permission {0:?}")]
    DuplicatePermission(PluginPermission),
}

fn ensure_unique<T>(items: &[T], make_error: fn(T) -> PluginManifestError) -> Result<(), PluginManifestError>
where
    T: Copy + Ord,
{
    let mut seen = BTreeSet::new();
    for item in items {
        if !seen.insert(*item) {
            return Err(make_error(*item));
        }
    }
    Ok(())
}

fn is_canonical_plugin_id(id: &str) -> bool {
    let mut segments = id.split('.');
    let Some(first) = segments.next() else {
        return false;
    };
    let rest = segments.collect::<Vec<_>>();
    if rest.is_empty() || !valid_id_segment(first) {
        return false;
    }
    rest.into_iter().all(valid_id_segment)
}

fn valid_id_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && segment
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && segment
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spotify_like_manifest() -> PluginManifest {
        PluginManifest {
            schema_version: PLUGIN_MANIFEST_SCHEMA_VERSION,
            id: "org.aurora.spotify".to_owned(),
            name: "Spotify integration".to_owned(),
            version: "1.0.0".to_owned(),
            api: ApiCompatibility {
                major: 1,
                min_minor: 0,
                max_minor: 2,
            },
            capabilities: vec![
                PluginCapability::MediaSource,
                PluginCapability::Browse,
                PluginCapability::Search,
                PluginCapability::PlaybackControl,
                PluginCapability::StreamingControl,
            ],
            permissions: vec![
                PluginPermission::NetworkClient,
                PluginPermission::CredentialBroker,
                PluginPermission::PlaybackControl,
            ],
            entrypoint: "bin/plugin".to_owned(),
            protocol: PluginProtocol::JsonLinesStdioV1,
        }
    }

    #[test]
    fn representative_streaming_plugin_is_admitted() {
        spotify_like_manifest()
            .validate_for_host(HOST_PLUGIN_API_VERSION)
            .unwrap();
    }

    #[test]
    fn manifest_round_trip_is_stable_json() {
        let manifest = spotify_like_manifest();
        let json = serde_json::to_string(&manifest).unwrap();
        let decoded: PluginManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, manifest);
    }

    #[test]
    fn incompatible_major_fails_closed() {
        let mut manifest = spotify_like_manifest();
        manifest.api.major = HOST_PLUGIN_API_VERSION.major + 1;
        assert!(matches!(
            manifest.validate_for_host(HOST_PLUGIN_API_VERSION),
            Err(PluginManifestError::IncompatibleHostApi { .. })
        ));
    }

    #[test]
    fn duplicate_permissions_are_rejected() {
        let mut manifest = spotify_like_manifest();
        manifest.permissions.push(PluginPermission::NetworkClient);
        assert_eq!(
            manifest.validate_for_host(HOST_PLUGIN_API_VERSION),
            Err(PluginManifestError::DuplicatePermission(
                PluginPermission::NetworkClient
            ))
        );
    }

    #[test]
    fn entrypoint_cannot_escape_package_root() {
        let mut manifest = spotify_like_manifest();
        manifest.entrypoint = "../escape".to_owned();
        assert!(matches!(
            manifest.validate_for_host(HOST_PLUGIN_API_VERSION),
            Err(PluginManifestError::InvalidEntrypoint(_))
        ));
    }

    #[test]
    fn plugin_id_requires_canonical_segments() {
        let mut manifest = spotify_like_manifest();
        manifest.id = "Spotify.Plugin".to_owned();
        assert!(matches!(
            manifest.validate_for_host(HOST_PLUGIN_API_VERSION),
            Err(PluginManifestError::InvalidPluginId(_))
        ));
    }
}
