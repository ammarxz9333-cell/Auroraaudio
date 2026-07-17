use std::collections::VecDeque;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::sync::{Arc, Mutex};

use crate::{DiagnosticEvent, Severity};

/// Default maximum retained size estimate for one event.
pub const DEFAULT_MAX_EVENT_BYTES: usize = 16 * 1024;

/// Observable outcome of one bounded log insertion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum LogDisposition {
    /// Event was retained without eviction.
    Retained,
    /// Event was retained and the oldest event was evicted.
    RetainedAfterEviction,
    /// Event was below the configured severity threshold.
    Filtered,
    /// Event exceeded the configured per-event byte ceiling.
    RejectedOversized,
}

/// Failures returned by bounded diagnostic log operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticLogError {
    /// A zero-capacity log was requested.
    ZeroCapacity,
    /// A zero per-event byte ceiling was requested.
    ZeroEventBytes,
    /// A shared log mutex was poisoned by a panicking control thread.
    LockPoisoned,
}

impl Display for DiagnosticLogError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroCapacity => formatter.write_str("diagnostic log capacity must be non-zero"),
            Self::ZeroEventBytes => {
                formatter.write_str("diagnostic event byte ceiling must be non-zero")
            }
            Self::LockPoisoned => formatter.write_str("diagnostic log lock is poisoned"),
        }
    }
}

impl Error for DiagnosticLogError {}

/// Bounded, severity-filtered control-thread event retention.
#[derive(Debug)]
pub struct DiagnosticLog {
    capacity: usize,
    minimum_severity: Severity,
    events: VecDeque<DiagnosticEvent>,
    max_event_bytes: usize,
    dropped_events: u64,
    filtered_events: u64,
    oversized_events: u64,
}

impl DiagnosticLog {
    /// Creates an empty log with exactly the requested retained-event capacity.
    pub fn new(capacity: usize, minimum_severity: Severity) -> Result<Self, DiagnosticLogError> {
        Self::with_max_event_bytes(capacity, minimum_severity, DEFAULT_MAX_EVENT_BYTES)
    }

    /// Creates a log with explicit event count and per-event byte ceilings.
    pub fn with_max_event_bytes(
        capacity: usize,
        minimum_severity: Severity,
        max_event_bytes: usize,
    ) -> Result<Self, DiagnosticLogError> {
        if capacity == 0 {
            return Err(DiagnosticLogError::ZeroCapacity);
        }
        if max_event_bytes == 0 {
            return Err(DiagnosticLogError::ZeroEventBytes);
        }
        Ok(Self {
            capacity,
            minimum_severity,
            events: VecDeque::with_capacity(capacity),
            max_event_bytes,
            dropped_events: 0,
            filtered_events: 0,
            oversized_events: 0,
        })
    }

    /// Retains an event if it passes the current severity filter.
    pub fn push(&mut self, event: DiagnosticEvent) -> LogDisposition {
        if event.severity < self.minimum_severity {
            self.filtered_events = self.filtered_events.saturating_add(1);
            return LogDisposition::Filtered;
        }
        if event.estimated_size_bytes() > self.max_event_bytes {
            self.oversized_events = self.oversized_events.saturating_add(1);
            return LogDisposition::RejectedOversized;
        }
        let disposition = if self.events.len() == self.capacity {
            self.events.pop_front();
            self.dropped_events = self.dropped_events.saturating_add(1);
            LogDisposition::RetainedAfterEviction
        } else {
            LogDisposition::Retained
        };
        self.events.push_back(event);
        disposition
    }

    /// Changes the severity threshold for future events.
    pub fn set_minimum_severity(&mut self, minimum_severity: Severity) {
        self.minimum_severity = minimum_severity;
    }

    /// Returns retained events from oldest to newest.
    pub fn events(&self) -> impl ExactSizeIterator<Item = &DiagnosticEvent> {
        self.events.iter()
    }

    /// Serializes retained events as deterministic newline-delimited JSON.
    pub fn to_json_lines(&self) -> Result<String, serde_json::Error> {
        let mut output = String::new();
        for event in &self.events {
            output.push_str(&serde_json::to_string(event)?);
            output.push('\n');
        }
        Ok(output)
    }

    /// Formats retained events as deterministic human-readable lines.
    #[must_use]
    pub fn to_human_lines(&self) -> String {
        let mut output = String::new();
        for event in &self.events {
            output.push_str(&event.to_human_line());
            output.push('\n');
        }
        output
    }

    /// Returns the configured event capacity.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns the configured retained byte ceiling for each event.
    #[must_use]
    pub const fn max_event_bytes(&self) -> usize {
        self.max_event_bytes
    }

    /// Returns the number of events evicted because the log was full.
    #[must_use]
    pub const fn dropped_events(&self) -> u64 {
        self.dropped_events
    }

    /// Returns the number of events rejected by severity filtering.
    #[must_use]
    pub const fn filtered_events(&self) -> u64 {
        self.filtered_events
    }

    /// Returns the number of events rejected for exceeding the byte ceiling.
    #[must_use]
    pub const fn oversized_events(&self) -> u64 {
        self.oversized_events
    }
}

/// Cloneable control-thread handle for concurrent diagnostic producers.
#[derive(Clone, Debug)]
pub struct SharedDiagnosticLog {
    inner: Arc<Mutex<DiagnosticLog>>,
}

impl SharedDiagnosticLog {
    /// Creates a shared bounded log.
    pub fn new(capacity: usize, minimum_severity: Severity) -> Result<Self, DiagnosticLogError> {
        Ok(Self {
            inner: Arc::new(Mutex::new(DiagnosticLog::new(capacity, minimum_severity)?)),
        })
    }

    /// Pushes one event from a non-real-time thread.
    pub fn push(&self, event: DiagnosticEvent) -> Result<LogDisposition, DiagnosticLogError> {
        let disposition = self
            .inner
            .lock()
            .map_err(|_| DiagnosticLogError::LockPoisoned)?
            .push(event);
        Ok(disposition)
    }

    /// Runs a read-only operation while holding the control-thread lock.
    pub fn inspect<T>(
        &self,
        inspect: impl FnOnce(&DiagnosticLog) -> T,
    ) -> Result<T, DiagnosticLogError> {
        let guard = self
            .inner
            .lock()
            .map_err(|_| DiagnosticLogError::LockPoisoned)?;
        Ok(inspect(&guard))
    }
}
