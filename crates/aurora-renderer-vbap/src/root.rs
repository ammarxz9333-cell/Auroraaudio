include!("lib.rs");

/// Experimental three-dimensional loudspeaker VBAP.
pub mod three_d;

pub use three_d::{ValidatedTriplet, Vbap3dRenderer};
