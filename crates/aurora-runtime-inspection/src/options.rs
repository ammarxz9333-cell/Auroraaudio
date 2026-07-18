//! Inspection-owned option contracts.

/// Immutable options controlling inspection-owned projection behavior.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InspectionOptions {
    reveal_local_identifiers: bool,
}

impl InspectionOptions {
    /// Returns the redacted-by-default options.
    pub const fn redacted() -> Self {
        Self {
            reveal_local_identifiers: false,
        }
    }

    /// Returns options that retain identifiers for explicit local inspection.
    ///
    /// This option does not resolve identifiers or claim that a device exists.
    pub const fn unredacted_local() -> Self {
        Self {
            reveal_local_identifiers: true,
        }
    }

    /// Returns whether explicitly requested local identifiers are retained.
    pub const fn reveals_local_identifiers(self) -> bool {
        self.reveal_local_identifiers
    }
}
