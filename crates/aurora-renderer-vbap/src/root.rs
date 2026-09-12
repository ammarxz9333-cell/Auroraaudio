//! Aurora vector-base amplitude panning renderers.
//!
//! The existing two-dimensional implementation remains source-compatible at
//! the crate root. The generic height-capable convex-hull implementation stays
//! in [`three_d`], while [`immersive_layered`] adds a cinema-layout policy that
//! keeps sources at/above the highest loudspeaker ring inside that height layer.

#[path = "lib.rs"]
mod two_d;

pub use two_d::*;

#[cfg(test)]
pub(crate) use two_d::allocation_audit;

/// Generic three-dimensional loudspeaker VBAP.
pub mod three_d;

/// Layer-aware immersive renderer for cinema-style loudspeaker layouts.
pub mod immersive_layered;

pub use immersive_layered::ImmersiveLayeredRenderer;
pub use three_d::{ValidatedTriplet, Vbap3dRenderer};
