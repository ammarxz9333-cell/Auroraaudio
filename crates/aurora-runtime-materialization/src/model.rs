//! Bounded immutable marker and value contracts.

use crate::{MaterializationDependencyIssue, MaterializationError, MaterializationLimit};

/// Version of the materialization contract schema defined by this crate.
pub const MATERIALIZATION_SCHEMA_VERSION: u16 = 1;

/// Maximum aggregate resource descriptors in one future schema-1 plan.
pub const MAX_MATERIALIZATION_RESOURCES: usize = 32;
/// Maximum materialization stages in one future schema-1 plan.
pub const MAX_MATERIALIZATION_STAGES: usize = 6;
/// Maximum materialization dependencies in one future schema-1 plan.
pub const MAX_MATERIALIZATION_DEPENDENCIES: usize = 64;
/// Maximum capability requirements in one future schema-1 plan.
pub const MAX_CAPABILITY_REQUIREMENTS: usize = 64;
/// Maximum deferred requirements in one future schema-1 plan.
pub const MAX_DEFERRED_REQUIREMENTS: usize = 32;
/// Maximum UTF-8 bytes in one materialization-owned identifier.
pub const MAX_STRING_BYTES: usize = 256;
/// Maximum cumulative UTF-8 bytes retained by one future schema-1 plan.
pub const MAX_TOTAL_STRING_BYTES: usize = 32_768;

/// Aggregate future resource responsibility described without a resource.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RuntimeResourceKind {
    /// Unresolved requested device-selection responsibility.
    DeviceIntent,
    /// Requested, never negotiated audio-format responsibility.
    RequestedAudioFormat,
    /// Prepared canonical topology responsibility.
    Topology,
    /// Prepared canonical routing responsibility.
    Routing,
    /// Future renderer-state responsibility.
    Renderer,
    /// Future DSP-state responsibility.
    Dsp,
    /// Future backend-state responsibility.
    Backend,
    /// Future bounded shared-storage responsibility.
    SharedStorage,
}

/// Canonical passive materialization-planning stage.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MaterializationStage {
    /// Describe unresolved device-intent requirements.
    DeviceIntentRequirements,
    /// Describe requested audio-format requirements.
    RequestedFormatRequirements,
    /// Describe renderer requirements without constructing a renderer.
    RendererRequirements,
    /// Describe DSP requirements without constructing a processor.
    DspRequirements,
    /// Describe backend requirements without selecting or constructing one.
    BackendRequirements,
    /// Mark only that a passive materialization description is complete.
    ///
    /// This variant never means constructed, active, negotiated, or ready.
    MaterializationPlanComplete,
}

/// Semantic category carried by materialization-owned facts.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MaterializationFactSemantics {
    /// User-requested intent retained by accepted upstream contracts.
    Requested,
    /// Deterministically normalized intent from accepted prepared plans.
    Prepared,
    /// Passive future-resource requirements described by this boundary.
    MaterializationPlanned,
    /// A known requirement whose concrete value is unavailable.
    Deferred,
    /// Constructed state is explicitly unavailable as evidence here.
    ConstructedUnavailable,
    /// Active runtime state is explicitly unavailable as evidence here.
    ActiveUnavailable,
}

impl MaterializationFactSemantics {
    fn is_descriptor_evidence(self) -> bool {
        matches!(
            self,
            Self::Requested | Self::Prepared | Self::MaterializationPlanned | Self::Deferred
        )
    }
}

/// Kind of capability a future constructor would need to satisfy.
///
/// A requirement is not an observed, negotiated, healthy, or available
/// capability.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MaterializationCapabilityRequirementKind {
    /// Preserve requested audio-format intent.
    RequestedAudioFormat,
    /// Preserve prepared canonical topology.
    CanonicalTopology,
    /// Support the prepared renderer family.
    RendererFamily,
    /// Preserve the prepared DSP schema state.
    DspSchemaState,
    /// Preserve requested backend-family intent.
    BackendFamily,
    /// Provide bounded preallocated storage in a future constructor.
    PreallocatedStorage,
}

/// Kind of concrete requirement unavailable from accepted prepared plans.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DeferredMaterializationRequirementKind {
    /// Maximum simultaneous object count.
    MaximumObjects,
    /// Renderer scratch storage.
    RendererScratch,
    /// Renderer history storage.
    RendererHistory,
    /// Implementation-specific temporary storage.
    ImplementationTemporary,
    /// Delay-processor storage.
    DelayProcessorStorage,
    /// Asynchronous-resampler storage.
    AsrcStorage,
    /// Backend ring storage.
    BackendRingStorage,
}

/// Immutable aggregate descriptor for one future resource responsibility.
///
/// This value contains no resource handle, behavior, factory, or evidence that
/// construction occurred.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RuntimeResourceDescriptor {
    identifier: String,
    kind: RuntimeResourceKind,
    stage: MaterializationStage,
    semantics: MaterializationFactSemantics,
}

impl RuntimeResourceDescriptor {
    /// Creates a locally valid passive resource descriptor.
    ///
    /// Constructed and active semantic categories are rejected because this
    /// boundary cannot present either as evidence.
    pub fn new(
        identifier: impl Into<String>,
        kind: RuntimeResourceKind,
        stage: MaterializationStage,
        semantics: MaterializationFactSemantics,
    ) -> Result<Self, MaterializationError> {
        let identifier = validated_identifier(identifier.into())?;
        if !semantics.is_descriptor_evidence() {
            return Err(MaterializationError::UnsupportedSemanticCategory { semantics });
        }
        Ok(Self {
            identifier,
            kind,
            stage,
            semantics,
        })
    }

