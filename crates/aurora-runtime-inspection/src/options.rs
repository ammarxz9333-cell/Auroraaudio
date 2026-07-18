//! Inspection-owned option contracts.

/// Empty Checkpoint A marker for future immutable inspection options.
///
/// Redaction and projection options are reserved for later reviewed
/// checkpoints.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InspectionOptions;
