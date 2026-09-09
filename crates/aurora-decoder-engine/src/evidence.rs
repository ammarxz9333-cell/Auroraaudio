use crate::catalog::BackendId;

/// Evidence maturity for a decoder backend.
///
/// This is deliberately separate from source-code availability or licensing.
/// A permissively licensed backend can still be immature, and a backend does
/// not become production-ready merely because it compiles or is integrated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum EvidenceTier {
    /// Known candidate only; no Aurora integration is claimed.
    Catalogued = 0,
    /// Wired behind the Aurora boundary, but current-branch compile/conformance
    /// evidence has not yet passed all gates.
    Integrated = 1,
    /// Reproducible build/test evidence exists for the pinned revision.
    BuildVerified = 2,
    /// Deterministic corpus comparison against an independent reference passed.
    CorpusVerified = 3,
    /// Corruption/fuzz/panic-safety acceptance gates passed.
    FuzzVerified = 4,
    /// Realtime latency, allocation and continuity gates passed on target class.
    RealtimeVerified = 5,
    /// All required technical and product release gates are satisfied.
    ProductReady = 6,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendEvidence {
    pub backend: BackendId,
    pub tier: EvidenceTier,
    pub pinned_revision: bool,
    pub independent_reference: bool,
    pub fuzz_gate: bool,
    pub realtime_gate: bool,
}

impl BackendEvidence {
    pub const fn satisfies(self, minimum: EvidenceTier) -> bool {
        (self.tier as u8) >= (minimum as u8)
    }
}

/// Evidence state for the exact Aurora branch, not a claim about the upstream
/// project in isolation. Tiers only move upward when artifacts are retained.
pub const fn evidence_for(backend: BackendId) -> BackendEvidence {
    match backend {
        BackendId::OpenJoc => BackendEvidence {
            backend,
            tier: EvidenceTier::Integrated,
            pinned_revision: true,
            independent_reference: false,
            fuzz_gate: false,
            realtime_gate: false,
        },
        BackendId::OxideAc3 => BackendEvidence {
            backend,
            tier: EvidenceTier::Integrated,
            pinned_revision: true,
            independent_reference: false,
            fuzz_gate: false,
            realtime_gate: false,
        },
        BackendId::OxideAc4 => BackendEvidence {
            backend,
            tier: EvidenceTier::Integrated,
            pinned_revision: true,
            independent_reference: false,
            fuzz_gate: false,
            realtime_gate: false,
        },
        BackendId::OxideDtsCore => BackendEvidence {
            backend,
            tier: EvidenceTier::Integrated,
            pinned_revision: true,
            // Upstream has independent-reference evidence, but Aurora's own
            // adapter corpus still has to pass before this tier is promoted.
            independent_reference: false,
            fuzz_gate: false,
            realtime_gate: false,
        },
        BackendId::FfmpegWorker => BackendEvidence {
            backend,
            tier: EvidenceTier::Integrated,
            pinned_revision: true,
            independent_reference: false,
            fuzz_gate: false,
            realtime_gate: false,
        },
        BackendId::TrueHdNative
        | BackendId::OxideAac
        | BackendId::OxideOpus
        | BackendId::IamfTools
        | BackendId::Symphonia => BackendEvidence {
            backend,
            tier: EvidenceTier::Catalogued,
            pinned_revision: false,
            independent_reference: false,
            fuzz_gate: false,
            realtime_gate: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integrated_is_not_silently_product_ready() {
        for backend in [
            BackendId::OpenJoc,
            BackendId::OxideAc3,
            BackendId::OxideAc4,
            BackendId::OxideDtsCore,
            BackendId::FfmpegWorker,
        ] {
            let evidence = evidence_for(backend);
            assert!(evidence.satisfies(EvidenceTier::Integrated));
            assert!(!evidence.satisfies(EvidenceTier::BuildVerified));
            assert!(!evidence.satisfies(EvidenceTier::ProductReady));
        }
    }

    #[test]
    fn candidates_do_not_pass_integration_floor() {
        assert!(!evidence_for(BackendId::TrueHdNative).satisfies(EvidenceTier::Integrated));
        assert!(!evidence_for(BackendId::IamfTools).satisfies(EvidenceTier::Integrated));
    }
}
