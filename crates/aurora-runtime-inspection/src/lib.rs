//! Read-only control-plane inspection contracts for prepared runtime plans.
//!
//! # Ownership
//!
//! This crate owns its inspection model, options, errors, and formatter marker
//! types. It does not add serialization or other traits to prepared-plan types
//! owned by `aurora-runtime-assembly`.
//!
//! # Dependency boundary
//!
//! `aurora-runtime-assembly` is this crate's only Aurora crate dependency.
//! Runtime assembly does not depend back on inspection, and no renderer, DSP,
//! engine, backend, simulator, diagnostics, CLI, host, or hardware dependency
//! belongs here.
//!
//! # Checkpoint C stop boundary
//!
//! Checkpoint C formats bounded, immutable, redacted-by-default projections as
//! deterministic JSON or text. It performs no deserialization, persistence,
//! runtime construction, or runtime execution.
//!
//! # Forbidden behavior
//!
//! This crate must not serialize, deserialize, mutate, reconstruct, hash, or
//! fingerprint prepared plans. It must not access the filesystem, environment,
//! network, devices, callbacks, or diagnostics producers, and it must not make
//! negotiated, observed, runtime-readiness, latency, or physical claims.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod error;
pub mod formatter;
pub mod model;
pub mod options;

pub use error::{
    InspectionError, InspectionFloatField, InspectionLimit, InspectionOutputFormat,
    PlanRelationship,
};
pub use formatter::{
    JsonFormatter, TextFormatter, MAX_JSON_BYTES, MAX_NESTING_DEPTH,
    MAX_SERIALIZED_COLLECTION_ENTRIES, MAX_TEXT_BYTES,
};
pub use model::*;
pub use options::InspectionOptions;

#[cfg(test)]
mod tests {
    use super::{JsonFormatter, TextFormatter};

    fn assert_formatter_value<T: Clone + Copy + core::fmt::Debug + Default + Eq>() {}

    #[test]
    fn formatter_types_remain_stateless_values() {
        assert_formatter_value::<JsonFormatter>();
        assert_formatter_value::<TextFormatter>();
    }
}
