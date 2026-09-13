//! Aurora vector-base amplitude panning renderers.
//!
//! The existing two-dimensional implementation remains source-compatible at
//! the crate root. Object-semantic rendering that must preserve LFE as a
//! non-directional output slot is exposed through [`ObjectVbapRenderer`]. The
//! experimental height-capable implementation remains isolated in [`three_d`]
//! until issue #38 acceptance is complete.

#[path = "lib.rs"]
mod two_d;

pub use two_d::*;

#[cfg(test)]
pub(crate) use two_d::allocation_audit;

/// Object-semantic horizontal VBAP that excludes LFE from panning.
pub mod object;

pub use object::ObjectVbapRenderer;

/// Experimental three-dimensional loudspeaker VBAP.
pub mod three_d;

pub use three_d::{ValidatedTriplet, Vbap3dRenderer};
