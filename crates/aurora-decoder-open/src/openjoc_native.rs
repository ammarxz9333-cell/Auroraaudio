//! Native open JOC decode + speaker render adapter using the Apache-2.0
//! `openjoc-api` implementation as the admitted JOC rendering backend.
//!
//! Aurora does not load any Dolby binary or licensed runtime. JOC admission is
//! performed through OpenJOC's own positive complete-access-unit classifier;
//! this renderer is created only after positive admission and Aurora still
//! claims JOC/Atmos playback only after successful OpenJOC speaker rendering.

use std::collections::VecDeque;
use std::time::Duration;

use aurora_core::{AudioBlock, AudioFormat, StandardLayout};
use aurora_decoder_api::{DecodedFrame, DecoderError};
use openjoc_api::{
    OpenJocConfig, OpenJocPacket, OpenJocPcmFrame, OpenJocSession, OpenJocStatus,
    PcmSampleFormat, RenderMode, ValidationProfile,
};
use openjoc_scene::{SpeakerGeometry, SpeakerLayout};

pub const AURORA_ELEVEN_ONE_FOUR_REFERENCE_LAYOUT: &str = "aurora-11.1.4-reference-v1";

const LABELS_2_0: [&str; 2] = ["FL", "FR"];
const LABELS_5_1: [&str; 6] = ["FL", "FR", "FC", "LFE", "Ls", "Rs"];
const LABELS_5_1_2: [&str; 8] = ["FL", "FR", "FC", "LFE", "Ls", "Rs", "TFL", "TFR"];
const LABELS_5_1_4: [&str; 10] = [
    "FL", "FR", "FC", "LFE", "Ls", "Rs", "TFL", "TFR", "TBL", "TBR",
];
const LABELS_7_1: [&str; 8] = ["FL", "FR", "FC", "LFE", "Lb", "Rb", "Ls", "Rs"];
const LABELS_7_1_2: [&str; 10] = [
    "FL", "FR", "FC", "LFE", "Lb", "Rb", "Ls", "Rs", "TFL", "TFR",
];
const LABELS_7_1_4: [&str; 12] = [
    "FL", "FR", "FC", "LFE", "Lb", "Rb", "Ls", "Rs", "TFL", "TFR", "TBL", "TBR",
];
const LABELS_AURORA_11_1_4: [&str; 16] = [
    "FL",
    "FR",
    "FC",
    "LFE",
    "Ls",
    "Rs",
    "Lb",
    "Rb",
    "front-wide-left",
    "front-wide-right",
    "rear-side-left",
    "rear-side-right",
    "TFL",
    "TFR",
    "TBL",
    "TBR",
];

const MAP_2_0: [usize; 2] = [0, 1];
const MAP_5_1: [usize; 6] = [0, 1, 2, 3, 4, 5];
const MAP_5_1_2: [usize; 8] = [0, 1, 2, 3, 4, 5, 6, 7];
const MAP_5_1_4: [usize; 10] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
const MAP_7_1: [usize; 8] = [0, 1, 2, 3, 6, 7, 4, 5];
const MAP_7_1_2: [usize; 10] = [0, 1, 2, 3, 6, 7, 4, 5, 8, 9];
const MAP_7_1_4: [usize; 12] = [0, 1, 2, 3, 6, 7, 4, 5, 8, 9, 10, 11];
const MAX_RECYCLED_PLANAR_BLOCKS: usize = 128;

#[derive(Debug, Clone, Copy)]
struct OpenJocLayoutContract {
    labels: &'static [&'static str],
    aurora_map: &'static [usize],
}

