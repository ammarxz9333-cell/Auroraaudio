//! Native open JOC decode + speaker render adapter using the Apache-2.0
//! `openjoc-api` implementation as the admitted JOC rendering backend.
//!
//! Aurora does not load any Dolby binary or licensed runtime. JOC admission is
//! performed through OpenJOC's own positive complete-access-unit classifier;
//! this renderer is created only after positive admission and Aurora still
//! claims JOC/Atmos playback only after successful OpenJOC speaker rendering.

use std::collections::VecDeque;
use std::time::Duration;

use aurora_core::{AudioBlock, AudioFormat};
use aurora_decoder_api::{DecodedFrame, DecoderError};
use openjoc_api::{
    OpenJocConfig, OpenJocPacket, OpenJocPcmFrame, OpenJocSession, OpenJocStatus,
    PcmSampleFormat, RenderMode, ValidationProfile,
};

const LABELS_2_0: [&str; 2] = ["FL", "FR"];
const LABELS_5_1: [&str; 6] = ["FL", "FR", "FC", "LFE", "Ls", "Rs"];
const LABELS_5_1_4: [&str; 10] = [
    "FL", "FR", "FC", "LFE", "Ls", "Rs", "TFL", "TFR", "TBL", "TBR",
];
const LABELS_7_1: [&str; 8] = ["FL", "FR", "FC", "LFE", "Lb", "Rb", "Ls", "Rs"];
const LABELS_7_1_4: [&str; 12] = [
    "FL", "FR", "FC", "LFE", "Lb", "Rb", "Ls", "Rs", "TFL", "TFR", "TBL", "TBR",
];
const MAX_RECYCLED_PLANAR_BLOCKS: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JocRenderInfo {
    pub layout_name: String,
    pub channel_count: usize,
    pub latency_samples: usize,
    pub object_count: Option<u16>,
    pub complexity_index: Option<u8>,
    pub last_decode_time_us: Option<u64>,
    pub last_render_time_us: Option<u64>,
    pub last_total_time_us: Option<u64>,
    pub max_total_time_us: Option<u64>,
}

