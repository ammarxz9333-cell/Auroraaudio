//! Bounded compressed-stream ingest buffer and external-decoder boundary.
//!
//! This module does not claim codec decoding. It reports the unavailable
//! decoder error until a reviewed decoder is connected.

use std::collections::VecDeque;

use aurora_decoder_api::{DecodedFrame, Decoder};
use thiserror::Error;

use crate::decoder::Eac3AtmosDecoder;

/// Diagnostics and telemetry collected during live stream ingest.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct IngestTelemetry {
    /// Total raw bytes ingested from the input stream.
    pub total_bytes_ingested: u64,
    /// Number of successfully decoded audio frames.
    pub frames_decoded: u64,
    /// Number of detected stream discontinuities or resyncs.
    pub discontinuities_detected: u64,
    /// Number of active dynamic Atmos 3D objects in the last decoded frame.
    pub active_objects_count: usize,
    /// Whether the decoder currently has active lock on the stream.
    pub is_locked: bool,
    /// Presentation timestamp of the most recent frame in seconds.
    pub last_pts_seconds: f64,
}

/// Errors occurring during live stream ingest.
#[derive(Debug, Error)]
pub enum IngestError {
    /// Internal ring buffer capacity exceeded.
    #[error("ingest ring buffer capacity exceeded ({0} bytes)")]
    BufferOverflow(usize),
    /// Decoder error forwarded from the underlying decoder.
    #[error("decoder error: {0}")]
    Decoder(#[from] aurora_decoder_api::DecoderError),
}

/// Live streaming ingest engine.
pub struct LiveStreamIngest {
    decoder: Eac3AtmosDecoder,
    raw_buffer: VecDeque<u8>,
    max_buffer_bytes: usize,
    telemetry: IngestTelemetry,
    consecutive_empty_reads: usize,
}

impl LiveStreamIngest {
    /// Creates a new live stream ingest instance with specified max buffer size.
    pub fn new(max_buffer_bytes: usize) -> Self {
        Self {
            decoder: Eac3AtmosDecoder::new(),
            raw_buffer: VecDeque::with_capacity(max_buffer_bytes.min(65536)),
            max_buffer_bytes,
            telemetry: IngestTelemetry::default(),
            consecutive_empty_reads: 0,
        }
    }

    /// Ingests an incoming packet of raw bytes (e.g. from USB, socket, or pipe).
    pub fn ingest_bytes(&mut self, chunk: &[u8]) -> Result<(), IngestError> {
        if self.raw_buffer.len() + chunk.len() > self.max_buffer_bytes {
            self.telemetry.discontinuities_detected += 1;
            self.decoder.signal_discontinuity();
            return Err(IngestError::BufferOverflow(self.raw_buffer.len() + chunk.len()));
        }

        self.raw_buffer.extend(chunk);
        self.telemetry.total_bytes_ingested += chunk.len() as u64;
        self.consecutive_empty_reads = 0;
        Ok(())
    }

    /// Pulls the next decoded audio frame and associated 3D spatial objects.
    pub fn poll_next_frame(&mut self) -> Result<Option<DecodedFrame>, IngestError> {
        if self.raw_buffer.is_empty() {
            self.consecutive_empty_reads += 1;
            if self.consecutive_empty_reads > 10 {
                self.telemetry.is_locked = false;
            }
            return Ok(None);
        }

        // Convert slice to feed decoder
        let slice: Vec<u8> = self.raw_buffer.drain(..).collect();
        match self.decoder.decode_chunk(&slice) {
            Ok(Some(frame)) => {
                self.telemetry.frames_decoded += 1;
                self.telemetry.is_locked = true;
                self.telemetry.active_objects_count = frame.objects.len();
                self.telemetry.last_pts_seconds = frame.audio.presentation_time_seconds;
                Ok(Some(frame))
            }
            Ok(None) => {
                // Incomplete frame; restore bytes
                self.raw_buffer.extend(slice);
                Ok(None)
            }
            Err(e) => {
                self.telemetry.discontinuities_detected += 1;
                self.telemetry.is_locked = false;
                Err(IngestError::Decoder(e))
            }
        }
    }

    /// Returns a copy of current ingest diagnostics and telemetry.
    pub fn telemetry(&self) -> IngestTelemetry {
        self.telemetry.clone()
    }

    /// Resets the ingest engine and clears internal buffers.
    pub fn reset(&mut self) {
        self.raw_buffer.clear();
        self.decoder.reset();
        self.telemetry.is_locked = false;
        self.telemetry.discontinuities_detected += 1;
        self.consecutive_empty_reads = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compressed_input_reports_decoder_unavailable() {
        let mut ingest = LiveStreamIngest::new(4096);
        ingest.ingest_bytes(&[0x0b, 0x77, 0, 0, 0, 0]).unwrap();

        assert!(matches!(
            ingest.poll_next_frame(),
            Err(IngestError::Decoder(
                aurora_decoder_api::DecoderError::Unavailable(_)
            ))
        ));
        let telemetry = ingest.telemetry();
        assert_eq!(telemetry.frames_decoded, 0);
        assert!(!telemetry.is_locked);
        assert_eq!(telemetry.discontinuities_detected, 1);
    }
}
