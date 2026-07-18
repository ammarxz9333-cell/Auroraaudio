//! JSON formatter contract boundary.

/// Empty Checkpoint A marker for a future deterministic JSON formatter.
///
/// This type provides no formatting or serialization behavior in Checkpoint A.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct JsonFormatter;
