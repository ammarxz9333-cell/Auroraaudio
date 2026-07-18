//! JSON formatter contract boundary.

/// Marker reserved for the future Checkpoint C deterministic JSON formatter.
///
/// This type provides no formatting or serialization behavior in Checkpoint B.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct JsonFormatter;
