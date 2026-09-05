//! Versioned product-capability truth model.
//!
//! Capability entries describe what Aurora may claim from accepted evidence.
//! They are control-plane metadata only: they do not probe a host, construct a
//! runtime, or turn software evidence into a physical-product claim.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Current machine-readable capability-registry schema version.
pub const CAPABILITY_REGISTRY_SCHEMA_VERSION: u16 = 1;

/// Broad subsystem represented by a capability entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityKind {
    /// Spatial renderer or renderer backend.
    Renderer,
    /// Immersive or channel-based decoder/input adapter.
    Decoder,
    /// DSP engine or DSP adapter.
    Dsp,
}

/// Overall implementation maturity of one capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImplementationStatus {
    /// Executable implementation exists but its product behavior is still experimental.
    Experimental,
    /// The declared scope is functional, while production readiness remains separate.
    Functional,
    /// Only an adapter/interface placeholder exists; operational behavior is not accepted.
    AdapterPlaceholder,
    /// Code or research scaffolding may exist, but it is not an active product path.
    InactiveResearch,
    /// The product capability has not been implemented.
    NotImplemented,
}

/// Support level for an offline or real-time execution surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportStatus {
    /// The surface must not be claimed as supported.
    Unsupported,
    /// The surface is executable but remains experimental.
    Experimental,
    /// The declared surface is functional within the registry's evidence boundary.
    Functional,
}

impl SupportStatus {
    /// Returns whether this status represents any executable supported surface.
    pub const fn is_supported(self) -> bool {
        !matches!(self, Self::Unsupported)
    }

    /// Returns whether this status represents a functional supported surface.
    pub const fn is_functional(self) -> bool {
        matches!(self, Self::Functional)
    }
}

/// Strongest accepted evidence attached to one capability entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactVerification {
    /// No accepted execution artifact verifies the capability.
    None,
    /// Software tests exist, but no canonical CI artifact is claimed.
    SoftwareTested,
    /// A canonical CI artifact/evaluation path verifies the declared software scope.
    CiArtifact,
    /// Physical acceptance evidence exists for the declared scope.
    PhysicalAcceptance,
}

/// One capability declaration in the canonical registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityEntry {
    /// Stable machine-readable identifier.
    pub id: String,
    /// Human-readable capability name.
    pub name: String,
    /// Capability subsystem.
    pub kind: CapabilityKind,
    /// Overall implementation maturity.
    pub implementation_status: ImplementationStatus,
    /// Offline execution support.
    pub offline_support: SupportStatus,
    /// Real-time execution support.
    pub realtime_support: SupportStatus,
    /// Layouts with accepted testing/evidence for this capability.
    pub tested_layouts: Vec<String>,
    /// Strongest accepted artifact verification level.
    pub artifact_verification: ArtifactVerification,
    /// Explicit marker for adapter-only placeholder entries.
    pub adapter_placeholder: bool,
    /// Whether Aurora may describe this capability as production-ready.
    pub production_ready: bool,
    /// Known perceptual, behavioral, or scope limitations.
    pub cue_limitations: Vec<String>,
    /// Human-readable evidence references; references do not upgrade status by themselves.
    pub evidence: Vec<String>,
}

/// Versioned collection of Aurora capability declarations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityRegistry {
    /// Registry schema version.
    pub schema_version: u16,
    /// Deterministically ordered capability declarations.
    pub capabilities: Vec<CapabilityEntry>,
}