/// Resolves Aurora's typed standard layout identity to the exact admitted
/// OpenJOC preset. Custom layouts intentionally remain outside this fixed
/// contract until Aurora carries explicit geometry end to end.
pub fn openjoc_preset_for_standard_layout(
    layout: StandardLayout,
) -> Result<&'static str, DecoderError> {
    match layout {
        StandardLayout::Stereo => Ok("2.0"),
        StandardLayout::FiveOne => Ok("5.1"),
        StandardLayout::SevenOne => Ok("7.1"),
        StandardLayout::FiveOneTwo => Ok("5.1.2"),
        StandardLayout::FiveOneFour => Ok("5.1.4"),
        StandardLayout::SevenOneTwo => Ok("7.1.2"),
        StandardLayout::SevenOneFour => Ok("7.1.4"),
        StandardLayout::Custom => Err(DecoderError::UnsupportedInput(
            "custom JOC output requires explicit speaker geometry; a channel count or fixed-layout hint is insufficient",
        )),
    }
}

impl crate::OpenDecoderConfig {
    /// Selects a JOC speaker layout through Aurora's typed standard-layout
    /// identity instead of a caller-authored string.
    pub fn with_standard_joc_layout(
        mut self,
        layout: StandardLayout,
    ) -> Result<Self, DecoderError> {
        self.joc_layout_hint = Some(openjoc_preset_for_standard_layout(layout)?);
        Ok(self)
    }

    /// Selects Aurora's explicit sixteen-lane reference geometry. The lane
    /// contract is Aurora-owned and is not presented as a Dolby/ITU/vendor
    /// channel-naming standard.
    #[must_use]
    pub fn with_aurora_eleven_one_four_reference(mut self) -> Self {
        self.joc_layout_hint = Some(AURORA_ELEVEN_ONE_FOUR_REFERENCE_LAYOUT);
        self
    }
}

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
    ready_frames: Vec<OpenJocPcmFrame>,
    recycled_planar: Vec<Vec<Vec<f32>>>,
    emitted_frames: u64,
    discontinuity: bool,
    last_info: JocRenderInfo,
}

impl OpenJocNativeRenderer {
    pub fn new(output: AudioFormat, layout_hint: Option<&str>) -> Result<Self, DecoderError> {
        if layout_hint == Some(AURORA_ELEVEN_ONE_FOUR_REFERENCE_LAYOUT) {
            return Self::new_aurora_eleven_one_four_reference(output);
        }

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
        Self::from_session(session, output, channel_map, info.channel_labels, info.layout_name, info.channel_count, info.latency_samples)
    }

    fn new_aurora_eleven_one_four_reference(output: AudioFormat) -> Result<Self, DecoderError> {
        if output.channel_count != LABELS_AURORA_11_1_4.len() {
            return Err(DecoderError::UnsupportedInput(
                "Aurora 11.1.4 reference layout requires exactly sixteen output channels",
            ));
        }
        let layout = SpeakerLayout::custom(
            AURORA_ELEVEN_ONE_FOUR_REFERENCE_LAYOUT,
            vec![
                SpeakerGeometry::full_range("FL", -30.0, 0.0),
                SpeakerGeometry::full_range("FR", 30.0, 0.0),
                SpeakerGeometry::full_range("FC", 0.0, 0.0),
                SpeakerGeometry::lfe("LFE", 0.0, -30.0),
                SpeakerGeometry::full_range("Ls", -90.0, 0.0),
                SpeakerGeometry::full_range("Rs", 90.0, 0.0),
                SpeakerGeometry::full_range("Lb", -150.0, 0.0),
                SpeakerGeometry::full_range("Rb", 150.0, 0.0),
                SpeakerGeometry::full_range("front-wide-left", -60.0, 0.0),
                SpeakerGeometry::full_range("front-wide-right", 60.0, 0.0),
                SpeakerGeometry::full_range("rear-side-left", -120.0, 0.0),
                SpeakerGeometry::full_range("rear-side-right", 120.0, 0.0),
                SpeakerGeometry::full_range("TFL", -30.0, 45.0),
                SpeakerGeometry::full_range("TFR", 30.0, 45.0),
                SpeakerGeometry::full_range("TBL", -135.0, 45.0),
                SpeakerGeometry::full_range("TBR", 135.0, 45.0),
            ],
        )
        .map_err(|e| {
            DecoderError::ExternalProcess(format!("Aurora 11.1.4 OpenJOC layout failed: {e}"))
        })?;
        let mut config = OpenJocConfig::default().with_speaker_layout(layout);
        config.render_mode = RenderMode::Speaker;
        config.validation_profile = ValidationProfile::Auto;
        let mut session = OpenJocSession::new(config)
            .map_err(|e| DecoderError::ExternalProcess(format!("OpenJOC init failed: {e}")))?;
        session.enable_stage_timing();
        let info = session.output_info();
        if info.sample_format != PcmSampleFormat::F32
            || info.channel_count != output.channel_count
            || info.layout_name != AURORA_ELEVEN_ONE_FOUR_REFERENCE_LAYOUT
            || info
                .channel_labels
                .iter()
                .map(String::as_str)
                .ne(LABELS_AURORA_11_1_4.iter().copied())
        {
            return Err(DecoderError::UnsupportedInput(
                "OpenJOC did not preserve the Aurora 11.1.4 reference semantic contract",
            ));
        }
        if let Some(sample_rate) = info.sample_rate {
            if sample_rate != output.sample_rate {
                return Err(DecoderError::UnsupportedInput(
                    "OpenJOC speaker output sample rate does not match Aurora output format",
                ));
            }
        }
        let channel_map = (0..output.channel_count).collect::<Vec<_>>();
        Self::from_session(session, output, channel_map, info.channel_labels, info.layout_name, info.channel_count, info.latency_samples)
    }

