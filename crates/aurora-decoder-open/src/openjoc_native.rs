//! Native open JOC decode + speaker render adapter using the Apache-2.0
//! `openjoc-api` implementation as an independent standards-derived backend.
//!
//! Aurora does not load any Dolby binary or licensed runtime. This backend is
//! intentionally paired with the independent OxideAV JOC admission probe so a
//! stream is not promoted to immersive output on one implementation's guess.

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
            channels: (0..output.channel_count).map(|_| VecDeque::new()).collect(),
            emitted_frames: 0,
            discontinuity: true,
            last_info,
        })
    }

    pub fn render_info(&self) -> &JocRenderInfo {
        &self.last_info
    }

    /// Push one complete six-block E-AC-3 JOC access unit.
    pub fn push_access_unit(&mut self, bytes: &[u8]) -> Result<(), DecoderError> {
        if bytes.is_empty() {
            return Ok(());
        }
        let status = self
            .session
            .push_packet(OpenJocPacket {
                data: bytes,
                pts_samples: Some(self.emitted_frames.min(i64::MAX as u64) as i64),
                discontinuity: false,
                preroll: false,
            })
            .map_err(|e| DecoderError::ExternalProcess(format!("OpenJOC decode/render failed: {e}")))?;
        if status == OpenJocStatus::OutputPending {
            self.collect_output()?;
        }
        self.collect_output()?;
        let diagnostics = self.session.diagnostics();
        self.last_info.object_count = diagnostics.object_count;
        self.last_info.complexity_index = diagnostics.complexity_index;
        Ok(())
    }

    /// Return one exact Aurora processing block when enough rendered samples
    /// are queued. A 1536-sample JOC AU is therefore reblocked losslessly into
    /// the 40-frame realtime cadence without resampling.
    pub fn take_block(&mut self) -> Option<DecodedFrame> {
        let block = self.output.block_size.max(1);
        if self.channels.iter().any(|channel| channel.len() < block) {
            return None;
        }
        let mut planar = Vec::with_capacity(self.channels.len());
        for channel in &mut self.channels {
            let mut samples = Vec::with_capacity(block);
            for _ in 0..block {
                samples.push(channel.pop_front().expect("length checked above"));
            }
            planar.push(samples);
        }
        let pts = self.emitted_frames as f64 / f64::from(self.output.sample_rate);
        self.emitted_frames = self.emitted_frames.saturating_add(block as u64);
        let discontinuity = std::mem::replace(&mut self.discontinuity, false);
        Some(DecodedFrame {
            audio: AudioBlock {
                channels: planar,
                frame_count: block,
                presentation_time_seconds: pts,
                discontinuity,
            },
            // OpenJOC has already rendered the decoded object scene to the
            // requested physical layout. Aurora object telemetry is attached by
            // the metadata observer, not duplicated as a second renderer input.
            objects: Vec::new(),
        })
    }

    pub fn drain(&mut self) -> Result<Vec<DecodedFrame>, DecoderError> {
        self.session
            .drain()
            .map_err(|e| DecoderError::ExternalProcess(format!("OpenJOC drain failed: {e}")))?;
        self.collect_output()?;
        let mut frames = Vec::new();
        while let Some(frame) = self.take_block() {
            frames.push(frame);
        }
        // Keep a short tail rather than silently padding it into the live
        // timeline; the caller can discard it on a seek/reset or a future API
        // can expose an explicit final-short-block contract.
        Ok(frames)
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
            for sample in frame.interleaved_f32.chunks_exact(frame.channel_count) {
                for (channel, value) in self.channels.iter_mut().zip(sample.iter().copied()) {
                    channel.push_back(if value.is_finite() { value } else { 0.0 });
                }
            }
        }
        Ok(())
    }
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
    fn ambiguous_custom_count_fails_closed() {
        assert_eq!(default_layout_for_channels(11), None);
    }
}
