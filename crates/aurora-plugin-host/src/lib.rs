//! Aurora application-plugin host foundations.
//!
//! This first slice owns package admission and atomic staged update/rollback
//! state only. It does not spawn plugin processes or touch the realtime path.

use std::collections::BTreeMap;
use std::fmt;

use aurora_plugin_api::{ApiVersion, PluginManifest, PluginManifestError};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Canonical SHA-256 digest attached to an admitted plugin package.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PackageDigest(String);

impl PackageDigest {
    /// Validates and stores one lowercase hexadecimal SHA-256 digest.
    pub fn parse(value: impl Into<String>) -> Result<Self, PluginRegistryError> {
        let value = value.into();
        let valid = value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if !valid {
            return Err(PluginRegistryError::InvalidPackageDigest(value));
        }
        Ok(Self(value))
    }

    /// Returns the canonical lowercase hexadecimal digest.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PackageDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// An admitted immutable plugin package identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginPackageRecord {
    /// Validated plugin manifest carried by the package.
    pub manifest: PluginManifest,
    /// SHA-256 digest of the complete immutable package artifact.
    pub digest: PackageDigest,
}

impl PluginPackageRecord {
    /// Creates a package record from an already computed canonical SHA-256 digest.
    pub fn new(
        manifest: PluginManifest,
        digest: impl Into<String>,
    ) -> Result<Self, PluginRegistryError> {
        Ok(Self {
            manifest,
            digest: PackageDigest::parse(digest)?,
        })
    }
}

/// Update slots for one stable plugin ID.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PluginSlot {
    active: Option<PluginPackageRecord>,
    staged: Option<PluginPackageRecord>,
    rollback_target: Option<PluginPackageRecord>,
}

impl PluginSlot {
    /// Package currently selected for execution.
    pub fn active(&self) -> Option<&PluginPackageRecord> {
        self.active.as_ref()
    }

    /// Package admitted for a future atomic commit.
    pub fn staged(&self) -> Option<&PluginPackageRecord> {
        self.staged.as_ref()
    }

    /// Last package retained for one-step rollback/roll-forward.
    pub fn rollback_target(&self) -> Option<&PluginPackageRecord> {
        self.rollback_target.as_ref()
    }
}

/// In-memory source of truth for admitted application-plugin packages.
///
/// Persistence and filesystem activation belong to later Plugin Host slices.
/// The state transitions here are deliberately deterministic so those layers
/// can persist/execute the same contract without redefining update semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRegistry {
    host_api: ApiVersion,
    slots: BTreeMap<String, PluginSlot>,
}

impl PluginRegistry {
    /// Creates an empty registry for one concrete Aurora Plugin Host API.
    pub fn new(host_api: ApiVersion) -> Self {
        Self {
            host_api,
            slots: BTreeMap::new(),
        }
    }

    /// Host API used for all package admission decisions.
    pub const fn host_api(&self) -> ApiVersion {
        self.host_api
    }

    /// Returns the update slot for one plugin ID.
    pub fn slot(&self, plugin_id: &str) -> Option<&PluginSlot> {
        self.slots.get(plugin_id)
    }

    /// Returns all known plugin IDs in deterministic order.
    pub fn plugin_ids(&self) -> impl Iterator<Item = &str> {
        self.slots.keys().map(String::as_str)
    }

    /// Validates and stages a package without changing the active version.
    ///
    /// A staged package is never overwritten implicitly. Call
    /// [`Self::discard_staged`] before staging another candidate for the same
    /// plugin. The same `(plugin ID, package version)` must always map to the
    /// same digest while it remains known by any registry slot.
    pub fn stage(&mut self, package: PluginPackageRecord) -> Result<(), PluginRegistryError> {
        package.manifest.validate_for_host(self.host_api)?;
        let plugin_id = package.manifest.id.clone();

        if let Some(slot) = self.slots.get(&plugin_id) {
            ensure_version_digest_consistency(slot, &package)?;
            if same_package(slot.active.as_ref(), &package) {
                return Err(PluginRegistryError::PackageAlreadyActive {
                    plugin_id,
                    version: package.manifest.version,
                });
            }
            if let Some(existing) = slot.staged.as_ref() {
                return Err(PluginRegistryError::StagedPackageExists {
                    plugin_id,
                    version: existing.manifest.version.clone(),
                });
            }
        }

        self.slots.entry(plugin_id).or_default().staged = Some(package);
        Ok(())
    }

