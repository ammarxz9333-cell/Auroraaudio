//! Inspection-owned report contracts.

/// Empty Checkpoint A marker for a future immutable inspection report.
///
/// Report fields and projection behavior are reserved for later reviewed
/// checkpoints.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InspectionReport;
