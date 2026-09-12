//! Aurora application-plugin host foundations.
//!
//! This first slice owns package admission and atomic staged update/rollback
//! state only. It does not spawn plugin processes or touch the realtime path.

use std::collections::BTreeMap;
use std::fmt;

use aurora_plugin_api::{ApiVersion, PluginManifest, PluginManifestError};
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

/// Current persistent plugin-registry snapshot schema.
pub const PLUGIN_REGISTRY_SCHEMA_VERSION: u16 = 1;

/// Canonical SHA-256 digest attached to an admitted plugin package.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct PackageDigest(String);

impl PackageDigest {
    /// Validates and stores one lowercase hexadecimal SHA-256 digest.
    pub fn parse(value: impl Into<String>) -> Result<Self, PluginRegistryError> {
        let value = value.into();
        if !is_canonical_sha256(&value) {
            return Err(PluginRegistryError::InvalidPackageDigest(value));
        }
        Ok(Self(value))
    }

    /// Returns the canonical lowercase hexadecimal digest.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for PackageDigest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        PackageDigest::parse(value).map_err(serde::de::Error::custom)
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

/// Update slots and immutable package-identity history for one stable plugin ID.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PluginSlot {
    active: Option<PluginPackageRecord>,
    staged: Option<PluginPackageRecord>,
    rollback_target: Option<PluginPackageRecord>,
    known_versions: BTreeMap<String, PackageDigest>,
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

    /// Returns the immutable digest remembered for one previously admitted
    /// package version, including versions no longer active/rollback candidates.
    pub fn known_digest(&self, version: &str) -> Option<&PackageDigest> {
        self.known_versions.get(version)
    }

    fn packages(&self) -> impl Iterator<Item = &PluginPackageRecord> {
        [
            self.active.as_ref(),
            self.staged.as_ref(),
            self.rollback_target.as_ref(),
        ]
        .into_iter()
        .flatten()
    }

    fn is_empty(&self) -> bool {
        self.active.is_none()
            && self.staged.is_none()
            && self.rollback_target.is_none()
            && self.known_versions.is_empty()
    }
}

/// Versioned persistent representation of the plugin package registry.
///
/// `host_api_at_write` is diagnostic provenance only. Restore always validates
/// every retained package against the *current* host API supplied by the caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginRegistrySnapshot {
    /// Snapshot schema generation.
    pub schema_version: u16,
    /// Host API active when the snapshot was written.
    pub host_api_at_write: ApiVersion,
    /// Deterministically ordered plugin slots and package-identity history.
    pub slots: BTreeMap<String, PluginSlot>,
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

    /// Restores a versioned snapshot after revalidating every retained package
    /// against the current Plugin Host API and checking immutable identities.
    pub fn from_snapshot(
        host_api: ApiVersion,
        snapshot: PluginRegistrySnapshot,
    ) -> Result<Self, PluginRegistryError> {
        if snapshot.schema_version != PLUGIN_REGISTRY_SCHEMA_VERSION {
            return Err(PluginRegistryError::UnsupportedRegistrySchema {
                actual: snapshot.schema_version,
                expected: PLUGIN_REGISTRY_SCHEMA_VERSION,
            });
        }

        for (plugin_id, slot) in &snapshot.slots {
            validate_restored_slot(host_api, plugin_id, slot)?;
        }

        Ok(Self {
            host_api,
            slots: snapshot.slots,
        })
    }

    /// Host API used for all package admission decisions.
    pub const fn host_api(&self) -> ApiVersion {
        self.host_api
    }

    /// Produces a deterministic persistent snapshot of the current registry.
    pub fn snapshot(&self) -> PluginRegistrySnapshot {
        PluginRegistrySnapshot {
            schema_version: PLUGIN_REGISTRY_SCHEMA_VERSION,
            host_api_at_write: self.host_api,
            slots: self.slots.clone(),
        }
    }

    /// Returns the update/history slot for one plugin ID.
    pub fn slot(&self, plugin_id: &str) -> Option<&PluginSlot> {
        self.slots.get(plugin_id)
    }

    /// Returns all known plugin IDs in deterministic order, including IDs kept
    /// only for immutable package-version history.
    pub fn plugin_ids(&self) -> impl Iterator<Item = &str> {
        self.slots.keys().map(String::as_str)
    }

    /// Validates and stages a package without changing the active version.
    ///
    /// A staged package is never overwritten implicitly. Call
    /// [`Self::discard_staged`] before staging another candidate for the same
    /// plugin. Once a `(plugin ID, package version)` is admitted, its digest is
    /// remembered permanently by this registry snapshot and may not change even
    /// after that package falls out of active/rollback slots.
    pub fn stage(&mut self, package: PluginPackageRecord) -> Result<(), PluginRegistryError> {
        package.manifest.validate_for_host(self.host_api)?;
        let plugin_id = package.manifest.id.clone();
        let version = package.manifest.version.clone();

        if let Some(slot) = self.slots.get(&plugin_id) {
            ensure_known_version_consistency(&plugin_id, slot, &package)?;
            if same_package(slot.active.as_ref(), &package) {
                return Err(PluginRegistryError::PackageAlreadyActive { plugin_id, version });
            }
            if let Some(existing) = slot.staged.as_ref() {
                return Err(PluginRegistryError::StagedPackageExists {
                    plugin_id,
                    version: existing.manifest.version.clone(),
                });
            }
        }

        let slot = self.slots.entry(plugin_id).or_default();
        slot.known_versions
            .entry(version)
            .or_insert_with(|| package.digest.clone());
        slot.staged = Some(package);
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
    ///
    /// Its admitted `(version, digest)` identity remains remembered so the same
    /// version cannot later be silently replaced by different package bytes.
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

fn is_canonical_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && segment
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && segment
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
}

