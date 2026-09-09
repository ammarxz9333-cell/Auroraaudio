//! Aurora MPEG-H 3D Audio backend.
//!
//! The default workspace build carries no native C linkage. Enabling
//! `native-mpegh` activates a narrow unsafe FFI boundary around the pinned
//! Ittiam libmpegh external-render interface. That interface exposes pre-render
//! PCM plus channel/OAM/HOA metadata rather than forcing an early speaker mix.

#![cfg_attr(not(feature = "native-mpegh"), forbid(unsafe_code))]

#[cfg(feature = "native-mpegh")]
mod ffi;
#[cfg(feature = "native-mpegh")]
mod native;

#[cfg(feature = "native-mpegh")]
pub use native::{
    MpeghExternalFrame, MpeghNativeError, MpeghSpeaker, MpeghSpeakerLayout,
    NativeMpeghDecoder,
};

/// Whether this build contains the native libmpegh external-render backend.
pub const fn native_backend_enabled() -> bool {
    cfg!(feature = "native-mpegh")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_build_reports_native_boundary_truthfully() {
        assert_eq!(native_backend_enabled(), cfg!(feature = "native-mpegh"));
    }
}
