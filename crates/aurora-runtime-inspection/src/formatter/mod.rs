//! Deterministic bounded formatter implementations.

use crate::{InspectionError, InspectionFloatField, InspectionReport};

/// Maximum bytes returned by the compact JSON formatter.
pub const MAX_JSON_BYTES: usize = 256 * 1024;
/// Maximum bytes returned by the stable text formatter.
pub const MAX_TEXT_BYTES: usize = 256 * 1024;
/// Maximum nesting depth allowed by inspection schema formatters.
pub const MAX_NESTING_DEPTH: usize = 8;
/// Maximum total entries across all serialized report collections.
pub const MAX_SERIALIZED_COLLECTION_ENTRIES: usize = 256;

const SCHEMA_NESTING_DEPTH: usize = 5;

pub mod json;
pub mod text;

pub use json::JsonFormatter;
pub use text::TextFormatter;

fn validate_report(report: &InspectionReport) -> Result<(), InspectionError> {
    if SCHEMA_NESTING_DEPTH > MAX_NESTING_DEPTH {
        return Err(InspectionError::NestingDepthExceeded {
            actual: SCHEMA_NESTING_DEPTH,
            maximum: MAX_NESTING_DEPTH,
        });
    }

    let topology = &report.runtime().prepared_topology;
    let capacities = &report.runtime().capacities;
    let setup = report.setup();
    let counts = [
        report.findings().len(),
        report.represented_semantics().len(),
        topology.inputs.len(),
        topology.outputs.len(),
        topology.routes.len(),
        topology.inactive_output_ids.len(),
        topology.speakers.len(),
        capacities.deferred_until_setup.len(),
        setup.stages.len(),
        setup.dependencies.len(),
    ];
    let entries = counts.into_iter().try_fold(0usize, |total, count| {
        total
            .checked_add(count)
            .ok_or(InspectionError::SerializedCollectionAccountingOverflow)
    })?;
    if entries > MAX_SERIALIZED_COLLECTION_ENTRIES {
        return Err(InspectionError::SerializedCollectionEntriesExceeded {
            actual: entries,
            maximum: MAX_SERIALIZED_COLLECTION_ENTRIES,
        });
    }

    if report
        .runtime()
        .prepared_renderer
        .horizontal_spread
        .is_some_and(|value| !value.is_finite())
    {
        return Err(InspectionError::NonFiniteFloat {
            field: InspectionFloatField::HorizontalSpread,
        });
    }
    for speaker in &topology.speakers {
        for (value, field) in [
            (
                speaker.normalized_x,
                InspectionFloatField::SpeakerNormalizedX,
            ),
            (
                speaker.normalized_y,
                InspectionFloatField::SpeakerNormalizedY,
            ),
            (
                speaker.normalized_z,
                InspectionFloatField::SpeakerNormalizedZ,
            ),
        ] {
            if !value.is_finite() {
                return Err(InspectionError::NonFiniteFloat { field });
            }
        }
    }
    Ok(())
}
