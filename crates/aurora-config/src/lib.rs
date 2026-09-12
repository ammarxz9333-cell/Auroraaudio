#![forbid(unsafe_code)]
//! Versioned, deterministic, bounded Aurora configuration and presets.
//!
//! This crate is a control-plane library. It describes configuration intent;
//! it does not probe devices, negotiate formats, mutate audio callbacks, or
//! prove physical behavior. Use [`ValidatedConfiguration`] before consuming a
//! configuration.

mod error;
mod limits;
mod migration;
mod model;
mod preset;
mod redaction;
mod validation;

pub use error::{ConfigError, ErrorCategory, ErrorCode};
pub use limits::*;
pub use migration::{
    migrate_v0_to_v3, migrate_v1_to_v3, migrate_v2_to_v3, MigrationResult, MigrationWarning,
};
pub use model::*;
pub use preset::{materialize_preset, Preset, PresetCollection, PresetPayload, PresetType};
pub use redaction::{RedactedConfiguration, RedactedDeviceIntent};
pub use validation::ValidatedConfiguration;