pub struct OpenJocNativeRenderer {
    session: OpenJocSession,
    output: AudioFormat,
    channel_map: Vec<usize>,
    expected_channel_labels: Vec<String>,
    channels: Vec<VecDeque<f32>>,
    /// Reused outer staging storage for one transactional receive cycle. The
    /// PCM vectors themselves remain owned by OpenJOC frames; retaining this
    /// vector removes an avoidable allocation from every successful JOC AU.
    ready_frames: Vec<OpenJocPcmFrame>,
    /// Bounded pool of Aurora planar blocks returned by the downstream playback
    /// runtime. A 40-frame 7.1.4 JOC AU otherwise allocates one outer Vec plus
    /// twelve channel Vecs for every emitted block. Recycling keeps the exact
    /// public DecodedFrame contract while removing those steady-state heap hits.
    recycled_planar: Vec<Vec<Vec<f32>>>,
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
                    "JOC speaker layout is ambiguous or lacks a proven Aurora channel map; provide a verified supported layout",
                ))?
                .to_owned(),
        };
        let mut config = OpenJocConfig::default();
        config.render_mode = RenderMode::Speaker;
        config.speaker_layout = layout;
        config.validation_profile = ValidationProfile::Auto;
        let mut session = OpenJocSession::new(config)
            .map_err(|e| DecoderError::ExternalProcess(format!("OpenJOC init failed: {e}")))?;
        session.enable_stage_timing();
        let info = session.output_info();
        if info.sample_format != PcmSampleFormat::F32 {
            return Err(DecoderError::UnsupportedInput(
                "OpenJOC speaker output is not interleaved F32",
            ));
        }
        if let Some(sample_rate) = info.sample_rate {
            if sample_rate != output.sample_rate {
                return Err(DecoderError::UnsupportedInput(
                    "OpenJOC speaker output sample rate does not match Aurora output format",
                ));
            }
        }
        if info.channel_count != output.channel_count {
            return Err(DecoderError::UnsupportedInput(
                "selected JOC layout channel count does not match Aurora output format",
            ));
        }
        let expected_labels = expected_openjoc_channel_labels(&info.layout_name, info.channel_count)?;
        if info.channel_labels.len() != expected_labels.len()
            || info
                .channel_labels
                .iter()
                .map(String::as_str)
                .ne(expected_labels.iter().copied())
        {
            return Err(DecoderError::UnsupportedInput(
                "OpenJOC output labels do not match the verified Aurora speaker-layout contract",
            ));
        }
        let channel_map = aurora_channel_map(&info.layout_name, info.channel_count)?;
        let expected_channel_labels = info.channel_labels;
        let last_info = JocRenderInfo {
            layout_name: info.layout_name,
            channel_count: info.channel_count,
            latency_samples: info.latency_samples,
            object_count: None,
            complexity_index: None,
            last_decode_time_us: None,
            last_render_time_us: None,
            last_total_time_us: None,
            max_total_time_us: None,
        };
        Ok(Self {
            session,
            output,
            channel_map,
            expected_channel_labels,
            channels: (0..output.channel_count).map(|_| VecDeque::new()).collect(),
            ready_frames: Vec::with_capacity(2),
            recycled_planar: Vec::with_capacity(64),
            emitted_frames: 0,
            discontinuity: true,
            last_info,
        })
    }

    pub fn render_info(&self) -> &JocRenderInfo {
        &self.last_info
    }

    pub fn push_access_unit(&mut self, bytes: &[u8]) -> Result<(), DecoderError> {
        if bytes.is_empty() {
            return Ok(());
        }
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
        let timing = self.session.take_stage_timing();
        let decode_us = duration_us(timing.decode);
        let render_us = duration_us(timing.render);
        let total_us = duration_us(timing.total);
        self.last_info.last_decode_time_us = Some(decode_us);
        self.last_info.last_render_time_us = Some(render_us);
        self.last_info.last_total_time_us = Some(total_us);
        self.last_info.max_total_time_us = Some(
            self.last_info
                .max_total_time_us
                .map_or(total_us, |current| current.max(total_us)),
        );
        Ok(())
    }

    pub fn take_block(&mut self) -> Option<DecodedFrame> {
        self.take_frames(self.output.block_size.max(1))
    }

    /// Returns a consumed Aurora JOC frame's planar storage to a bounded pool.
    /// Frames from other backends or unexpected shapes are simply dropped.
    pub fn recycle_frame(&mut self, frame: DecodedFrame) {
        if !frame.objects.is_empty() {
            return;
        }
        let frame_count = frame.audio.frame_count;
        recycle_planar_storage(
            &mut self.recycled_planar,
            frame.audio.channels,
            self.output.channel_count,
            frame_count,
            self.output.block_size.max(1),
        );
    }

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
        self.ready_frames.clear();
        self.emitted_frames = 0;
        self.discontinuity = true;
        self.last_info.object_count = None;
        self.last_info.complexity_index = None;
        self.last_info.last_decode_time_us = None;
        self.last_info.last_render_time_us = None;
        self.last_info.last_total_time_us = None;
        self.last_info.max_total_time_us = None;
        Ok(())
    }

    fn take_frames(&mut self, frame_count: usize) -> Option<DecodedFrame> {
        if frame_count == 0 || self.channels.iter().any(|channel| channel.len() < frame_count) {
            return None;
        }
        let mut planar = take_planar_storage(
            &mut self.recycled_planar,
            self.channels.len(),
            frame_count,
        );
        for (samples, channel) in planar.iter_mut().zip(&mut self.channels) {
            for _ in 0..frame_count {
                samples.push(channel.pop_front().expect("length checked above"));
            }
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
            objects: Vec::new(),
        })
    }

    /// Receive every currently available OpenJOC frame, validate the entire
    /// batch first, and only then publish samples into Aurora's channel queues.
    /// This makes one receive cycle transactional: a malformed/non-finite later
    /// frame cannot leave partial PCM committed and then cause the same AU to be
    /// decoded again by the E-AC-3 bed fallback.
    fn collect_output(&mut self) -> Result<(), DecoderError> {
        self.ready_frames.clear();
        while let Some(frame) = self.session.receive_frame() {
            let malformed_shape = frame.sample_count == 0
                || frame.sample_format != PcmSampleFormat::F32
                || frame.sample_rate != self.output.sample_rate
                || frame.channel_count != self.output.channel_count
                || frame.layout_name.as_str() != self.last_info.layout_name.as_str()
                || frame.channel_labels.as_slice() != self.expected_channel_labels.as_slice()
                || frame.interleaved_f32.len()
                    != frame.sample_count.saturating_mul(frame.channel_count);
            if malformed_shape {
                self.ready_frames.clear();
                return Err(DecoderError::UnsupportedInput(
                    "OpenJOC output semantic layout, channel order or PCM format changed",
                ));
            }
            if frame.interleaved_f32.iter().any(|sample| !sample.is_finite()) {
                self.ready_frames.clear();
                return Err(DecoderError::Decode(
                    "OpenJOC returned non-finite PCM; refusing to sanitize corrupted decoder output"
                        .to_owned(),
                ));
            }
            self.ready_frames.push(frame);
        }

        for frame in &self.ready_frames {
            for source_frame in frame.interleaved_f32.chunks_exact(frame.channel_count) {
                for (destination, source_index) in
                    self.channels.iter_mut().zip(self.channel_map.iter().copied())
                {
                    destination.push_back(source_frame[source_index]);
                }
            }
        }
        self.ready_frames.clear();
        Ok(())
    }
}

