//! Versioned control-plane source arbitration for Aurora.
//!
//! This module deliberately owns only validated media/control intents and source
//! lifecycle state. It exposes no realtime buffers, renderer objects, DSP state,
//! provider SDK types, or hardware handles.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::PluginPermission;

/// Current Source Manager API schema.
pub const SOURCE_MANAGER_SCHEMA_VERSION: u16 = 1;

/// Stable origin of a source session.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    LocalMedia,
    ProviderControl,
    NetworkMedia,
    LiveInput,
    TvEarc,
}

/// Lifecycle visible to the control plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceState {
    Registered,
    Available,
    Preparing,
    Active,
    Paused,
    Draining,
    Failed,
    Stopped,
}

/// Typed reference handed to Aurora's media/decode path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlayableMediaRef {
    LocalFile {
        path: String,
    },
    NetworkUrl {
        url: String,
    },
    ProviderResolved {
        provider_id: String,
        media_id: String,
    },
    LiveEndpoint {
        endpoint_id: String,
    },
    TvEarc {
        input_id: String,
    },
}

/// Non-realtime metadata associated with a source.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SourceMetadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub artwork_ref: Option<String>,
}

/// Media capabilities exposed before activation.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SourceCapabilities {
    pub can_seek: bool,
    pub can_next: bool,
    pub can_previous: bool,
    pub can_pause: bool,
    pub codec_hint: Option<String>,
    pub channel_layout_hint: Option<String>,
    pub sample_rate_hz: Option<u32>,
}

/// Authorization provenance supplied by Plugin Host or trusted built-in source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceAuthorization {
    pub principal_id: String,
    pub permissions: Vec<PluginPermission>,
    pub builtin_trusted: bool,
}

impl SourceAuthorization {
    fn permits_playback_control(&self) -> bool {
        self.builtin_trusted
            || self
                .permissions
                .contains(&PluginPermission::PlaybackControl)
    }
}

/// Monotonic source session identity. Generation invalidates stale commands.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SourceSessionId {
    pub source_id: String,
    pub generation: u64,
}

/// Source definition submitted to Source Manager.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRegistration {
    pub schema_version: u16,
    pub source_id: String,
    pub provider_id: String,
    pub kind: SourceKind,
    pub priority: u16,
    pub authorization: SourceAuthorization,
}

/// Candidate prepared for transactional activation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreparedSource {
    pub session: SourceSessionId,
    pub media: PlayableMediaRef,
    pub metadata: SourceMetadata,
    pub capabilities: SourceCapabilities,
}

/// Transport request scoped to one concrete generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportIntent {
    Play,
    Pause,
    Resume,
    SeekMillis(u64),
    Next,
    Previous,
    Stop,
}

/// Control-plane source record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRecord {
    registration: SourceRegistration,
    generation: u64,
    state: SourceState,
    prepared: Option<PreparedSource>,
    last_error: Option<String>,
}

