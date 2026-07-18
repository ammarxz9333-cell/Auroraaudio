//! Inspection-owned error contracts.

/// Empty Checkpoint A marker for a future structured inspection error.
///
/// Error categories and validation behavior are reserved for later reviewed
/// checkpoints.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InspectionError;