fn take_planar_storage(
    pool: &mut Vec<Vec<Vec<f32>>>,
    channel_count: usize,
    frame_count: usize,
) -> Vec<Vec<f32>> {
    let mut planar = match pool.pop() {
        Some(planar) if planar.len() == channel_count => planar,
        _ => (0..channel_count)
            .map(|_| Vec::with_capacity(frame_count))
            .collect(),
    };
    for channel in &mut planar {
        channel.clear();
        if channel.capacity() < frame_count {
            channel.reserve(frame_count);
        }
    }
    planar
}

fn recycle_planar_storage(
    pool: &mut Vec<Vec<Vec<f32>>>,
    mut planar: Vec<Vec<f32>>,
    channel_count: usize,
    frame_count: usize,
    max_frame_count: usize,
) {
    if pool.len() >= MAX_RECYCLED_PLANAR_BLOCKS
        || frame_count == 0
        || frame_count > max_frame_count
        || planar.len() != channel_count
        || planar.iter().any(|channel| channel.len() != frame_count)
    {
        return;
    }
    for channel in &mut planar {
        channel.clear();
    }
    pool.push(planar);
}

fn duration_us(duration: Duration) -> u64 {
    duration.as_micros().min(u128::from(u64::MAX)) as u64
}

fn expected_openjoc_channel_labels(
    layout: &str,
    channels: usize,
) -> Result<&'static [&'static str], DecoderError> {
    match (layout, channels) {
        ("2.0", 2) => Ok(&LABELS_2_0),
        ("5.1", 6) => Ok(&LABELS_5_1),
        ("5.1.4", 10) => Ok(&LABELS_5_1_4),
        ("7.1", 8) => Ok(&LABELS_7_1),
        ("7.1.4", 12) => Ok(&LABELS_7_1_4),
        _ => Err(DecoderError::UnsupportedInput(
            "OpenJOC speaker layout has no verified semantic channel-label contract in Aurora",
        )),
    }
}

fn aurora_channel_map(layout: &str, channels: usize) -> Result<Vec<usize>, DecoderError> {
    let _ = expected_openjoc_channel_labels(layout, channels)?;
    let map: Vec<usize> = match (layout, channels) {
        ("7.1", 8) => vec![0, 1, 2, 3, 6, 7, 4, 5],
        ("7.1.4", 12) => vec![0, 1, 2, 3, 6, 7, 4, 5, 8, 9, 10, 11],
        ("2.0", 2) | ("5.1", 6) | ("5.1.4", 10) => (0..channels).collect(),
        _ => unreachable!("verified layout contract above covers supported mappings"),
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
        12 => Some("7.1.4"),
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
    fn ambiguous_widths_require_an_explicit_verified_layout() {
        assert_eq!(default_layout_for_channels(8), None);
        assert_eq!(default_layout_for_channels(10), None);
    }

    #[test]
    fn openjoc_7_1_4_side_back_order_is_normalized_to_aurora() {
        assert_eq!(
            aurora_channel_map("7.1.4", 12).unwrap(),
            vec![0, 1, 2, 3, 6, 7, 4, 5, 8, 9, 10, 11]
        );
        assert_eq!(
            expected_openjoc_channel_labels("7.1.4", 12).unwrap(),
            &LABELS_7_1_4
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
    fn unverified_same_width_layout_fails_closed() {
        assert!(aurora_channel_map("custom-12", 12).is_err());
        assert!(aurora_channel_map("9.1.2", 12).is_err());
        assert_eq!(default_layout_for_channels(16), None);
    }

    #[test]
    fn duration_conversion_saturates_into_u64_microseconds() {
        assert_eq!(duration_us(Duration::from_millis(3)), 3_000);
    }

    #[test]
    fn ambiguous_custom_count_fails_closed() {
        assert_eq!(default_layout_for_channels(11), None);
    }

    #[test]
    fn planar_pool_reuses_existing_channel_allocations() {
        let mut pool = Vec::new();
        let mut planar = (0..12)
            .map(|_| Vec::with_capacity(40))
            .collect::<Vec<_>>();
        for channel in &mut planar {
            channel.resize(40, 0.0);
        }
        let first_ptr = planar[0].as_ptr();
        recycle_planar_storage(&mut pool, planar, 12, 40, 40);
        assert_eq!(pool.len(), 1);

        let reused = take_planar_storage(&mut pool, 12, 40);
        assert_eq!(reused[0].as_ptr(), first_ptr);
        assert!(reused.iter().all(Vec::is_empty));
        assert!(reused.iter().all(|channel| channel.capacity() >= 40));
    }

    #[test]
    fn planar_pool_rejects_oversized_or_malformed_frames() {
        let mut pool = Vec::new();
        recycle_planar_storage(&mut pool, vec![vec![0.0; 80]; 12], 12, 80, 40);
        recycle_planar_storage(&mut pool, vec![vec![0.0; 40]; 11], 12, 40, 40);
        assert!(pool.is_empty());
    }
}