    /// Returns the stable materialization-owned marker identifier.
    pub fn identifier(&self) -> &str {
        &self.identifier
    }

    /// Returns the aggregate future resource responsibility.
    pub fn kind(&self) -> RuntimeResourceKind {
        self.kind
    }

    /// Returns the passive planning stage.
    pub fn stage(&self) -> MaterializationStage {
        self.stage
    }

    /// Returns the honest semantic category represented by the descriptor.
    pub fn semantics(&self) -> MaterializationFactSemantics {
        self.semantics
    }
}

/// Directed resource-index dependency with no executable edge.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MaterializationDependency {
    resource_index: usize,
    depends_on_index: usize,
}

impl MaterializationDependency {
    /// Creates a locally valid directed dependency between bounded indices.
    pub fn new(
        resource_index: usize,
        depends_on_index: usize,
    ) -> Result<Self, MaterializationError> {
        if resource_index >= MAX_MATERIALIZATION_RESOURCES {
            return Err(MaterializationError::InvalidDependencyRelationship {
                issue: MaterializationDependencyIssue::ResourceIndexOutOfRange,
            });
        }
        if depends_on_index >= MAX_MATERIALIZATION_RESOURCES {
            return Err(MaterializationError::InvalidDependencyRelationship {
                issue: MaterializationDependencyIssue::DependencyIndexOutOfRange,
            });
        }
        if resource_index == depends_on_index {
            return Err(MaterializationError::InvalidDependencyRelationship {
                issue: MaterializationDependencyIssue::SelfDependency,
            });
        }
        Ok(Self {
            resource_index,
            depends_on_index,
        })
    }

    /// Returns the dependent resource index.
    pub fn resource_index(self) -> usize {
        self.resource_index
    }

    /// Returns the prerequisite resource index.
    pub fn depends_on_index(self) -> usize {
        self.depends_on_index
    }
}

/// Immutable marker for one future capability requirement.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MaterializationCapabilityRequirement {
    identifier: String,
    kind: MaterializationCapabilityRequirementKind,
}

impl MaterializationCapabilityRequirement {
    /// Creates a locally valid capability-requirement marker.
    pub fn new(
        identifier: impl Into<String>,
        kind: MaterializationCapabilityRequirementKind,
    ) -> Result<Self, MaterializationError> {
        Ok(Self {
            identifier: validated_identifier(identifier.into())?,
            kind,
        })
    }

    /// Returns the stable materialization-owned marker identifier.
    pub fn identifier(&self) -> &str {
        &self.identifier
    }

    /// Returns the future capability requirement kind.
    pub fn kind(&self) -> MaterializationCapabilityRequirementKind {
        self.kind
    }

    /// Returns the fixed honest semantics for this requirement.
    pub fn semantics(&self) -> MaterializationFactSemantics {
        MaterializationFactSemantics::MaterializationPlanned
    }
}

/// Immutable marker for one explicitly deferred concrete requirement.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DeferredMaterializationRequirement {
    identifier: String,
    kind: DeferredMaterializationRequirementKind,
}

impl DeferredMaterializationRequirement {
    /// Creates a locally valid deferred-requirement marker.
    pub fn new(
        identifier: impl Into<String>,
        kind: DeferredMaterializationRequirementKind,
    ) -> Result<Self, MaterializationError> {
        Ok(Self {
            identifier: validated_identifier(identifier.into())?,
            kind,
        })
    }

    /// Returns the stable materialization-owned marker identifier.
    pub fn identifier(&self) -> &str {
        &self.identifier
    }

    /// Returns the unavailable concrete requirement kind.
    pub fn kind(&self) -> DeferredMaterializationRequirementKind {
        self.kind
    }

    /// Returns the fixed honest semantics for this requirement.
    pub fn semantics(&self) -> MaterializationFactSemantics {
        MaterializationFactSemantics::Deferred
    }
}

fn validated_identifier(identifier: String) -> Result<String, MaterializationError> {
    let actual = identifier.len();
    if actual == 0 || actual > MAX_STRING_BYTES {
        return Err(MaterializationError::InvalidIdentifierLength {
            actual,
            minimum: 1,
            maximum: MAX_STRING_BYTES,
        });
    }
    Ok(identifier)
}

#[allow(dead_code)]
fn checked_total_string_bytes(
    current: usize,
    additional: usize,
) -> Result<usize, MaterializationError> {
    let total = current
        .checked_add(additional)
        .ok_or(MaterializationError::CumulativeStringAccountingOverflow)?;
    if total > MAX_TOTAL_STRING_BYTES {
        return Err(MaterializationError::LimitExceeded {
            limit: MaterializationLimit::TotalStringBytes,
            actual: total,
            maximum: MAX_TOTAL_STRING_BYTES,
        });
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cumulative_string_accounting_is_checked_and_bounded() {
        assert_eq!(checked_total_string_bytes(10, 20), Ok(30));
        assert_eq!(
            checked_total_string_bytes(MAX_TOTAL_STRING_BYTES, 1),
            Err(MaterializationError::LimitExceeded {
                limit: MaterializationLimit::TotalStringBytes,
                actual: MAX_TOTAL_STRING_BYTES + 1,
                maximum: MAX_TOTAL_STRING_BYTES,
            })
        );
        assert_eq!(
            checked_total_string_bytes(usize::MAX, 1),
            Err(MaterializationError::CumulativeStringAccountingOverflow)
        );
    }
}
