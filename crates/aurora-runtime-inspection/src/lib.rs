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
//! # Checkpoint A stop boundary
//!
//! Checkpoint A defines empty public contracts and module boundaries only. It
//! performs no report projection, redaction, validation, JSON generation, text
//! generation, plan access, runtime construction, or runtime execution.
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

pub use error::InspectionError;
pub use formatter::{JsonFormatter, TextFormatter};
pub use model::InspectionReport;
pub use options::InspectionOptions;

#[cfg(test)]
mod tests {
    use super::{
        InspectionError, InspectionOptions, InspectionReport, JsonFormatter, TextFormatter,
    };

    fn assert_public_contract<T: Clone + Copy + core::fmt::Debug + Default + Eq>() {}
    fn assert_leaf_value<T: Send + Sync + Unpin + 'static>() {}

    #[test]
    fn crate_and_public_api_compile() {
        assert_public_contract::<InspectionReport>();
        assert_public_contract::<InspectionOptions>();
        assert_public_contract::<InspectionError>();
        assert_leaf_value::<InspectionReport>();
        assert_leaf_value::<InspectionOptions>();
        assert_leaf_value::<InspectionError>();
    }

    #[test]
    fn formatter_marker_traits_compile() {
        assert_public_contract::<JsonFormatter>();
        assert_public_contract::<TextFormatter>();
        assert_leaf_value::<JsonFormatter>();
        assert_leaf_value::<TextFormatter>();
    }
}
