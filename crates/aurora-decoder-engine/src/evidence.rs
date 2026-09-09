use crate::catalog::BackendId;

/// Evidence maturity for a decoder backend.
///
/// This is deliberately separate from source-code availability or licensing.
/// A backend does not become production-ready merely because it exists,
/// compiles upstream, or has a strong reputation; Aurora promotes it only when
/// the exact pinned integration passes retained gates.
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

const fn integrated(backend: BackendId, pinned_revision: bool) -> BackendEvidence {
    BackendEvidence {
        backend,
        tier: EvidenceTier::Integrated,
        pinned_revision,
        independent_reference: false,
        fuzz_gate: false,
        realtime_gate: false,
    }
}

const fn catalogued(backend: BackendId) -> BackendEvidence {
    BackendEvidence {
        backend,
        tier: EvidenceTier::Catalogued,
        pinned_revision: false,
        independent_reference: false,
        fuzz_gate: false,
        realtime_gate: false,
    }
}

/// Evidence state for the exact Aurora branch, not a claim about the upstream
/// project in isolation. Tiers only move upward when artifacts are retained.
pub const fn evidence_for(backend: BackendId) -> BackendEvidence {
    match backend {
        BackendId::OpenJoc => integrated(backend, true),
        BackendId::OxideAc3 => integrated(backend, true),
        BackendId::OxideAc4 => integrated(backend, true),
        BackendId::OxideDtsCore => integrated(backend, true),
        BackendId::FfmpegDtsHd => integrated(backend, true),
        BackendId::FfmpegWorker => integrated(backend, true),
        BackendId::TrueHdNative
        | BackendId::OxideAac
        | BackendId::OxideOpus
        | BackendId::LibIamfReference
        | BackendId::IamfTools
        | BackendId::LibMpegH
        | BackendId::Symphonia => catalogued(backend),
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
            BackendId::FfmpegDtsHd,
            BackendId::FfmpegWorker,
        ] {
            let evidence = evidence_for(backend);
            assert!(evidence.satisfies(EvidenceTier::Integrated));
            assert!(!evidence.satisfies(EvidenceTier::BuildVerified));
            assert!(!evidence.satisfies(EvidenceTier::ProductReady));
        }
    }

    #[test]
    fn best_of_breed_candidates_do_not_fake_integration() {
        for backend in [
            BackendId::TrueHdNative,
            BackendId::LibIamfReference,
            BackendId::IamfTools,
            BackendId::LibMpegH,
        ] {
            assert!(!evidence_for(backend).satisfies(EvidenceTier::Integrated));
        }
    }
}
