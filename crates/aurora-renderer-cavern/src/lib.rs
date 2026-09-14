//! Cavern integration status boundary.
//!
//! Aurora does not vendor, link, or execute Cavern in the accepted runtime. Cavern remains an
//! inactive research candidate pending license/redistribution review. Keeping that fact as
//! capability metadata is safer than exposing a `Renderer` implementation that can configure
//! successfully and then fail every render call.

/// Whether a Cavern runtime renderer is available in this source revision.
pub const CAVERN_RUNTIME_AVAILABLE: bool = false;

/// Stable reason the Cavern runtime is not exposed.
pub const fn cavern_runtime_unavailable_reason() -> &'static str {
    "Cavern runtime integration is inactive pending license and redistribution review"
}

/// Non-executable status returned to control-plane discovery code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CavernIntegrationStatus {
    /// Whether Aurora may instantiate Cavern as a renderer.
    pub runtime_available: bool,
    /// Human-readable status reason.
    pub reason: &'static str,
}

/// Returns the current Cavern integration status without constructing a fake renderer.
pub const fn integration_status() -> CavernIntegrationStatus {
    CavernIntegrationStatus {
        runtime_available: CAVERN_RUNTIME_AVAILABLE,
        reason: cavern_runtime_unavailable_reason(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cavern_is_explicitly_non_executable() {
        let status = integration_status();
        assert!(!status.runtime_available);
        assert!(status.reason.contains("license"));
    }
}
