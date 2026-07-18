//! Structured failures for local Checkpoint A contract validation.

use core::fmt;

use crate::MaterializationFactSemantics;

/// A finite materialization contract limit.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MaterializationLimit {
    /// Maximum aggregate resource descriptors.
    Resources,
    /// Maximum materialization stages.
    Stages,
    /// Maximum dependency records.
    Dependencies,
    /// Maximum capability requirements.
    CapabilityRequirements,
    /// Maximum deferred requirements.
    DeferredRequirements,
    /// Maximum UTF-8 bytes in one identifier.
    StringBytes,
    /// Maximum cumulative UTF-8 bytes retained by a future plan.
    TotalStringBytes,
}

/// A locally invalid dependency relationship.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MaterializationDependencyIssue {
    /// A resource was declared as depending on itself.
    SelfDependency,
    /// The dependent resource index is outside the published resource bound.
    ResourceIndexOutOfRange,
    /// The prerequisite resource index is outside the published resource bound.
    DependencyIndexOutOfRange,
}

/// Deterministic failures from Checkpoint A marker/value construction.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MaterializationError {
    /// A published finite limit was exceeded.
    LimitExceeded {
        /// Limit that was exceeded.
        limit: MaterializationLimit,
        /// Supplied value or count.
        actual: usize,
        /// Published inclusive maximum.
        maximum: usize,
    },
    /// A marker identifier is empty or exceeds the local UTF-8 byte limit.
    InvalidIdentifierLength {
        /// Actual identifier size in UTF-8 bytes.
        actual: usize,
        /// Inclusive minimum accepted size.
        minimum: usize,
        /// Inclusive maximum accepted size.
        maximum: usize,
    },
    /// Future cumulative string accounting could not be represented.
    CumulativeStringAccountingOverflow,
    /// A dependency violates a local Checkpoint A relationship invariant.
    InvalidDependencyRelationship {
        /// Deterministic relationship issue.
        issue: MaterializationDependencyIssue,
    },
    /// A descriptor attempted to present unavailable runtime state as evidence.
    UnsupportedSemanticCategory {
        /// Semantic category rejected for a concrete descriptor.
        semantics: MaterializationFactSemantics,
    },
}

impl fmt::Display for MaterializationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LimitExceeded {
                limit,
                actual,
                maximum,
            } => write!(
                formatter,
                "materialization limit {limit:?} exceeded: {actual} > {maximum}"
            ),
            Self::InvalidIdentifierLength {
                actual,
                minimum,
                maximum,
            } => write!(
                formatter,
                "materialization identifier length {actual} is outside {minimum}..={maximum} bytes"
            ),
            Self::CumulativeStringAccountingOverflow => {
                formatter.write_str("materialization string accounting overflowed")
            }
            Self::InvalidDependencyRelationship { issue } => {
                write!(formatter, "invalid materialization dependency: {issue:?}")
            }
            Self::UnsupportedSemanticCategory { semantics } => write!(
                formatter,
                "materialization descriptor cannot represent {semantics:?} as evidence"
            ),
        }
    }
}

impl std::error::Error for MaterializationError {}