    fn from_session(
        session: OpenJocSession,
        output: AudioFormat,
        channel_map: Vec<usize>,
        expected_channel_labels: Vec<String>,
        layout_name: String,
        channel_count: usize,
        latency_samples: usize,
    ) -> Result<Self, DecoderError> {
        if channel_map.len() != channel_count
            || channel_map.iter().any(|&index| index >= channel_count)
            || {
                let mut sorted = channel_map.clone();
                sorted.sort_unstable();
                sorted.dedup();
                sorted.len() != channel_count
            }
        {
            return Err(DecoderError::UnsupportedInput(
                "OpenJOC-to-Aurora channel map is not a complete permutation",
            ));
        }
        let last_info = JocRenderInfo {
            layout_name,
            channel_count,
            latency_samples,
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

fn openjoc_layout_contract(
    layout: &str,
    channels: usize,
) -> Result<OpenJocLayoutContract, DecoderError> {
    let contract = match (layout, channels) {
        ("2.0", 2) => OpenJocLayoutContract {
            labels: &LABELS_2_0,
            aurora_map: &MAP_2_0,
        },
        ("5.1", 6) => OpenJocLayoutContract {
            labels: &LABELS_5_1,
            aurora_map: &MAP_5_1,
        },
        ("5.1.2", 8) => OpenJocLayoutContract {
            labels: &LABELS_5_1_2,
            aurora_map: &MAP_5_1_2,
        },
        ("5.1.4", 10) => OpenJocLayoutContract {
            labels: &LABELS_5_1_4,
            aurora_map: &MAP_5_1_4,
        },
        ("7.1", 8) => OpenJocLayoutContract {
            labels: &LABELS_7_1,
            aurora_map: &MAP_7_1,
        },
        ("7.1.2", 10) => OpenJocLayoutContract {
            labels: &LABELS_7_1_2,
            aurora_map: &MAP_7_1_2,
        },
        ("7.1.4", 12) => OpenJocLayoutContract {
            labels: &LABELS_7_1_4,
            aurora_map: &MAP_7_1_4,
        },
        _ => {
            return Err(DecoderError::UnsupportedInput(
                "OpenJOC speaker layout has no verified semantic channel-label contract in Aurora",
            ))
        }
    };
    Ok(contract)
}

fn expected_openjoc_channel_labels(
    layout: &str,
    channels: usize,
) -> Result<&'static [&'static str], DecoderError> {
    Ok(openjoc_layout_contract(layout, channels)?.labels)
}

fn aurora_channel_map(layout: &str, channels: usize) -> Result<Vec<usize>, DecoderError> {
    let map = openjoc_layout_contract(layout, channels)?.aurora_map.to_vec();
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
    fn typed_standard_layout_builder_selects_exact_openjoc_presets() {
        let cases = [
            (StandardLayout::Stereo, "2.0"),
            (StandardLayout::FiveOne, "5.1"),
            (StandardLayout::SevenOne, "7.1"),
            (StandardLayout::FiveOneTwo, "5.1.2"),
            (StandardLayout::FiveOneFour, "5.1.4"),
            (StandardLayout::SevenOneTwo, "7.1.2"),
            (StandardLayout::SevenOneFour, "7.1.4"),
        ];
        for (layout, preset) in cases {
            let config = crate::OpenDecoderConfig::default()
                .with_standard_joc_layout(layout)
                .unwrap();
            assert_eq!(config.joc_layout_hint, Some(preset));
        }
    }

    #[test]
    fn aurora_reference_builder_uses_explicit_custom_identity() {
        let config = crate::OpenDecoderConfig::default()
            .with_aurora_eleven_one_four_reference();
        assert_eq!(
            config.joc_layout_hint,
            Some(AURORA_ELEVEN_ONE_FOUR_REFERENCE_LAYOUT)
        );
    }

    #[test]
    fn typed_custom_layout_requires_explicit_geometry_contract() {
        assert!(crate::OpenDecoderConfig::default()
            .with_standard_joc_layout(StandardLayout::Custom)
            .is_err());
    }

    #[test]
    fn aurora_7_1_4_maps_to_openjoc_7_1_4() {
        assert_eq!(default_layout_for_channels(12), Some("7.1.4"));
    }

    #[test]
    fn ambiguous_widths_require_an_explicit_verified_layout() {
        assert_eq!(default_layout_for_channels(8), None);
        assert_eq!(default_layout_for_channels(10), None);
        assert_eq!(default_layout_for_channels(16), None);
    }

    #[test]
    fn fixed_height_layout_contracts_are_explicitly_supported() {
        assert_eq!(
            expected_openjoc_channel_labels("5.1.2", 8).unwrap(),
            &LABELS_5_1_2
        );
        assert_eq!(
            expected_openjoc_channel_labels("5.1.4", 10).unwrap(),
            &LABELS_5_1_4
        );
        assert_eq!(
            expected_openjoc_channel_labels("7.1.2", 10).unwrap(),
            &LABELS_7_1_2
        );
        assert_eq!(aurora_channel_map("5.1.2", 8).unwrap(), MAP_5_1_2);
        assert_eq!(aurora_channel_map("5.1.4", 10).unwrap(), MAP_5_1_4);
    }

    #[test]
    fn openjoc_7_1_4_side_back_order_is_normalized_to_aurora() {
        assert_eq!(aurora_channel_map("7.1.4", 12).unwrap(), MAP_7_1_4);
        assert_eq!(
            expected_openjoc_channel_labels("7.1.4", 12).unwrap(),
            &LABELS_7_1_4
        );
    }

    #[test]
    fn openjoc_7_1_2_side_back_order_is_normalized_to_aurora() {
        assert_eq!(aurora_channel_map("7.1.2", 10).unwrap(), MAP_7_1_2);
    }

    #[test]
    fn openjoc_7_1_side_back_order_is_normalized_to_aurora() {
        assert_eq!(aurora_channel_map("7.1", 8).unwrap(), MAP_7_1);
    }

    #[test]
    fn unverified_same_width_layout_fails_closed() {
        assert!(aurora_channel_map("custom-12", 12).is_err());
        assert!(aurora_channel_map("9.1.2", 12).is_err());
    }

    #[test]
    fn duration_conversion_saturates_into_u64_microseconds() {
        assert_eq!(duration_us(Duration::from_millis(3)), 3_000);
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