impl SourceRecord {
    pub fn registration(&self) -> &SourceRegistration {
        &self.registration
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub const fn state(&self) -> SourceState {
        self.state
    }

    pub fn prepared(&self) -> Option<&PreparedSource> {
        self.prepared.as_ref()
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }
}

/// Transactional Source Manager. No method mutates realtime engine state.
#[derive(Debug, Default)]
pub struct SourceManager {
    sources: BTreeMap<String, SourceRecord>,
    active: Option<SourceSessionId>,
}

impl SourceManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        registration: SourceRegistration,
    ) -> Result<SourceSessionId, SourceManagerError> {
        validate_registration(&registration)?;
        if self.sources.contains_key(&registration.source_id) {
            return Err(SourceManagerError::SourceAlreadyRegistered(
                registration.source_id,
            ));
        }
        let session = SourceSessionId {
            source_id: registration.source_id.clone(),
            generation: 1,
        };
        self.sources.insert(
            registration.source_id.clone(),
            SourceRecord {
                registration,
                generation: 1,
                state: SourceState::Available,
                prepared: None,
                last_error: None,
            },
        );
        Ok(session)
    }

    /// Replaces a source definition and increments its generation, invalidating
    /// every command or prepared candidate from the previous generation.
    pub fn replace_registration(
        &mut self,
        registration: SourceRegistration,
    ) -> Result<SourceSessionId, SourceManagerError> {
        validate_registration(&registration)?;
        let source_id = registration.source_id.clone();
        let previous = self
            .sources
            .get(&source_id)
            .ok_or_else(|| SourceManagerError::UnknownSource(source_id.clone()))?;
        let generation = previous
            .generation
            .checked_add(1)
            .ok_or_else(|| SourceManagerError::GenerationExhausted(source_id.clone()))?;
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.source_id == source_id)
        {
            self.active = None;
        }
        self.sources.insert(
            source_id.clone(),
            SourceRecord {
                registration,
                generation,
                state: SourceState::Available,
                prepared: None,
                last_error: None,
            },
        );
        Ok(SourceSessionId {
            source_id,
            generation,
        })
    }

    /// Validates and stages a candidate without disturbing the currently active source.
    pub fn prepare(
        &mut self,
        session: &SourceSessionId,
        media: PlayableMediaRef,
        metadata: SourceMetadata,
        capabilities: SourceCapabilities,
    ) -> Result<PreparedSource, SourceManagerError> {
        validate_media_ref(&media)?;
        let record = self.record_for_session_mut(session)?;
        if !record.registration.authorization.permits_playback_control() {
            return Err(SourceManagerError::PlaybackControlDenied(
                record.registration.authorization.principal_id.clone(),
            ));
        }
        record.state = SourceState::Preparing;
        record.last_error = None;
        let prepared = PreparedSource {
            session: session.clone(),
            media,
            metadata,
            capabilities,
        };
        record.prepared = Some(prepared.clone());
        record.state = SourceState::Available;
        Ok(prepared)
    }

    /// Records a preparation failure while leaving the active source untouched.
    pub fn fail_prepare(
        &mut self,
        session: &SourceSessionId,
        detail: impl Into<String>,
    ) -> Result<(), SourceManagerError> {
        let record = self.record_for_session_mut(session)?;
        record.state = SourceState::Failed;
        record.prepared = None;
        record.last_error = Some(detail.into());
        Ok(())
    }

    /// Atomically selects an already prepared candidate as the active source.
    pub fn activate(&mut self, session: &SourceSessionId) -> Result<(), SourceManagerError> {
        {
            let record = self.record_for_session(session)?;
            if record.prepared.is_none() {
                return Err(SourceManagerError::SourceNotPrepared(
                    session.source_id.clone(),
                ));
            }
        }

        if let Some(previous) = self.active.clone() {
            if previous != *session {
                if let Ok(old) = self.record_for_session_mut(&previous) {
                    old.state = SourceState::Draining;
                }
            }
        }

        let record = self.record_for_session_mut(session)?;
        record.state = SourceState::Active;
        self.active = Some(session.clone());
        Ok(())
    }

    pub fn transport(
        &mut self,
        session: &SourceSessionId,
        intent: TransportIntent,
    ) -> Result<(), SourceManagerError> {
        let active = self
            .active
            .as_ref()
            .ok_or(SourceManagerError::NoActiveSource)?;
        if active != session {
            return Err(SourceManagerError::StaleOrInactiveSession {
                source_id: session.source_id.clone(),
                generation: session.generation,
            });
        }

        let record = self.record_for_session_mut(session)?;
        if !record.registration.authorization.permits_playback_control() {
            return Err(SourceManagerError::PlaybackControlDenied(
                record.registration.authorization.principal_id.clone(),
            ));
        }

        let capabilities = record
            .prepared
            .as_ref()
            .map(|p| &p.capabilities)
            .ok_or_else(|| SourceManagerError::SourceNotPrepared(session.source_id.clone()))?;

        match intent {
            TransportIntent::Play | TransportIntent::Resume => record.state = SourceState::Active,
            TransportIntent::Pause if capabilities.can_pause => record.state = SourceState::Paused,
            TransportIntent::SeekMillis(_) if capabilities.can_seek => {}
            TransportIntent::Next if capabilities.can_next => {}
            TransportIntent::Previous if capabilities.can_previous => {}
            TransportIntent::Stop => {
                record.state = SourceState::Stopped;
                self.active = None;
            }
            other => return Err(SourceManagerError::UnsupportedTransport(other)),
        }
        Ok(())
    }

    pub fn active_session(&self) -> Option<&SourceSessionId> {
        self.active.as_ref()
    }

    pub fn record(&self, source_id: &str) -> Option<&SourceRecord> {
        self.sources.get(source_id)
    }

    /// Returns the highest-priority prepared candidate, deterministically
    /// breaking ties by source ID. Higher numeric priority wins.
    pub fn preferred_prepared(&self) -> Option<&PreparedSource> {
        self.sources
            .values()
            .filter_map(|record| record.prepared.as_ref().map(|prepared| (record, prepared)))
            .max_by(|(left, _), (right, _)| {
                left.registration
                    .priority
                    .cmp(&right.registration.priority)
                    .then_with(|| {
                        right
                            .registration
                            .source_id
                            .cmp(&left.registration.source_id)
                    })
            })
            .map(|(_, prepared)| prepared)
    }

    fn record_for_session(
        &self,
        session: &SourceSessionId,
    ) -> Result<&SourceRecord, SourceManagerError> {
        let record = self
            .sources
            .get(&session.source_id)
            .ok_or_else(|| SourceManagerError::UnknownSource(session.source_id.clone()))?;
        ensure_generation(record, session)?;
        Ok(record)
    }

    fn record_for_session_mut(
        &mut self,
        session: &SourceSessionId,
    ) -> Result<&mut SourceRecord, SourceManagerError> {
        let record = self
            .sources
            .get_mut(&session.source_id)
            .ok_or_else(|| SourceManagerError::UnknownSource(session.source_id.clone()))?;
        ensure_generation(record, session)?;
        Ok(record)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SourceManagerError {
    #[error("unsupported source manager schema {actual}; expected {expected}")]
    UnsupportedSchema { actual: u16, expected: u16 },
    #[error("invalid source id `{0}`")]
    InvalidSourceId(String),
    #[error("invalid provider id `{0}`")]
    InvalidProviderId(String),
    #[error("source `{0}` is already registered")]
    SourceAlreadyRegistered(String),
    #[error("unknown source `{0}`")]
    UnknownSource(String),
    #[error("source generation exhausted for `{0}`")]
    GenerationExhausted(String),
    #[error("stale generation for `{source_id}`: got {actual}, current {current}")]
    StaleGeneration {
        source_id: String,
        actual: u64,
        current: u64,
    },
    #[error("playback control denied for principal `{0}`")]
    PlaybackControlDenied(String),
    #[error("source `{0}` has not been prepared")]
    SourceNotPrepared(String),
    #[error("no active source")]
    NoActiveSource,
    #[error("session is stale or inactive: {source_id}@{generation}")]
    StaleOrInactiveSession { source_id: String, generation: u64 },
    #[error("unsupported transport intent {0:?}")]
    UnsupportedTransport(TransportIntent),
    #[error("invalid playable media reference: {0}")]
    InvalidMediaReference(String),
}

fn ensure_generation(
    record: &SourceRecord,
    session: &SourceSessionId,
) -> Result<(), SourceManagerError> {
    if record.generation != session.generation {
        return Err(SourceManagerError::StaleGeneration {
            source_id: session.source_id.clone(),
            actual: session.generation,
            current: record.generation,
        });
    }
    Ok(())
}

fn validate_registration(registration: &SourceRegistration) -> Result<(), SourceManagerError> {
    if registration.schema_version != SOURCE_MANAGER_SCHEMA_VERSION {
        return Err(SourceManagerError::UnsupportedSchema {
            actual: registration.schema_version,
            expected: SOURCE_MANAGER_SCHEMA_VERSION,
        });
    }
    if !is_canonical_id(&registration.source_id) {
        return Err(SourceManagerError::InvalidSourceId(
            registration.source_id.clone(),
        ));
    }
    if !is_canonical_id(&registration.provider_id) {
        return Err(SourceManagerError::InvalidProviderId(
            registration.provider_id.clone(),
        ));
    }
    Ok(())
}

fn validate_media_ref(media: &PlayableMediaRef) -> Result<(), SourceManagerError> {
    let invalid = match media {
        PlayableMediaRef::LocalFile { path } => path.trim().is_empty(),
        PlayableMediaRef::NetworkUrl { url } => {
            let lower = url.to_ascii_lowercase();
            !(lower.starts_with("https://") || lower.starts_with("http://"))
        }
        PlayableMediaRef::ProviderResolved {
            provider_id,
            media_id,
        } => !is_canonical_id(provider_id) || media_id.trim().is_empty(),
        PlayableMediaRef::LiveEndpoint { endpoint_id } => endpoint_id.trim().is_empty(),
        PlayableMediaRef::TvEarc { input_id } => input_id.trim().is_empty(),
    };
    if invalid {
        return Err(SourceManagerError::InvalidMediaReference(format!(
            "{media:?}"
        )));
    }
    Ok(())
}

fn is_canonical_id(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'_')
        })
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authorization() -> SourceAuthorization {
        SourceAuthorization {
            principal_id: "org.aurora.local".to_owned(),
            permissions: vec![
                PluginPermission::PlaybackControl,
                PluginPermission::LocalMediaRead,
            ],
            builtin_trusted: false,
        }
    }

    fn registration(id: &str, priority: u16) -> SourceRegistration {
        SourceRegistration {
            schema_version: SOURCE_MANAGER_SCHEMA_VERSION,
            source_id: id.to_owned(),
            provider_id: "org.aurora.local".to_owned(),
            kind: SourceKind::LocalMedia,
            priority,
            authorization: authorization(),
        }
    }

    fn media(path: &str) -> PlayableMediaRef {
        PlayableMediaRef::LocalFile {
            path: path.to_owned(),
        }
    }

    fn caps() -> SourceCapabilities {
        SourceCapabilities {
            can_seek: true,
            can_next: true,
            can_previous: true,
            can_pause: true,
            codec_hint: Some("flac".to_owned()),
            channel_layout_hint: Some("stereo".to_owned()),
            sample_rate_hz: Some(48_000),
        }
    }

    #[test]
    fn stale_session_is_rejected_after_replacement() {
        let mut manager = SourceManager::new();
        let old = manager.register(registration("local.main", 10)).unwrap();
        let new = manager
            .replace_registration(registration("local.main", 10))
            .unwrap();
        assert_eq!(new.generation, old.generation + 1);
        assert!(matches!(
            manager.prepare(
                &old,
                media("/music/a.flac"),
                SourceMetadata::default(),
                caps()
            ),
            Err(SourceManagerError::StaleGeneration { .. })
        ));
    }

    #[test]
    fn failed_prepare_leaves_active_source_unchanged() {
        let mut manager = SourceManager::new();
        let first = manager.register(registration("local.first", 10)).unwrap();
        manager
            .prepare(
                &first,
                media("/music/a.flac"),
                SourceMetadata::default(),
                caps(),
            )
            .unwrap();
        manager.activate(&first).unwrap();

        let second = manager.register(registration("local.second", 20)).unwrap();
        manager
            .fail_prepare(&second, "decoder probe failed")
            .unwrap();
        assert_eq!(manager.active_session(), Some(&first));
        assert_eq!(
            manager.record("local.second").unwrap().state(),
            SourceState::Failed
        );
    }

    #[test]
    fn switching_is_prepare_then_atomic_activate() {
        let mut manager = SourceManager::new();
        let first = manager.register(registration("local.first", 10)).unwrap();
        manager
            .prepare(
                &first,
                media("/music/a.flac"),
                SourceMetadata::default(),
                caps(),
            )
            .unwrap();
        manager.activate(&first).unwrap();

        let second = manager.register(registration("local.second", 20)).unwrap();
        manager
            .prepare(
                &second,
                media("/music/b.flac"),
                SourceMetadata::default(),
                caps(),
            )
            .unwrap();
        assert_eq!(manager.active_session(), Some(&first));
        manager.activate(&second).unwrap();
        assert_eq!(manager.active_session(), Some(&second));
        assert_eq!(
            manager.record("local.first").unwrap().state(),
            SourceState::Draining
        );
    }

    #[test]
    fn transport_requires_active_current_generation_and_capability() {
        let mut manager = SourceManager::new();
        let session = manager.register(registration("local.main", 10)).unwrap();
        let mut capabilities = caps();
        capabilities.can_next = false;
        manager
            .prepare(
                &session,
                media("/music/a.flac"),
                SourceMetadata::default(),
                capabilities,
            )
            .unwrap();
        manager.activate(&session).unwrap();
        assert_eq!(
            manager.transport(&session, TransportIntent::Next),
            Err(SourceManagerError::UnsupportedTransport(
                TransportIntent::Next
            ))
        );
        manager.transport(&session, TransportIntent::Pause).unwrap();
        assert_eq!(
            manager.record("local.main").unwrap().state(),
            SourceState::Paused
        );
    }

    #[test]
    fn arbitration_prefers_higher_priority_then_stable_id() {
        let mut manager = SourceManager::new();
        let low = manager.register(registration("source.low", 10)).unwrap();
        let high_b = manager.register(registration("source.high-b", 20)).unwrap();
        let high_a = manager.register(registration("source.high-a", 20)).unwrap();
        for session in [&low, &high_b, &high_a] {
            manager
                .prepare(
                    session,
                    media("/music/a.flac"),
                    SourceMetadata::default(),
                    caps(),
                )
                .unwrap();
        }
        assert_eq!(
            manager.preferred_prepared().unwrap().session.source_id,
            "source.high-a"
        );
    }

    #[test]
    fn unauthorized_plugin_fails_closed() {
        let mut reg = registration("provider.main", 10);
        reg.authorization.permissions.clear();
        let mut manager = SourceManager::new();
        let session = manager.register(reg).unwrap();
        assert!(matches!(
            manager.prepare(
                &session,
                media("/music/a.flac"),
                SourceMetadata::default(),
                caps()
            ),
            Err(SourceManagerError::PlaybackControlDenied(_))
        ));
    }

    #[test]
    fn source_contract_round_trips_json() {
        let value = PreparedSource {
            session: SourceSessionId {
                source_id: "local.main".to_owned(),
                generation: 7,
            },
            media: PlayableMediaRef::ProviderResolved {
                provider_id: "org.aurora.provider".to_owned(),
                media_id: "track-42".to_owned(),
            },
            metadata: SourceMetadata::default(),
            capabilities: caps(),
        };
        let json = serde_json::to_string(&value).unwrap();
        let decoded: PreparedSource = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, value);
    }
}
