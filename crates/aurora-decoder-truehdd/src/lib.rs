//! TrueHD integration status boundary.
//!
//! Aurora does not vendor or execute `truehdd` in the accepted runtime. The upstream project can
//! decode channel presentations and export DAMF assets, but Aurora does not yet have a reviewed
//! mapping from those DAMF audio/metadata files into complete object-to-PCM bindings. Exposing a
//! decoder that silently drops that distinction would be misleading, so this crate is intentionally
//! non-executable until that contract is implemented and validated.

/// Whether a TrueHD/Atmos runtime decoder is available in this source revision.
pub const TRUEHDD_RUNTIME_AVAILABLE: bool = false;

/// Whether native TrueHD/Atmos object-scene decoding is available.
pub const TRUEHDD_OBJECT_SCENE_AVAILABLE: bool = false;

/// Stable reason the TrueHD runtime is not exposed.
pub const fn truehdd_runtime_unavailable_reason() -> &'static str {
    "TrueHD/DAMF integration is deferred until Aurora has reviewed object-to-PCM bindings"
}

/// Non-executable status returned to control-plane discovery code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TruehddIntegrationStatus {
    /// Whether Aurora may instantiate a TrueHD decoder.
    pub runtime_available: bool,
    /// Whether Aurora may claim native object-scene decoding.
    pub object_scene_available: bool,
    /// Human-readable status reason.
    pub reason: &'static str,
}

/// Returns the current TrueHD integration status without constructing a fake decoder.
pub const fn integration_status() -> TruehddIntegrationStatus {
    TruehddIntegrationStatus {
        runtime_available: TRUEHDD_RUNTIME_AVAILABLE,
        object_scene_available: TRUEHDD_OBJECT_SCENE_AVAILABLE,
        reason: truehdd_runtime_unavailable_reason(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truehdd_is_explicitly_non_executable() {
        let status = integration_status();
        assert!(!status.runtime_available);
        assert!(!status.object_scene_available);
        assert!(status.reason.contains("object-to-PCM"));
    }
}
