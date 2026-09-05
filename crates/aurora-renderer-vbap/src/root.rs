//! Aurora vector-base amplitude panning renderers.
//!
//! The existing two-dimensional implementation remains source-compatible at
//! the crate root. The experimental height-capable implementation is isolated
//! in [`three_d`] until issue #38 acceptance is complete.

#[path = "lib.rs"]
mod two_d;

pub use two_d::*;

/// Experimental three-dimensional loudspeaker VBAP.
pub mod three_d;

pub use three_d::{ValidatedTriplet, Vbap3dRenderer};
