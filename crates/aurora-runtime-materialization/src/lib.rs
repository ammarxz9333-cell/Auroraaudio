//! Passive contracts for future runtime-resource requirement planning.
//!
//! # Ownership
//!
//! This crate owns bounded marker and immutable value contracts that describe
//! materialization-planning vocabulary. It does not own prepared plans or any
//! constructed runtime resource.
//!
//! # Dependency boundary
//!
//! `aurora-runtime-assembly` is the only Aurora production dependency. This
//! Checkpoint A crate does not inspect or derive from its prepared plans, and
//! no existing crate depends back on materialization.
//!
//! # Checkpoint A stop boundary
//!
//! Checkpoint A publishes schema limits, passive enums, immutable descriptors,
//! local invariant validation, and structured errors only. Plan derivation,
//! canonical collection building, inspection formatting, resource construction,
//! runtime execution, and integration belong to later separately reviewed work.
//!
//! # Forbidden behavior
//!
//! This crate performs no host, device, filesystem, environment, network,
//! clock, randomness, process, thread, stream, callback, renderer, DSP,
//! backend, engine, simulator, CLI, diagnostics, or physical work. Its values
//! do not prove construction, activity, negotiation, readiness, health,
//! latency, hardware behavior, or physical evidence.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod error;
pub mod model;

pub use error::{MaterializationDependencyIssue, MaterializationError, MaterializationLimit};
pub use model::{
    DeferredMaterializationRequirement, DeferredMaterializationRequirementKind,
    MaterializationCapabilityRequirement, MaterializationCapabilityRequirementKind,
    MaterializationDependency, MaterializationFactSemantics, MaterializationStage,
    RuntimeResourceDescriptor, RuntimeResourceKind, MATERIALIZATION_SCHEMA_VERSION,
    MAX_CAPABILITY_REQUIREMENTS, MAX_DEFERRED_REQUIREMENTS, MAX_MATERIALIZATION_DEPENDENCIES,
    MAX_MATERIALIZATION_RESOURCES, MAX_MATERIALIZATION_STAGES, MAX_STRING_BYTES,
    MAX_TOTAL_STRING_BYTES,
};