    /// Atomically promotes the staged package to active registry state.
    ///
    /// The previous active package becomes the one-step rollback target. An
    /// initial installation has no rollback target.
    pub fn commit_staged(&mut self, plugin_id: &str) -> Result<(), PluginRegistryError> {
        let slot = self
            .slots
            .get_mut(plugin_id)
            .ok_or_else(|| PluginRegistryError::UnknownPlugin(plugin_id.to_owned()))?;
        let staged = slot
            .staged
            .take()
            .ok_or_else(|| PluginRegistryError::NoStagedPackage(plugin_id.to_owned()))?;
        let previous = slot.active.replace(staged);
        slot.rollback_target = previous;
        Ok(())
    }

    /// Discards a candidate package without disturbing active/rollback state.
    pub fn discard_staged(&mut self, plugin_id: &str) -> Result<(), PluginRegistryError> {
        let slot = self
            .slots
            .get_mut(plugin_id)
            .ok_or_else(|| PluginRegistryError::UnknownPlugin(plugin_id.to_owned()))?;
        if slot.staged.take().is_none() {
            return Err(PluginRegistryError::NoStagedPackage(plugin_id.to_owned()));
        }
        Ok(())
    }

    /// Swaps the active package with the retained rollback target.
    ///
    /// This supports deterministic rollback and explicit roll-forward using the
    /// same operation. Any uncommitted staged candidate is discarded first so
    /// rollback cannot accidentally activate a package that was only staged.
    pub fn rollback(&mut self, plugin_id: &str) -> Result<(), PluginRegistryError> {
        let slot = self
            .slots
            .get_mut(plugin_id)
            .ok_or_else(|| PluginRegistryError::UnknownPlugin(plugin_id.to_owned()))?;
        let rollback_target = slot
            .rollback_target
            .take()
            .ok_or_else(|| PluginRegistryError::NoRollbackTarget(plugin_id.to_owned()))?;
        slot.staged = None;
        let replaced = slot.active.replace(rollback_target);
        slot.rollback_target = replaced;
        Ok(())
    }
}

fn same_package(existing: Option<&PluginPackageRecord>, candidate: &PluginPackageRecord) -> bool {
    existing.is_some_and(|known| {
        known.manifest.version == candidate.manifest.version && known.digest == candidate.digest
    })
}

fn ensure_version_digest_consistency(
    slot: &PluginSlot,
    candidate: &PluginPackageRecord,
) -> Result<(), PluginRegistryError> {
    for known in [
        slot.active.as_ref(),
        slot.staged.as_ref(),
        slot.rollback_target.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        if known.manifest.version == candidate.manifest.version && known.digest != candidate.digest {
            return Err(PluginRegistryError::VersionDigestMismatch {
                plugin_id: candidate.manifest.id.clone(),
                version: candidate.manifest.version.clone(),
                known_digest: known.digest.clone(),
                candidate_digest: candidate.digest.clone(),
            });
        }
    }
    Ok(())
}

