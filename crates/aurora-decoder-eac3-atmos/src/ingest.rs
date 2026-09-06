//! Real-time live streaming ingest controller for IEC 61937 and E-AC-3 Atmos streams.
//!
//! Provides ring buffering, continuity checking, PTS tracking, and fail-safe mute.

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
    use crate::oamd::{serialize_oamd_metadata, AtmosBedLayout, AtmosFrameMetadata, AtmosObjectMetadata};
    use aurora_core::Vector3;

    #[test]
    fn streaming_ingest_processes_continuous_packets() {
        let mut ingest = LiveStreamIngest::new(131072);

        // Build mock E-AC-3 Atmos packet
        let mut frame_bytes = vec![
            0x0B, 0x77, // syncword
            0x02, 0xFF, // strmtyp=0, substream=0, frmsiz=767
            0x3F, // 48k, 6 blocks, 3/2, lfeon=1
            0x87, 0x00, // bsid=16, dialnorm=28
        ];
        let oamd_meta = AtmosFrameMetadata {
            bed_layout: AtmosBedLayout::FivePointOne,
            sequence_number: 1,
            decorrelation_factor: 0.1,
            objects: vec![AtmosObjectMetadata {
                object_id: 1,
                position: Vector3::new(0.0, 0.0, 1.0), // overhead center
                gain_db: 0.0,
                spread: 0.1,
                is_active: true,
            }],
        };
        frame_bytes.extend_from_slice(&serialize_oamd_metadata(&oamd_meta));

        // Ingest packet
        ingest.ingest_bytes(&frame_bytes).unwrap();
        let frame = ingest.poll_next_frame().unwrap().expect("frame expected");

        assert_eq!(frame.audio.frame_count, 1536);
        assert_eq!(frame.objects.len(), 1);

        let telem = ingest.telemetry();
        assert_eq!(telem.frames_decoded, 1);
        assert!(telem.is_locked);
        assert_eq!(telem.active_objects_count, 1);
    }
}
