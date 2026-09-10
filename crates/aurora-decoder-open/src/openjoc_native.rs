//! Native open JOC decode + speaker render adapter using the Apache-2.0
//! `openjoc-api` implementation as the admitted JOC rendering backend.
//!
//! Aurora does not load any Dolby binary or licensed runtime. JOC admission is
//! performed through OpenJOC's own positive complete-access-unit classifier;
//! this renderer is created only after positive admission and Aurora still
//! claims JOC/Atmos playback only after successful OpenJOC speaker rendering.

use std::collections::VecDeque;

use aurora_core::{AudioBlock, AudioFormat};
use aurora_decoder_api::{DecodedFrame, DecoderError};
use openjoc_api::{
    OpenJocConfig, OpenJocPacket, OpenJocSession, OpenJocStatus, RenderMode,
    ValidationProfile,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JocRenderInfo {
    pub layout_name: String,
    pub channel_count: usize,
    pub latency_samples: usize,
    pub object_count: Option<u16>,
    pub complexity_index: Option<u8>,
}

pub struct OpenJocNativeRenderer {
    session: OpenJocSession,
    output: AudioFormat,
    /// Aurora output index -> OpenJOC source index.
    channel_map: Vec<usize>,
    channels: Vec<VecDeque<f32>>,
    emitted_frames: u64,
    discontinuity: bool,
    last_info: JocRenderInfo,
}

impl OpenJocNativeRenderer {
    pub fn new(output: AudioFormat, layout_hint: Option<&str>) -> Result<Self, DecoderError> {
        let layout = match layout_hint {
            Some(name) if !name.trim().is_empty() => name.to_owned(),
            _ => default_layout_for_channels(output.channel_count)
                .ok_or(DecoderError::UnsupportedInput(
                    "JOC speaker layout is ambiguous; provide an explicit OpenJOC layout hint",
                ))?
                .to_owned(),
        };
        let mut config = OpenJocConfig::default();
        config.render_mode = RenderMode::Speaker;
        config.speaker_layout = layout.clone();
        config.validation_profile = ValidationProfile::Auto;
        let session = OpenJocSession::new(config)
            .map_err(|e| DecoderError::ExternalProcess(format!("OpenJOC init failed: {e}")))?;
        let info = session.output_info();
        if info.channel_count != output.channel_count {
            return Err(DecoderError::UnsupportedInput(
                "selected JOC layout channel count does not match Aurora output format",
            ));
        }
        let channel_map = aurora_channel_map(&info.layout_name, info.channel_count)?;
        let last_info = JocRenderInfo {
            layout_name: info.layout_name,
            channel_count: info.channel_count,
            latency_samples: info.latency_samples,
            object_count: None,
            complexity_index: None,
        };
        Ok(Self {
            session,
            output,
            channel_map,
            channels: (0..output.channel_count).map(|_| VecDeque::new()).collect(),
            emitted_frames: 0,
            discontinuity: true,
            last_info,
        })
    }

    pub fn render_info(&self) -> &JocRenderInfo {
        &self.last_info
    }

    /// Push one complete E-AC-3/JOC access unit.
    pub fn push_access_unit(&mut self, bytes: &[u8]) -> Result<(), DecoderError> {
        if bytes.is_empty() {
            return Ok(());
        }
        // Direct eARC currently reaches this boundary without a source-domain
        // sample PTS. Do not synthesize one from `emitted_frames`: Aurora
        // reblocks 1536-sample E-AC-3 access units into 40-frame output blocks,
        // so emitted output lags the input timeline whenever a short tail remains
        // queued. OpenJOC supports `None` and maintains decode sequence internally.
        //
        // `OutputPending` means OpenJOC did not consume this packet. Drain the
        // prior owned PCM and retry this exact AU once; silently returning here
        // would otherwise drop a compressed access unit under backpressure.
        let mut retried_after_pending = false;
        loop {
            let status = self
                .session
                .push_packet(OpenJocPacket {
                    data: bytes,
                    pts_samples: None,
                    discontinuity: false,
                    preroll: false,
                })
                .map_err(|e| {
                    DecoderError::ExternalProcess(format!("OpenJOC decode/render failed: {e}"))
                })?;

            if status != OpenJocStatus::OutputPending {
                self.collect_output()?;
                break;
            }

            self.collect_output()?;
            if retried_after_pending {
                return Err(DecoderError::Decode(
                    "OpenJOC remained OutputPending after Aurora drained prior PCM".to_owned(),
                ));
            }
            retried_after_pending = true;
        }

        let diagnostics = self.session.diagnostics();
        self.last_info.object_count = diagnostics.object_count;
        self.last_info.complexity_index = diagnostics.complexity_index;
        Ok(())
    }

    /// Return one exact Aurora processing block when enough rendered samples
    /// are queued. A 1536-sample JOC AU is therefore reblocked losslessly into
    /// the 40-frame realtime cadence without resampling.
    pub fn take_block(&mut self) -> Option<DecodedFrame> {
        self.take_frames(self.output.block_size.max(1))
    }

    /// Remove every PCM sample already collected from OpenJOC without asking
    /// the decoder session to process or drain anything further. This is used
    /// when the next compressed AU fails: previously rendered PCM must not be
    /// discarded merely because the new AU is bad.
    pub(crate) fn take_buffered_frames(&mut self) -> Result<Vec<DecodedFrame>, DecoderError> {
        let mut frames = Vec::new();
        while let Some(frame) = self.take_block() {
            frames.push(frame);
        }

        let remaining = self.channels.first().map(VecDeque::len).unwrap_or(0);
        if self.channels.iter().any(|channel| channel.len() != remaining) {
            return Err(DecoderError::Decode(
                "OpenJOC channel queues diverged while retiring buffered PCM".to_owned(),
            ));
        }
        if remaining > 0 {
            frames.push(
                self.take_frames(remaining)
                    .expect("all channel queues were checked above"),
            );
        }
        Ok(frames)
    }

    /// Finalize OpenJOC and return every remaining PCM sample. Full realtime
    /// blocks are emitted first; a final short block is emitted without padding
    /// so finite files/fixtures do not lose up to `block_size - 1` samples and
    /// the presentation timeline is not extended with synthetic silence.
    pub fn drain(&mut self) -> Result<Vec<DecodedFrame>, DecoderError> {
        self.session
            .drain()
            .map_err(|e| DecoderError::ExternalProcess(format!("OpenJOC drain failed: {e}")))?;
        self.collect_output()?;
        self.take_buffered_frames()
    }

    pub fn reset(&mut self) -> Result<(), DecoderError> {
        self.session.reset();
        for channel in &mut self.channels {
            channel.clear();
        }
        self.emitted_frames = 0;
        self.discontinuity = true;
        self.last_info.object_count = None;
        self.last_info.complexity_index = None;
        Ok(())
    }

    fn take_frames(&mut self, frame_count: usize) -> Option<DecodedFrame> {
        if frame_count == 0 || self.channels.iter().any(|channel| channel.len() < frame_count) {
            return None;
        }
        let mut planar = Vec::with_capacity(self.channels.len());
        for channel in &mut self.channels {
            let mut samples = Vec::with_capacity(frame_count);
            for _ in 0..frame_count {
                samples.push(channel.pop_front().expect("length checked above"));
            }
            planar.push(samples);
        }
        let pts = self.emitted_frames as f64 / f64::from(self.output.sample_rate);
        self.emitted_frames = self.emitted_frames.saturating_add(frame_count as u64);
        let discontinuity = std::mem::replace(&mut self.discontinuity, false);
        Some(DecodedFrame {
            audio: AudioBlock {
                channels: planar,
                frame_count,
                presentation_time_seconds: pts,
                discontinuity,
            },
            // OpenJOC has already rendered the decoded object scene to the
            // requested physical layout. Aurora object telemetry is attached by
            // the metadata observer, not duplicated as a second renderer input.
            objects: Vec::new(),
        })
    }

    fn collect_output(&mut self) -> Result<(), DecoderError> {
        while let Some(frame) = self.session.receive_frame() {
            if frame.sample_rate != self.output.sample_rate
                || frame.channel_count != self.output.channel_count
                || frame.interleaved_f32.len()
                    != frame.sample_count.saturating_mul(frame.channel_count)
            {
                return Err(DecoderError::UnsupportedInput(
                    "OpenJOC output format changed or returned malformed PCM",
                ));
            }
            if frame.interleaved_f32.iter().any(|sample| !sample.is_finite()) {
                return Err(DecoderError::Decode(
                    "OpenJOC returned non-finite PCM; refusing to sanitize corrupted decoder output"
                        .to_owned(),
                ));
            }
            for source_frame in frame.interleaved_f32.chunks_exact(frame.channel_count) {
                for (destination, source_index) in
                    self.channels.iter_mut().zip(self.channel_map.iter().copied())
                {
                    destination.push_back(source_frame[source_index]);
                }
            }
        }
        Ok(())
    }
}

/// Returns Aurora output index -> OpenJOC source index.
///
/// OpenJOC's 7.1-family presets use `FL FR FC LFE Lb Rb Ls Rs ...`, while
/// Aurora's canonical 7.1/7.1.4 order is `FL FR FC LFE SL SR SBL SBR ...`.
/// The explicit permutation keeps side/back calibration, bass management and
/// physical routing attached to the correct semantic speakers.
fn aurora_channel_map(layout: &str, channels: usize) -> Result<Vec<usize>, DecoderError> {
    let map: Vec<usize> = match (layout, channels) {
        ("7.1", 8) => vec![0, 1, 2, 3, 6, 7, 4, 5],
        ("7.1.4", 12) => vec![0, 1, 2, 3, 6, 7, 4, 5, 8, 9, 10, 11],
        // These admitted presets already match Aurora's corresponding semantic
        // order for the channel roles Aurora currently names explicitly.
        ("2.0", 2) | ("5.1", 6) | ("5.1.4", 10) => (0..channels).collect(),
        // Other layouts are usable only as explicitly selected non-canonical
        // targets. Aurora has no complete semantic role contract for them yet,
        // so preserve the renderer-owned order rather than guessing a mapping.
        (_, count) => (0..count).collect(),
    };
    if map.len() != channels
        || map.iter().any(|&index| index >= channels)
        || {
            let mut sorted = map.clone();
            sorted.sort_unstable();
            sorted.dedup();
            sorted.len() != channels
        }
    {
        return Err(DecoderError::UnsupportedInput(
            "OpenJOC-to-Aurora channel map is not a complete permutation",
        ));
    }
    Ok(map)
}

pub const fn default_layout_for_channels(channels: usize) -> Option<&'static str> {
    match channels {
        2 => Some("2.0"),
        6 => Some("5.1"),
        8 => Some("7.1"),
        10 => Some("5.1.4"),
        12 => Some("7.1.4"),
        14 => Some("7.1.6"),
        16 => Some("9.1.6"),
        24 => Some("22.2"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aurora_7_1_4_maps_to_openjoc_7_1_4() {
        assert_eq!(default_layout_for_channels(12), Some("7.1.4"));
    }

    #[test]
    fn openjoc_7_1_4_side_back_order_is_normalized_to_aurora() {
        assert_eq!(
            aurora_channel_map("7.1.4", 12).unwrap(),
            vec![0, 1, 2, 3, 6, 7, 4, 5, 8, 9, 10, 11]
        );
    }

    #[test]
    fn openjoc_7_1_side_back_order_is_normalized_to_aurora() {
        assert_eq!(
            aurora_channel_map("7.1", 8).unwrap(),
            vec![0, 1, 2, 3, 6, 7, 4, 5]
        );
    }

    #[test]
    fn ambiguous_custom_count_fails_closed() {
        assert_eq!(default_layout_for_channels(11), None);
    }
}