/// Fail-closed plugin package registry errors.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PluginRegistryError {
    /// Plugin manifest is incompatible or malformed.
    #[error(transparent)]
    Manifest(#[from] PluginManifestError),
    /// SHA-256 package digest is not canonical lowercase hexadecimal.
    #[error("invalid plugin package SHA-256 digest `{0}`")]
    InvalidPackageDigest(String),
    /// Stable plugin ID is not known to the registry.
    #[error("unknown plugin `{0}`")]
    UnknownPlugin(String),
    /// A different candidate is already staged and requires explicit discard.
    #[error("plugin `{plugin_id}` already has staged version `{version}`")]
    StagedPackageExists { plugin_id: String, version: String },
    /// Candidate exactly matches the active package.
    #[error("plugin `{plugin_id}` version `{version}` is already active")]
    PackageAlreadyActive { plugin_id: String, version: String },
    /// Commit/discard was requested without an admitted candidate.
    #[error("plugin `{0}` has no staged package")]
    NoStagedPackage(String),
    /// Rollback was requested without a previous committed package.
    #[error("plugin `{0}` has no rollback target")]
    NoRollbackTarget(String),
    /// One immutable package version was observed with two different digests.
    #[error(
        "plugin `{plugin_id}` version `{version}` digest changed from {known_digest} to {candidate_digest}"
    )]
    VersionDigestMismatch {
        plugin_id: String,
        version: String,
        known_digest: PackageDigest,
        candidate_digest: PackageDigest,
    },
}

#[cfg(test)]
mod tests {
    use aurora_plugin_api::{
        ApiCompatibility, PluginCapability, PluginPermission, PluginProtocol,
        PLUGIN_MANIFEST_SCHEMA_VERSION,
    };

    use super::*;

    const HOST_API: ApiVersion = ApiVersion { major: 1, minor: 0 };

    fn manifest(id: &str, version: &str) -> PluginManifest {
        PluginManifest {
            schema_version: PLUGIN_MANIFEST_SCHEMA_VERSION,
            id: id.to_owned(),
            name: id.to_owned(),
            version: version.to_owned(),
            api: ApiCompatibility {
                major: 1,
                min_minor: 0,
                max_minor: 1,
            },
            capabilities: vec![PluginCapability::MediaSource],
            permissions: vec![PluginPermission::LocalMediaRead],
            entrypoint: "bin/plugin".to_owned(),
            protocol: PluginProtocol::JsonLinesStdioV1,
        }
    }

    fn digest(byte: char) -> String {
        std::iter::repeat(byte).take(64).collect()
    }

    fn package(id: &str, version: &str, digest_byte: char) -> PluginPackageRecord {
        PluginPackageRecord::new(manifest(id, version), digest(digest_byte)).unwrap()
    }

    #[test]
    fn initial_install_is_staged_then_committed() {
        let mut registry = PluginRegistry::new(HOST_API);
        registry
            .stage(package("org.aurora.localmusic", "1.0.0", 'a'))
            .unwrap();

        let staged = registry
            .slot("org.aurora.localmusic")
            .unwrap()
            .staged()
            .unwrap();
        assert_eq!(staged.manifest.version, "1.0.0");
        assert!(registry
            .slot("org.aurora.localmusic")
            .unwrap()
            .active()
            .is_none());

        registry.commit_staged("org.aurora.localmusic").unwrap();
        let slot = registry.slot("org.aurora.localmusic").unwrap();
        assert_eq!(slot.active().unwrap().manifest.version, "1.0.0");
        assert!(slot.staged().is_none());
        assert!(slot.rollback_target().is_none());
    }

    #[test]
    fn staging_update_does_not_touch_active_package() {
        let mut registry = installed_registry();
        registry
            .stage(package("org.aurora.localmusic", "2.0.0", 'b'))
            .unwrap();

        let slot = registry.slot("org.aurora.localmusic").unwrap();
        assert_eq!(slot.active().unwrap().manifest.version, "1.0.0");
        assert_eq!(slot.staged().unwrap().manifest.version, "2.0.0");
    }

    #[test]
    fn committing_update_keeps_previous_package_for_rollback() {
        let mut registry = installed_registry();
        registry
            .stage(package("org.aurora.localmusic", "2.0.0", 'b'))
            .unwrap();
        registry.commit_staged("org.aurora.localmusic").unwrap();

        let slot = registry.slot("org.aurora.localmusic").unwrap();
        assert_eq!(slot.active().unwrap().manifest.version, "2.0.0");
        assert_eq!(slot.rollback_target().unwrap().manifest.version, "1.0.0");
    }

