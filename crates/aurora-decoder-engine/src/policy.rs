use crate::catalog::{BackendDescriptor, DecoderCatalog, ExecutionClass, LicenseClass};
use crate::catalog::CodecId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FidelityMode {
    Balanced,
    MaximumFidelity,
    LowestLatency,
    MaximumPortability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectRequirement {
    Ignore,
    Prefer,
    Require,
}

#[derive(Debug, Clone, Copy)]
pub struct DecoderPolicy {
    pub fidelity_mode: FidelityMode,
    pub objects: ObjectRequirement,
    pub prefer_native: bool,
    pub require_closed_core_compatible: bool,
    pub allow_external_worker: bool,
    pub allow_file_level_copyleft: bool,
}

impl Default for DecoderPolicy {
    fn default() -> Self {
        Self {
            fidelity_mode: FidelityMode::Balanced,
            objects: ObjectRequirement::Prefer,
            prefer_native: true,
            require_closed_core_compatible: true,
            allow_external_worker: true,
            allow_file_level_copyleft: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BackendDecision {
    pub backend: &'static BackendDescriptor,
    pub score: i32,
}

impl DecoderPolicy {
    pub fn rank(
        &self,
        catalog: &DecoderCatalog,
        codec: CodecId,
        integrated_only: bool,
    ) -> Vec<BackendDecision> {
        let mut ranked = catalog
            .candidates(codec)
            .filter(|backend| !integrated_only || backend.integrated)
            .filter(|backend| self.admits(backend))
            .filter_map(|backend| self.score(backend).map(|score| BackendDecision { backend, score }))
            .collect::<Vec<_>>();

        ranked.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| (a.backend.id as u8).cmp(&(b.backend.id as u8)))
        });
        ranked
    }

    fn admits(&self, backend: &BackendDescriptor) -> bool {
        if self.require_closed_core_compatible && !backend.closed_core_compatible {
            return false;
        }
        if !self.allow_external_worker && backend.execution == ExecutionClass::ExternalWorker {
            return false;
        }
        if !self.allow_file_level_copyleft && backend.license == LicenseClass::FileLevelCopyleft {
            return false;
        }
        if self.objects == ObjectRequirement::Require && !backend.object_metadata {
            return false;
        }
        true
    }

    fn score(&self, backend: &BackendDescriptor) -> Option<i32> {
        if !self.admits(backend) {
            return None;
        }

        let mut score = match self.fidelity_mode {
            FidelityMode::Balanced => {
                i32::from(backend.quality) * 3
                    + i32::from(backend.robustness) * 2
                    + i32::from(backend.latency) * 2
                    + i32::from(backend.metadata_fidelity) * 2
                    + i32::from(backend.portability)
            }
            FidelityMode::MaximumFidelity => {
                i32::from(backend.quality) * 4
                    + i32::from(backend.metadata_fidelity) * 4
                    + i32::from(backend.robustness) * 2
                    + i32::from(backend.latency)
                    + i32::from(backend.portability)
            }
            FidelityMode::LowestLatency => {
                i32::from(backend.latency) * 5
                    + i32::from(backend.quality) * 2
                    + i32::from(backend.robustness) * 2
                    + i32::from(backend.portability)
            }
            FidelityMode::MaximumPortability => {
                i32::from(backend.portability) * 5
                    + i32::from(backend.robustness) * 2
                    + i32::from(backend.quality) * 2
                    + i32::from(backend.latency)
            }
        };

        if self.prefer_native {
            score += match backend.execution {
                ExecutionClass::NativeRust => 80,
                ExecutionClass::NativeLibrary => 35,
                ExecutionClass::ExternalWorker => 0,
            };
        }

        score += match self.objects {
            ObjectRequirement::Ignore => 0,
            ObjectRequirement::Prefer if backend.object_metadata => 120,
            ObjectRequirement::Prefer => 0,
            ObjectRequirement::Require => 180,
        };

        if backend.integrated {
            score += 25;
        }

        Some(score)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::BackendId;

    #[test]
    fn joc_metadata_requirement_excludes_bed_only_backends() {
        let policy = DecoderPolicy {
            objects: ObjectRequirement::Require,
            ..DecoderPolicy::default()
        };
        let catalog = DecoderCatalog::default();
        let ranked = policy.rank(&catalog, CodecId::Eac3Joc, false);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].backend.id, BackendId::OpenJoc);
    }

    #[test]
    fn native_truehd_candidate_outranks_external_fallback() {
        let policy = DecoderPolicy::default();
        let catalog = DecoderCatalog::default();
        let ranked = policy.rank(&catalog, CodecId::TrueHd, false);
        assert_eq!(ranked.first().map(|d| d.backend.id), Some(BackendId::TrueHdNative));
        assert!(ranked.iter().any(|d| d.backend.id == BackendId::FfmpegWorker));
    }

    #[test]
    fn proprietary_default_rejects_mpl_candidate() {
        let policy = DecoderPolicy::default();
        let catalog = DecoderCatalog::default();
        let ranked = policy.rank(&catalog, CodecId::Flac, false);
        assert!(ranked.iter().all(|d| d.backend.id != BackendId::Symphonia));
    }
}