impl CapabilityRegistry {
    /// Validates schema, identifiers, uniqueness, and cross-field claim invariants.
    pub fn validate(&self) -> Result<(), CapabilityRegistryError> {
        if self.schema_version != CAPABILITY_REGISTRY_SCHEMA_VERSION {
            return Err(CapabilityRegistryError::UnsupportedSchemaVersion {
                actual: self.schema_version,
                expected: CAPABILITY_REGISTRY_SCHEMA_VERSION,
            });
        }

        let mut ids = BTreeSet::new();
        for capability in &self.capabilities {
            validate_identifier(&capability.id)?;
            if capability.name.trim().is_empty() {
                return Err(CapabilityRegistryError::EmptyName {
                    id: capability.id.clone(),
                });
            }
            if !ids.insert(capability.id.clone()) {
                return Err(CapabilityRegistryError::DuplicateId {
                    id: capability.id.clone(),
                });
            }

            let placeholder_status = matches!(
                capability.implementation_status,
                ImplementationStatus::AdapterPlaceholder
            );
            if capability.adapter_placeholder != placeholder_status {
                return Err(invariant(
                    capability,
                    CapabilityInvariant::AdapterPlaceholderMarkerMismatch,
                ));
            }

            match capability.implementation_status {
                ImplementationStatus::NotImplemented => {
                    if capability.offline_support.is_supported()
                        || capability.realtime_support.is_supported()
                    {
                        return Err(invariant(
                            capability,
                            CapabilityInvariant::NotImplementedHasSupport,
                        ));
                    }
                    if !capability.tested_layouts.is_empty()
                        || capability.artifact_verification != ArtifactVerification::None
                    {
                        return Err(invariant(
                            capability,
                            CapabilityInvariant::NotImplementedHasAcceptedEvidence,
                        ));
                    }
                }
                ImplementationStatus::AdapterPlaceholder => {
                    if capability.offline_support.is_functional()
                        || capability.realtime_support.is_functional()
                    {
                        return Err(invariant(
                            capability,
                            CapabilityInvariant::PlaceholderClaimsFunctionalSupport,
                        ));
                    }
                }
                ImplementationStatus::InactiveResearch => {
                    if capability.offline_support.is_functional()
                        || capability.realtime_support.is_functional()
                    {
                        return Err(invariant(
                            capability,
                            CapabilityInvariant::InactiveResearchClaimsFunctionalSupport,
                        ));
                    }
                }
                ImplementationStatus::Functional => {
                    if !capability.offline_support.is_functional()
                        && !capability.realtime_support.is_functional()
                    {
                        return Err(invariant(
                            capability,
                            CapabilityInvariant::FunctionalStatusWithoutFunctionalSurface,
                        ));
                    }
                }
                ImplementationStatus::Experimental => {}
            }

            if !capability.tested_layouts.is_empty()
                && !capability.offline_support.is_supported()
                && !capability.realtime_support.is_supported()
            {
                return Err(invariant(
                    capability,
                    CapabilityInvariant::TestedLayoutsWithoutSupportedSurface,
                ));
            }

            if capability.production_ready {
                if capability.implementation_status != ImplementationStatus::Functional {
                    return Err(invariant(
                        capability,
                        CapabilityInvariant::ProductionReadyWithoutFunctionalStatus,
                    ));
                }
                if capability.artifact_verification != ArtifactVerification::PhysicalAcceptance {
                    return Err(invariant(
                        capability,
                        CapabilityInvariant::ProductionReadyWithoutPhysicalAcceptance,
                    ));
                }
            }
        }

        Ok(())
    }
}

/// Contradictory claim detected while validating a capability entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityInvariant {
    /// `adapter_placeholder` disagrees with the typed implementation status.
    AdapterPlaceholderMarkerMismatch,
    /// A not-implemented entry claims an offline or real-time supported surface.
    NotImplementedHasSupport,
    /// A not-implemented entry claims accepted layouts or artifact verification.
    NotImplementedHasAcceptedEvidence,
    /// An adapter placeholder claims functional execution support.
    PlaceholderClaimsFunctionalSupport,
    /// An inactive research entry claims functional execution support.
    InactiveResearchClaimsFunctionalSupport,
    /// A functional entry has no functional execution surface.
    FunctionalStatusWithoutFunctionalSurface,
    /// Tested layouts are listed while both execution surfaces are unsupported.
    TestedLayoutsWithoutSupportedSurface,
    /// Production readiness is claimed without functional implementation status.
    ProductionReadyWithoutFunctionalStatus,
    /// Production readiness is claimed without physical acceptance evidence.
    ProductionReadyWithoutPhysicalAcceptance,
}