    #[test]
    fn rollback_swaps_active_and_previous_package() {
        let mut registry = installed_registry();
        registry
            .stage(package("org.aurora.localmusic", "2.0.0", 'b'))
            .unwrap();
        registry.commit_staged("org.aurora.localmusic").unwrap();
        registry.rollback("org.aurora.localmusic").unwrap();

        let slot = registry.slot("org.aurora.localmusic").unwrap();
        assert_eq!(slot.active().unwrap().manifest.version, "1.0.0");
        assert_eq!(slot.rollback_target().unwrap().manifest.version, "2.0.0");

        registry.rollback("org.aurora.localmusic").unwrap();
        let slot = registry.slot("org.aurora.localmusic").unwrap();
        assert_eq!(slot.active().unwrap().manifest.version, "2.0.0");
        assert_eq!(slot.rollback_target().unwrap().manifest.version, "1.0.0");
    }

    #[test]
    fn incompatible_manifest_is_rejected_without_mutation() {
        let mut registry = installed_registry();
        let before = registry.clone();
        let mut incompatible = package("org.aurora.spotify", "1.0.0", 'c');
        incompatible.manifest.api.major = 2;

        assert!(matches!(
            registry.stage(incompatible),
            Err(PluginRegistryError::Manifest(
                PluginManifestError::IncompatibleHostApi { .. }
            ))
        ));
        assert_eq!(registry, before);
    }

    #[test]
    fn same_version_with_different_digest_is_rejected() {
        let mut registry = installed_registry();
        let error = registry
            .stage(package("org.aurora.localmusic", "1.0.0", 'f'))
            .unwrap_err();

        assert!(matches!(
            error,
            PluginRegistryError::VersionDigestMismatch { .. }
        ));
        assert_eq!(
            registry
                .slot("org.aurora.localmusic")
                .unwrap()
                .active()
                .unwrap()
                .digest
                .as_str(),
            digest('a')
        );
    }

    #[test]
    fn invalid_digest_is_rejected_before_registry_mutation() {
        assert_eq!(
            PluginPackageRecord::new(manifest("org.aurora.localmusic", "1.0.0"), "ABC"),
            Err(PluginRegistryError::InvalidPackageDigest("ABC".to_owned()))
        );
    }

    #[test]
    fn one_staged_candidate_cannot_be_replaced_implicitly() {
        let mut registry = installed_registry();
        registry
            .stage(package("org.aurora.localmusic", "2.0.0", 'b'))
            .unwrap();

        assert_eq!(
            registry
                .stage(package("org.aurora.localmusic", "3.0.0", 'c'))
                .unwrap_err(),
            PluginRegistryError::StagedPackageExists {
                plugin_id: "org.aurora.localmusic".to_owned(),
                version: "2.0.0".to_owned(),
            }
        );
    }

    #[test]
    fn plugin_slots_are_independent_and_deterministically_ordered() {
        let mut registry = installed_registry();
        registry
            .stage(package("org.aurora.spotify", "1.0.0", 'c'))
            .unwrap();
        registry.commit_staged("org.aurora.spotify").unwrap();

        assert_eq!(
            registry.plugin_ids().collect::<Vec<_>>(),
            vec!["org.aurora.localmusic", "org.aurora.spotify"]
        );
        assert_eq!(
            registry
                .slot("org.aurora.localmusic")
                .unwrap()
                .active()
                .unwrap()
                .manifest
                .version,
            "1.0.0"
        );
        assert_eq!(
            registry
                .slot("org.aurora.spotify")
                .unwrap()
                .active()
                .unwrap()
                .manifest
                .version,
            "1.0.0"
        );
    }

    fn installed_registry() -> PluginRegistry {
        let mut registry = PluginRegistry::new(HOST_API);
        registry
            .stage(package("org.aurora.localmusic", "1.0.0", 'a'))
            .unwrap();
        registry.commit_staged("org.aurora.localmusic").unwrap();
        registry
    }
}
