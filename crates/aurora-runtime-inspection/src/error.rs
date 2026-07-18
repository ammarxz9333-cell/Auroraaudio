//! Structured inspection-owned error contracts.

use core::fmt;

/// A published inspection projection limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectionLimit {
    /// Maximum retained findings.
    Findings,
    /// Maximum bytes in one copied source string.
    StringBytes,
    /// Maximum bytes across copied source strings.
    TotalStringBytes,
    /// Maximum identities in either channel direction.
    Channels,
    /// Maximum routes.
    Routes,
    /// Maximum speakers.
    Speakers,
    /// Maximum setup stages.
    SetupStages,
    /// Maximum setup dependencies.
    SetupDependencies,
}

/// A relationship that must agree between paired prepared plans.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlanRelationship {
    /// Runtime and setup device intent must agree.
    DeviceIntent,
    /// Runtime and setup requested formats must agree.
    RequestedAudioFormat,
    /// Runtime and setup renderer intent must agree.
    RendererIntent,
    /// Runtime and setup topology intent must agree.
    TopologyIntent,
    /// Runtime and setup DSP intent must agree.
    DspIntent,
}

/// Structured failures produced while projecting prepared plans.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InspectionError {
    /// A published finite projection bound was exceeded.
    LimitExceeded {
        /// Limit that was exceeded.
        limit: InspectionLimit,
        /// Actual source value.
        actual: usize,
        /// Published maximum.
        maximum: usize,
    },
    /// The supplied setup plan was not derived from the supplied runtime facts.
    SourcePlansMismatch {
        /// Relationship that differs.
        relationship: PlanRelationship,
    },
    /// Checked cumulative string accounting overflowed.
    StringAccountingOverflow,
}

impl fmt::Display for InspectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LimitExceeded {
                limit,
                actual,
                maximum,
            } => write!(
                formatter,
                "inspection limit {limit:?} exceeded: {actual} > {maximum}"
            ),
            Self::SourcePlansMismatch { relationship } => {
                write!(formatter, "prepared plans disagree on {relationship:?}")
            }
            Self::StringAccountingOverflow => {
                formatter.write_str("inspection string accounting overflowed")
            }
        }
    }
}

impl std::error::Error for InspectionError {}
