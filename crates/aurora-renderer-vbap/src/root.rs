//! Aurora loudspeaker renderers.
//!
//! The historical `VbapRenderer` remains the accepted horizontal-plane 2D
//! renderer. `Vbap3dRenderer` is a separate height-capable implementation so
//! 2D evidence can never be confused with 3D/height evidence.

#[path = "lib.rs"]
mod horizontal;
pub use horizontal::*;

#[cfg(test)]
pub(crate) use horizontal::allocation_audit;

// Indexed triplet enumeration is intentional: stable speaker indices are part
// of deterministic triplet tie-breaking and are written directly to gains.
#[allow(clippy::needless_range_loop)]
pub mod three_d;
pub use three_d::Vbap3dRenderer;