fn same_package(existing: Option<&PluginPackageRecord>, candidate: &PluginPackageRecord) -> bool {
    existing.is_some_and(|known| {
        known.manifest.version == candidate.manifest.version && known.digest == candidate.digest
    })
}

fn ensure_known_version_consistency(
    plugin_id: &str,
    slot: &PluginSlot,
    candidate: &PluginPackageRecord,
) -> Result<(), PluginRegistryError> {
    if let Some(known_digest) = slot.known_versions.get(&candidate.manifest.version) {
        if known_digest != &candidate.digest {
            return Err(PluginRegistryError::VersionDigestMismatch {
                plugin_id: plugin_id.to_owned(),
                version: candidate.manifest.version.clone(),
                known_digest: known_digest.clone(),
                candidate_digest: candidate.digest.clone(),
            });
        }
    }
    Ok(())
}

fn validate_restored_slot(
    host_api: ApiVersion,
    plugin_id: &str,
    slot: &PluginSlot,
) -> Result<(), PluginRegistryError> {
    if slot.is_empty() {
        return Err(PluginRegistryError::EmptyRestoredSlot(plugin_id.to_owned()));
    }
    if !is_canonical_plugin_id(plugin_id) {
        return Err(PluginRegistryError::InvalidRestoredPluginId(
            plugin_id.to_owned(),
        ));
    }
    if slot
        .known_versions
        .keys()
        .any(|version| version.trim().is_empty())
    {
        return Err(PluginRegistryError::EmptyKnownVersion(plugin_id.to_owned()));
    }

    for package in slot.packages() {
        if package.manifest.id != plugin_id {
            return Err(PluginRegistryError::RestoredPluginIdMismatch {
                slot_id: plugin_id.to_owned(),
                manifest_id: package.manifest.id.clone(),
            });
        }
        package.manifest.validate_for_host(host_api)?;
        let known_digest = slot
            .known_versions
            .get(&package.manifest.version)
            .ok_or_else(|| PluginRegistryError::MissingKnownVersionIdentity {
                plugin_id: plugin_id.to_owned(),
                version: package.manifest.version.clone(),
            })?;
        if known_digest != &package.digest {
            return Err(PluginRegistryError::VersionDigestMismatch {
                plugin_id: plugin_id.to_owned(),
                version: package.manifest.version.clone(),
                known_digest: known_digest.clone(),
                candidate_digest: package.digest.clone(),
            });
        }
    }

    if let (Some(active), Some(staged)) = (slot.active.as_ref(), slot.staged.as_ref()) {
        if active == staged {
            return Err(PluginRegistryError::PackageAlreadyActive {
                plugin_id: plugin_id.to_owned(),
                version: active.manifest.version.clone(),
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
    /// Persistent registry schema is not supported by this host.
    #[error("unsupported plugin registry schema {actual}; expected {expected}")]
    UnsupportedRegistrySchema { actual: u16, expected: u16 },
    /// Stable plugin ID is not known to the registry.
    #[error("unknown plugin `{0}`")]
    UnknownPlugin(String),
    /// A persisted slot cannot be completely empty.
    #[error("restored plugin slot `{0}` is empty")]
    EmptyRestoredSlot(String),
    /// Historical-only restored IDs must still use canonical plugin-ID syntax.
    #[error("restored plugin id `{0}` is invalid")]
    InvalidRestoredPluginId(String),
    /// Package-history entries require nonempty version identities.
    #[error("restored plugin `{0}` contains an empty known package version")]
    EmptyKnownVersion(String),
    /// Snapshot map key and package manifest ID disagree.
    #[error("restored slot `{slot_id}` contains package for `{manifest_id}`")]
    RestoredPluginIdMismatch {
        slot_id: String,
        manifest_id: String,
    },
    /// A retained runtime package lacks the immutable historical identity entry.
    #[error("plugin `{plugin_id}` version `{version}` is missing package identity history")]
    MissingKnownVersionIdentity { plugin_id: String, version: String },
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
        assert_eq!(slot.known_digest("1.0.0").unwrap().as_str(), digest('a'));
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
    fn old_version_digest_remains_pinned_after_multiple_upgrades() {
        let mut registry = installed_registry();
        registry
            .stage(package("org.aurora.localmusic", "2.0.0", 'b'))
            .unwrap();
        registry.commit_staged("org.aurora.localmusic").unwrap();
        registry
            .stage(package("org.aurora.localmusic", "3.0.0", 'c'))
            .unwrap();
        registry.commit_staged("org.aurora.localmusic").unwrap();

        let slot = registry.slot("org.aurora.localmusic").unwrap();
        assert_eq!(slot.active().unwrap().manifest.version, "3.0.0");
        assert_eq!(slot.rollback_target().unwrap().manifest.version, "2.0.0");
        assert_eq!(slot.known_digest("1.0.0").unwrap().as_str(), digest('a'));

        assert!(matches!(
            registry.stage(package("org.aurora.localmusic", "1.0.0", 'f')),
            Err(PluginRegistryError::VersionDigestMismatch { .. })
        ));
    }

    #[test]
    fn discarded_candidate_identity_remains_immutable() {
        let mut registry = PluginRegistry::new(HOST_API);
        registry
            .stage(package("org.aurora.spotify", "1.0.0", 'c'))
            .unwrap();
        registry.discard_staged("org.aurora.spotify").unwrap();

        let slot = registry.slot("org.aurora.spotify").unwrap();
        assert!(slot.active().is_none());
        assert!(slot.staged().is_none());
        assert_eq!(slot.known_digest("1.0.0").unwrap().as_str(), digest('c'));
        assert!(matches!(
            registry.stage(package("org.aurora.spotify", "1.0.0", 'd')),
            Err(PluginRegistryError::VersionDigestMismatch { .. })
        ));
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

    #[test]
    fn snapshot_round_trip_revalidates_against_current_host() {
        let mut registry = installed_registry();
        registry
            .stage(package("org.aurora.localmusic", "2.0.0", 'b'))
            .unwrap();
        let json = serde_json::to_string(&registry.snapshot()).unwrap();
        let snapshot: PluginRegistrySnapshot = serde_json::from_str(&json).unwrap();
        let restored = PluginRegistry::from_snapshot(HOST_API, snapshot).unwrap();
        assert_eq!(restored, registry);
    }

    #[test]
    fn restore_rejects_packages_incompatible_with_new_host() {
        let registry = installed_registry();
        let snapshot = registry.snapshot();
        let newer_incompatible_host = ApiVersion { major: 2, minor: 0 };

        assert!(matches!(
            PluginRegistry::from_snapshot(newer_incompatible_host, snapshot),
            Err(PluginRegistryError::Manifest(
                PluginManifestError::IncompatibleHostApi { .. }
            ))
        ));
    }

    #[test]
    fn digest_deserialization_is_fail_closed() {
        let bad = format!(
            "{{\"manifest\":{},\"digest\":\"ABC\"}}",
            serde_json::to_string(&manifest("org.aurora.localmusic", "1.0.0")).unwrap()
        );
        assert!(serde_json::from_str::<PluginPackageRecord>(&bad).is_err());
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
