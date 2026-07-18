//! Deterministic compact JSON formatting for inspection-owned reports.

use std::io::{self, Write};

use crate::{InspectionError, InspectionOutputFormat, InspectionReport};

use super::{validate_report, MAX_JSON_BYTES};

/// Stateless deterministic JSON formatter for inspection-owned reports.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct JsonFormatter;

impl JsonFormatter {
    /// Formats one inspection report as compact schema-ordered JSON.
    ///
    /// Only inspection-owned values are serialized. The method preserves the
    /// report's canonical vectors, emits no timestamps or host values, and
    /// validates finite structure and output bounds before returning.
    ///
    /// # Errors
    ///
    /// Returns a structured [`InspectionError`] when report structure, floats,
    /// collection counts, serialization, or the published JSON byte limit are
    /// invalid.
    pub fn format(report: &InspectionReport) -> Result<String, InspectionError> {
        validate_report(report)?;
        let mut writer = BoundedJsonWriter::default();
        if serde_json::to_writer(&mut writer, report).is_err() {
            if let Some(actual) = writer.overflow_actual {
                return Err(InspectionError::SerializedOutputTooLarge {
                    format: InspectionOutputFormat::Json,
                    actual,
                    maximum: MAX_JSON_BYTES,
                });
            }
            return Err(InspectionError::JsonSerializationFailed);
        }
        String::from_utf8(writer.bytes).map_err(|_| InspectionError::JsonSerializationFailed)
    }
}

struct BoundedJsonWriter {
    bytes: Vec<u8>,
    overflow_actual: Option<usize>,
}

impl Default for BoundedJsonWriter {
    fn default() -> Self {
        Self {
            bytes: Vec::with_capacity(4 * 1024),
            overflow_actual: None,
        }
    }
}

impl Write for BoundedJsonWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let actual = self.bytes.len().saturating_add(buffer.len());
        if actual > MAX_JSON_BYTES {
            self.overflow_actual = Some(actual);
            return Err(io::Error::other("inspection JSON byte limit exceeded"));
        }
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