/// Errors produced by capability-registry validation.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CapabilityRegistryError {
    /// Registry schema is unknown to this build.
    #[error("unsupported capability registry schema {actual}; expected {expected}")]
    UnsupportedSchemaVersion {
        /// Received schema version.
        actual: u16,
        /// Supported schema version.
        expected: u16,
    },
    /// Capability ID is empty or contains a non-canonical character.
    #[error("invalid capability id `{id}`; use lowercase ASCII letters, digits, and hyphens")]
    InvalidId {
        /// Invalid ID.
        id: String,
    },
    /// Human-readable capability name is empty.
    #[error("capability `{id}` has an empty name")]
    EmptyName {
        /// Capability ID.
        id: String,
    },
    /// Stable capability IDs must be unique.
    #[error("duplicate capability id `{id}`")]
    DuplicateId {
        /// Duplicate ID.
        id: String,
    },
    /// Cross-field fields would permit a claim stronger than the typed status allows.
    #[error("capability `{id}` violates {invariant:?}")]
    ContradictoryClaim {
        /// Capability ID.
        id: String,
        /// Violated invariant.
        invariant: CapabilityInvariant,
    },
}

fn invariant(
    capability: &CapabilityEntry,
    invariant: CapabilityInvariant,
) -> CapabilityRegistryError {
    CapabilityRegistryError::ContradictoryClaim {
        id: capability.id.clone(),
        invariant,
    }
}

fn validate_identifier(id: &str) -> Result<(), CapabilityRegistryError> {
    let valid = !id.is_empty()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !id.starts_with('-')
        && !id.ends_with('-');
    if valid {
        Ok(())
    } else {
        Err(CapabilityRegistryError::InvalidId { id: id.to_owned() })
    }
}

/// Returns Aurora's canonical capability truth registry for this source revision.
pub fn canonical_capability_registry() -> CapabilityRegistry {
    let registry = CapabilityRegistry {
        schema_version: CAPABILITY_REGISTRY_SCHEMA_VERSION,
        capabilities: vec![
            CapabilityEntry {
                id: "geometric-binaural".to_owned(),
                name: "Geometric binaural renderer".to_owned(),
                kind: CapabilityKind::Renderer,
                implementation_status: ImplementationStatus::Experimental,
                offline_support: SupportStatus::Functional,
                realtime_support: SupportStatus::Experimental,
                tested_layouts: vec!["stereo".to_owned()],
                artifact_verification: ArtifactVerification::CiArtifact,
                adapter_placeholder: false,
                production_ready: false,
                cue_limitations: vec![
                    "No validated elevation cues".to_owned(),
                    "No validated front/back discrimination".to_owned(),
                    "No HRIR, pinna, or individualized HRTF data".to_owned(),
                ],
                evidence: vec![
                    "PR #57 Checkpoint A".to_owned(),
                    "Issue #44 renderer-evaluation artifacts".to_owned(),
                ],
            },
            CapabilityEntry {
                id: "iamf".to_owned(),
                name: "IAMF decoder adapter".to_owned(),
                kind: CapabilityKind::Decoder,
                implementation_status: ImplementationStatus::AdapterPlaceholder,
                offline_support: SupportStatus::Unsupported,
                realtime_support: SupportStatus::Unsupported,
                tested_layouts: vec![],
                artifact_verification: ArtifactVerification::None,
                adapter_placeholder: true,
                production_ready: false,
                cue_limitations: vec![
                    "No accepted legal sample has been decoded through the Aurora path".to_owned(),
                ],
                evidence: vec!["aurora-decoder-iamf adapter crate only".to_owned()],
            },
            CapabilityEntry {
                id: "camilladsp".to_owned(),
                name: "CamillaDSP external adapter".to_owned(),
                kind: CapabilityKind::Dsp,
                implementation_status: ImplementationStatus::Functional,
                offline_support: SupportStatus::Functional,
                realtime_support: SupportStatus::Unsupported,
                tested_layouts: vec!["5.1".to_owned()],
                artifact_verification: ArtifactVerification::SoftwareTested,
                adapter_placeholder: false,
                production_ready: false,
                cue_limitations: vec![
                    "Offline external-process scope only".to_owned(),
                    "Redistribution and deployment obligations require separate review".to_owned(),
                ],
                evidence: vec!["CamillaDSP 4.1.3 offline adapter validation".to_owned()],
            },
            CapabilityEntry {
                id: "cavern".to_owned(),
                name: "Cavern renderer adapter".to_owned(),
                kind: CapabilityKind::Renderer,
                implementation_status: ImplementationStatus::InactiveResearch,
                offline_support: SupportStatus::Unsupported,
                realtime_support: SupportStatus::Unsupported,
                tested_layouts: vec![],
                artifact_verification: ArtifactVerification::None,
                adapter_placeholder: false,
                production_ready: false,
                cue_limitations: vec![
                    "Inactive pending license and redistribution review".to_owned()
                ],
                evidence: vec!["Disabled-by-default research adapter".to_owned()],
            },
            CapabilityEntry {
                id: "truehdd".to_owned(),
                name: "truehdd decoder adapter".to_owned(),
                kind: CapabilityKind::Decoder,
                implementation_status: ImplementationStatus::InactiveResearch,
                offline_support: SupportStatus::Unsupported,
                realtime_support: SupportStatus::Unsupported,
                tested_layouts: vec![],
                artifact_verification: ArtifactVerification::None,
                adapter_placeholder: false,
                production_ready: false,
                cue_limitations: vec![
                    "Inactive research path; product use requires legal review".to_owned()
                ],
                evidence: vec!["Disabled-by-default research adapter".to_owned()],
            },
            CapabilityEntry {
                id: "loudspeaker-3d".to_owned(),
                name: "3D loudspeaker renderer".to_owned(),
                kind: CapabilityKind::Renderer,
                implementation_status: ImplementationStatus::NotImplemented,
                offline_support: SupportStatus::Unsupported,
                realtime_support: SupportStatus::Unsupported,
                tested_layouts: vec![],
                artifact_verification: ArtifactVerification::None,
                adapter_placeholder: false,
                production_ready: false,
                cue_limitations: vec![
                    "Must not be claimed before issue #38 acceptance evidence".to_owned()
                ],
                evidence: vec![],
            },
        ],
    };
    debug_assert!(registry.validate().is_ok());
    registry
}

