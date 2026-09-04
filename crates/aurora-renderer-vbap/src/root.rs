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

pub mod three_d;
pub use three_d::Vbap3dRenderer;