#[cfg(test)]
mod tests {
    use serde::{de::value::StrDeserializer, Deserialize};

    use super::*;

    #[test]
    fn canonical_registry_is_valid_and_deterministically_ordered() {
        let registry = canonical_capability_registry();
        registry.validate().unwrap();
        let ids = registry
            .capabilities
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            [
                "geometric-binaural",
                "iamf",
                "camilladsp",
                "cavern",
                "truehdd",
                "loudspeaker-3d"
            ]
        );
    }

    #[test]
    fn unknown_implementation_status_is_rejected_by_typed_deserialization() {
        let result = ImplementationStatus::deserialize(
            StrDeserializer::<serde::de::value::Error>::new("invented-status"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn placeholder_cannot_claim_functional_support() {
        let mut registry = canonical_capability_registry();
        let entry = registry
            .capabilities
            .iter_mut()
            .find(|entry| entry.id == "iamf")
            .unwrap();
        entry.offline_support = SupportStatus::Functional;
        assert_eq!(
            registry.validate(),
            Err(CapabilityRegistryError::ContradictoryClaim {
                id: "iamf".to_owned(),
                invariant: CapabilityInvariant::PlaceholderClaimsFunctionalSupport,
            })
        );
    }

    #[test]
    fn not_implemented_capability_cannot_claim_layouts() {
        let mut registry = canonical_capability_registry();
        let entry = registry
            .capabilities
            .iter_mut()
            .find(|entry| entry.id == "loudspeaker-3d")
            .unwrap();
        entry.tested_layouts.push("7.1.4".to_owned());
        assert_eq!(
            registry.validate(),
            Err(CapabilityRegistryError::ContradictoryClaim {
                id: "loudspeaker-3d".to_owned(),
                invariant: CapabilityInvariant::NotImplementedHasAcceptedEvidence,
            })
        );
    }

    #[test]
    fn production_ready_requires_functional_status_and_physical_acceptance() {
        let mut registry = canonical_capability_registry();
        let entry = registry
            .capabilities
            .iter_mut()
            .find(|entry| entry.id == "camilladsp")
            .unwrap();
        entry.production_ready = true;
        assert_eq!(
            registry.validate(),
            Err(CapabilityRegistryError::ContradictoryClaim {
                id: "camilladsp".to_owned(),
                invariant: CapabilityInvariant::ProductionReadyWithoutPhysicalAcceptance,
            })
        );
    }
}
