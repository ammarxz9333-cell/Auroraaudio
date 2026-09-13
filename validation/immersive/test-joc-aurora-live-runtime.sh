#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORK_DIR="${AURORA_JOC_TEST_WORKDIR:-}"
BUILD_MODE="${AURORA_JOC_BUILD_MODE:-debug}"
TOOLCHAIN="${AURORA_EXTERNAL_RUST_TOOLCHAIN:-stable}"

if [[ -z "$WORK_DIR" ]]; then
  echo "AURORA_JOC_TEST_WORKDIR must point at the completed test-joc-stack workdir" >&2
  exit 2
fi
case "$BUILD_MODE" in
  debug) PROFILE_ARGS=(); PROFILE_DIR="debug" ;;
  release) PROFILE_ARGS=(--release); PROFILE_DIR="release" ;;
  *) echo "unsupported AURORA_JOC_BUILD_MODE: $BUILD_MODE" >&2; exit 2 ;;
esac

HARLETTY_DIR="$WORK_DIR/harletty-bridge"
OMNIP_DIR="$WORK_DIR/Omniphony"
HARLETTY_TARGET_DIR="${AURORA_HARLETTY_TARGET_DIR:-$HARLETTY_DIR/target}"
HARNESS_TARGET_DIR="${AURORA_JOC_HARNESS_TARGET_DIR:-$WORK_DIR/iec-joc-harness-target}"
BRIDGE_LIB="$HARLETTY_TARGET_DIR/$PROFILE_DIR/libharletty_bridge.so"
JOC_IEC="$WORK_DIR/joc_atmos_1s.spdif"
PLAIN_IEC="$WORK_DIR/plain_eac3_5_1.spdif"

for path in "$BRIDGE_LIB" "$JOC_IEC" "$PLAIN_IEC" "$OMNIP_DIR/omniphony-renderer/bridge_api/Cargo.toml"; do
  [[ -f "$path" ]] || { echo "missing baseline JOC artifact: $path" >&2; exit 1; }
done

HARNESS_DIR="$WORK_DIR/aurora-live-runtime-harness"
rm -rf "$HARNESS_DIR"
mkdir -p "$HARNESS_DIR/src"

cat > "$HARNESS_DIR/Cargo.toml" <<EOF_CARGO
[package]
name = "aurora-live-joc-runtime-harness"
version = "0.1.0"
edition = "2021"
publish = false

[dependencies]
abi_stable = "0.11"
bridge_api = { path = "$OMNIP_DIR/omniphony-renderer/bridge_api" }
spdif = { path = "$OMNIP_DIR/omniphony-renderer/spdif" }
aurora-core = { path = "$ROOT_DIR/crates/aurora-core" }
aurora-decoder-api = { path = "$ROOT_DIR/crates/aurora-decoder-api" }
aurora-renderer-basic = { path = "$ROOT_DIR/crates/aurora-renderer-basic" }
aurora-source-runtime = { path = "$ROOT_DIR/crates/aurora-source-runtime" }
EOF_CARGO

cat > "$HARNESS_DIR/src/main.rs" <<'EOF_RS'
use abi_stable::library::RootModule;
use aurora_core::{
    AudioBlock, AudioFormat, AudioObject, ChannelRole, Listener, SampleType, Speaker, Vector3,
};
use aurora_decoder_api::{
    DecodedBatch, DecodedChannelKind, DecodedFrame, DecoderError, DecoderInfo,
    DecoderOutputSemantics, DecoderPacket, DecoderPacketTransport, ObjectChannelBinding,
    StreamingDecodedFrame, StreamingDecoder, StreamingDecoderConfig,
};
use aurora_renderer_basic::{BasicRenderer, BasicRendererMode};
use aurora_source_runtime::live::{
    LiveDecodePolicy, LiveDecodeState, LiveImmersiveRuntime, IEC61937_EAC3_DATA_TYPE,
};
use bridge_api::{
    BridgeLibRef, FormatBridgeBox, RChannelLabel, RCoordinateFormat, RInputTransport,
};
use spdif::SpdifParser;
use std::collections::BTreeMap;
use std::{env, fs, path::Path};

const PCM_SCALE: f32 = 8_388_608.0;

#[derive(Clone, Copy)]
struct ObjectState {
    position: Vector3,
    gain_db: f32,
    spread: f32,
}

struct HarlettyStreamingAdapter {
    bridge: FormatBridgeBox,
    coordinate_format: RCoordinateFormat,
    config: Option<StreamingDecoderConfig>,
    objects: BTreeMap<u32, ObjectState>,
    object_channels: BTreeMap<u32, usize>,
    fixed_channel_gains: BTreeMap<usize, f32>,
    presentation_samples: u64,
}

impl HarlettyStreamingAdapter {
    fn load(path: &Path) -> Self {
        let lib = BridgeLibRef::load_from_file(path).expect("load pinned Harletty bridge");
        let bridge = (lib.new_bridge())(true);
        let coordinate_format = bridge.coordinate_format();
        Self {
            bridge,
            coordinate_format,
            config: None,
            objects: BTreeMap::new(),
            object_channels: BTreeMap::new(),
            fixed_channel_gains: BTreeMap::new(),
            presentation_samples: 0,
        }
    }

    fn clear_semantics(&mut self) {
        self.objects.clear();
        self.object_channels.clear();
        self.fixed_channel_gains.clear();
        self.presentation_samples = 0;
    }

    fn update_metadata(&mut self, frame: &bridge_api::RDecodedFrame) -> Result<(), DecoderError> {
        for metadata in frame.metadata.iter() {
            for binding in metadata.object_channels.iter() {
                let channel = binding.channel as usize;
                self.object_channels
                    .retain(|id, existing| *id == binding.id || *existing != channel);
                self.object_channels.insert(binding.id, channel);
            }
            for gain in metadata.channel_gains.iter() {
                self.fixed_channel_gains
                    .insert(gain.channel as usize, db_to_linear(gain.gain_db as f32));
            }
            for event in metadata.events.iter() {
                let mut state = self.objects.get(&event.id).copied().unwrap_or(ObjectState {
                    position: Vector3::ZERO,
                    gain_db: event.gain_db as f32,
                    spread: 0.0,
                });
                if event.has_pos {
                    state.position = event_position(self.coordinate_format, event.pos)?;
                } else if !self.objects.contains_key(&event.id) {
                    continue;
                }
                state.gain_db = event.gain_db as f32;
                state.spread = event
                    .size
                    .iter()
                    .copied()
                    .fold(0.0_f64, f64::max)
                    .clamp(0.0, 1.0) as f32;
                self.objects.insert(event.id, state);
            }
        }
        Ok(())
    }

    fn convert_frame(
        &mut self,
        frame: &bridge_api::RDecodedFrame,
        discontinuity: bool,
        has_objects: bool,
    ) -> Result<Option<StreamingDecodedFrame>, DecoderError> {
        let config = self
            .config
            .ok_or(DecoderError::UnsupportedInput("streaming adapter not configured"))?;
        let channels = frame.channel_count as usize;
        let sample_count = frame.sample_count as usize;
        if frame.sampling_frequency != config.sample_rate
            || channels == 0
            || channels > config.maximum_pcm_channels
            || sample_count == 0
            || frame.channel_labels.len() != channels
            || frame.pcm.len() != channels.saturating_mul(sample_count)
        {
            return Err(DecoderError::UnsupportedInput(
                "Harletty decoded frame violates configured PCM shape",
            ));
        }
        if !frame.drc_gain.is_finite() || frame.drc_gain < 0.0 {
            return Err(DecoderError::UnsupportedInput("invalid Harletty DRC gain"));
        }

        self.update_metadata(frame)?;
        let channel_kinds = frame
            .channel_labels
            .iter()
            .copied()
            .map(channel_kind)
            .collect::<Vec<_>>();

        let mut bindings = Vec::new();
        let mut objects = Vec::new();
        for channel_index in 0..channels {
            if channel_kinds[channel_index] != DecodedChannelKind::Object {
                continue;
            }
            let Some((&object_id, _)) = self
                .object_channels
                .iter()
                .find(|(_, bound_channel)| **bound_channel == channel_index)
            else {
                return Ok(None);
            };
            let Some(state) = self.objects.get(&object_id).copied() else {
                return Ok(None);
            };
            let id = format!("harletty-object-{object_id}");
            bindings.push(ObjectChannelBinding {
                object_id: id.clone(),
                channel_index,
            });
            objects.push(AudioObject {
                id,
                position: state.position,
                velocity: Vector3::ZERO,
                gain_db: state.gain_db,
                spread: state.spread,
                start_time_seconds: None,
                end_time_seconds: None,
            });
        }
        if has_objects && objects.is_empty() {
            return Ok(None);
        }

        let mut planar = vec![vec![0.0_f32; sample_count]; channels];
        for sample_index in 0..sample_count {
            for channel_index in 0..channels {
                let raw = frame.pcm[sample_index * channels + channel_index];
                let fixed_gain = self
                    .fixed_channel_gains
                    .get(&channel_index)
                    .copied()
                    .unwrap_or(1.0);
                planar[channel_index][sample_index] =
                    raw as f32 / PCM_SCALE * frame.drc_gain * fixed_gain;
            }
        }
        if planar
            .iter()
            .flatten()
            .any(|sample| !sample.is_finite())
        {
            return Err(DecoderError::UnsupportedInput(
                "Harletty PCM conversion produced non-finite output",
            ));
        }

        let presentation_time_seconds =
            self.presentation_samples as f64 / f64::from(frame.sampling_frequency);
        self.presentation_samples = self
            .presentation_samples
            .saturating_add(frame.sample_count as u64);
        Ok(Some(StreamingDecodedFrame {
            decoded: DecodedFrame {
                audio: AudioBlock {
                    channels: planar,
                    frame_count: sample_count,
                    presentation_time_seconds,
                    discontinuity: discontinuity || frame.is_new_segment,
                },
                objects,
            },
            channel_kinds,
            object_channels: bindings,
        }))
    }
}

impl StreamingDecoder for HarlettyStreamingAdapter {
    fn info(&self) -> DecoderInfo {
        DecoderInfo {
            name: "harletty-v0.7.4-validation-adapter",
            production_ready: false,
            maturity: "validation",
            output_semantics: DecoderOutputSemantics::ObjectScene,
        }
    }

    fn configure_stream(&mut self, config: StreamingDecoderConfig) -> Result<(), DecoderError> {
        if config.sample_rate == 0 || config.block_size == 0 || config.maximum_pcm_channels == 0 {
            return Err(DecoderError::UnsupportedInput("invalid Aurora streaming config"));
        }
        self.config = Some(config);
        Ok(())
    }

    fn push_packet(&mut self, packet: DecoderPacket<'_>) -> Result<DecodedBatch, DecoderError> {
        if packet.discontinuity {
            self.bridge.reset();
            self.clear_semantics();
        }
        let transport = match packet.transport {
            DecoderPacketTransport::RawElementary => RInputTransport::Raw,
            DecoderPacketTransport::Iec61937 => RInputTransport::Iec61937,
        };
        let data_type = packet.data_type.unwrap_or(0);
        let result = self.bridge.push_packet(packet.payload.into(), transport, data_type);
        if !result.error_message.is_empty() {
            return Err(DecoderError::ExternalProcess(
                result.error_message.as_str().to_owned(),
            ));
        }
        if result.did_reset {
            self.clear_semantics();
        }
        let has_objects = self.bridge.has_objects();
        let discontinuity = packet.discontinuity || result.did_reset;
        let mut frames = Vec::new();
        for frame in result.frames.iter() {
            if let Some(converted) = self.convert_frame(frame, discontinuity, has_objects)? {
                frames.push(converted);
            }
        }
        Ok(DecodedBatch {
            frames,
            native_objects_present: has_objects,
        })
    }

    fn reset_stream(&mut self) {
        self.bridge.reset();
        self.clear_semantics();
    }
}

fn event_position(
    format: RCoordinateFormat,
    pos: [f64; 3],
) -> Result<Vector3, DecoderError> {
    if pos.iter().any(|value| !value.is_finite()) {
        return Err(DecoderError::UnsupportedInput("non-finite object position"));
    }
    let vector = match format {
        RCoordinateFormat::Cartesian => Vector3::new(pos[0] as f32, pos[1] as f32, pos[2] as f32),
        RCoordinateFormat::Polar => {
            let azimuth = pos[0].to_radians();
            let elevation = pos[1].to_radians();
            let distance = pos[2].max(0.0);
            let horizontal = elevation.cos() * distance;
            Vector3::new(
                (azimuth.sin() * horizontal) as f32,
                (azimuth.cos() * horizontal) as f32,
                (elevation.sin() * distance) as f32,
            )
        }
    };
    Ok(vector)
}

fn channel_kind(label: RChannelLabel) -> DecodedChannelKind {
    let role = match label {
        RChannelLabel::L => ChannelRole::FrontLeft,
        RChannelLabel::R => ChannelRole::FrontRight,
        RChannelLabel::C => ChannelRole::FrontCenter,
        RChannelLabel::LFE => ChannelRole::LowFrequencyEffects,
        RChannelLabel::Ls => ChannelRole::SurroundLeft,
        RChannelLabel::Rs => ChannelRole::SurroundRight,
        RChannelLabel::Lb => ChannelRole::SurroundBackLeft,
        RChannelLabel::Rb => ChannelRole::SurroundBackRight,
        RChannelLabel::Tfl => ChannelRole::TopFrontLeft,
        RChannelLabel::Tfr => ChannelRole::TopFrontRight,
        RChannelLabel::Tbl => ChannelRole::TopRearLeft,
        RChannelLabel::Tbr => ChannelRole::TopRearRight,
        RChannelLabel::Object => return DecodedChannelKind::Object,
        _ => return DecodedChannelKind::Unknown,
    };
    DecodedChannelKind::Bed(role)
}

fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

fn speaker(id: &str, role: ChannelRole, x: f32, y: f32, z: f32) -> Speaker {
    Speaker {
        id: id.to_owned(),
        label: id.to_owned(),
        channel_role: role,
        position: Vector3::new(x, y, z),
        orientation: Vector3::new(0.0, 0.0, 0.0),
        gain_db: 0.0,
        delay_samples: 0.0,
        enabled: true,
    }
}

fn layout_7_1_4() -> Vec<Speaker> {
    vec![
        speaker("fl", ChannelRole::FrontLeft, -1.0, 1.0, 0.0),
        speaker("fr", ChannelRole::FrontRight, 1.0, 1.0, 0.0),
        speaker("fc", ChannelRole::FrontCenter, 0.0, 1.2, 0.0),
        speaker("lfe", ChannelRole::LowFrequencyEffects, 0.0, -10.0, -1.0),
        speaker("sl", ChannelRole::SurroundLeft, -1.0, 0.0, 0.0),
        speaker("sr", ChannelRole::SurroundRight, 1.0, 0.0, 0.0),
        speaker("sbl", ChannelRole::SurroundBackLeft, -1.0, -1.0, 0.0),
        speaker("sbr", ChannelRole::SurroundBackRight, 1.0, -1.0, 0.0),
        speaker("tfl", ChannelRole::TopFrontLeft, -1.0, 1.0, 1.0),
        speaker("tfr", ChannelRole::TopFrontRight, 1.0, 1.0, 1.0),
        speaker("trl", ChannelRole::TopRearLeft, -1.0, -1.0, 1.0),
        speaker("trr", ChannelRole::TopRearRight, 1.0, -1.0, 1.0),
    ]
}

fn run(bridge_path: &Path, carrier_path: &Path, expect_objects: bool) {
    let decoder = HarlettyStreamingAdapter::load(bridge_path);
    let mut runtime = LiveImmersiveRuntime::new(
        decoder,
        BasicRenderer::new(BasicRendererMode::InverseDistance),
        AudioFormat {
            sample_rate: 48_000,
            channel_count: 12,
            sample_type: SampleType::F32,
            block_size: 256,
        },
        Listener {
            position: Vector3::ZERO,
            orientation: Vector3::new(0.0, 1.0, 0.0),
            ear_height: 0.0,
        },
        layout_7_1_4(),
        LiveDecodePolicy {
            maximum_pcm_channels: 32,
            maximum_objects: 32,
            maximum_object_probe_packets: 8,
            ..LiveDecodePolicy::default()
        },
    )
    .expect("construct Aurora live immersive runtime");

    let carrier = fs::read(carrier_path).expect("read IEC61937 carrier");
    let mut parser = SpdifParser::new();
    let mut packets = 0usize;
    let mut audible_frames = 0usize;
    let mut speaker_frames = 0usize;
    let mut peak = 0.0_f32;
    let mut faulted = false;

    'chunks: for chunk in carrier.chunks(997) {
        parser.push_bytes(chunk);
        while let Some(packet) = parser.get_next_packet() {
            packets += 1;
            assert_eq!(packet.data_type, IEC61937_EAC3_DATA_TYPE);
            let result = runtime.push_packet(DecoderPacket {
                transport: DecoderPacketTransport::Iec61937,
                data_type: Some(packet.data_type),
                payload: packet.payload.as_slice(),
                discontinuity: false,
            });
            match result {
                Ok(report) => {
                    if report.audible {
                        audible_frames += report.frames.len();
                        for frame in report.frames {
                            speaker_frames += frame.speaker_audio.frame_count;
                            for sample in frame.speaker_audio.channels.iter().flatten() {
                                peak = peak.max(sample.abs());
                            }
                        }
                    }
                }
                Err(error) if !expect_objects && runtime.state() == LiveDecodeState::Faulted => {
                    eprintln!("plain E-AC-3 fail-closed after {packets} packets: {error}");
                    faulted = true;
                    break 'chunks;
                }
                Err(error) => panic!("Aurora live runtime error after packet {packets}: {error}"),
            }
        }
    }

    assert!(packets > 0, "no IEC61937 packets reached Aurora runtime");
    if expect_objects {
        assert_eq!(runtime.state(), LiveDecodeState::Running);
        assert!(audible_frames > 0, "real JOC never crossed Aurora stable-acquisition gate");
        assert!(speaker_frames > 0, "Aurora emitted no speaker frames");
        assert!(peak.is_finite() && peak > 1.0e-8, "Aurora speaker PCM is silent/non-finite");
        assert!(runtime.metrics().maximum_observed_objects > 0);
        assert!(runtime.metrics().maximum_observed_pcm_channels > 0);
        println!(
            "AURORA-HARLETTY-LIVE-RUNTIME-PASS packets={packets} audible_frames={audible_frames} speaker_frames={speaker_frames} peak={peak:.6} max_objects={} max_pcm_channels={}",
            runtime.metrics().maximum_observed_objects,
            runtime.metrics().maximum_observed_pcm_channels,
        );
    } else {
        assert_eq!(audible_frames, 0, "plain E-AC-3 was released as native object audio");
        assert!(
            faulted || runtime.state() != LiveDecodeState::Running,
            "plain E-AC-3 incorrectly reached native-object Running state"
        );
        println!(
            "AURORA-HARLETTY-PLAIN-EAC3-FAIL-CLOSED-PASS packets={packets} state={:?}",
            runtime.state()
        );
    }
}

fn main() {
    let mut args = env::args().skip(1);
    let bridge = args.next().expect("bridge path");
    let carrier = args.next().expect("carrier path");
    let mode = args.next().expect("objects|plain");
    assert!(args.next().is_none(), "unexpected extra arguments");
    match mode.as_str() {
        "objects" => run(Path::new(&bridge), Path::new(&carrier), true),
        "plain" => run(Path::new(&bridge), Path::new(&carrier), false),
        _ => panic!("mode must be objects or plain"),
    }
}
EOF_RS

printf '\n== Aurora live runtime: real Harletty JOC adapter ==\n'
CARGO_TARGET_DIR="$HARNESS_TARGET_DIR" cargo +"$TOOLCHAIN" run --quiet "${PROFILE_ARGS[@]}" \
  --manifest-path "$HARNESS_DIR/Cargo.toml" -- "$BRIDGE_LIB" "$JOC_IEC" objects

printf '\n== Aurora live runtime: plain E-AC-3 fail-closed control ==\n'
CARGO_TARGET_DIR="$HARNESS_TARGET_DIR" cargo +"$TOOLCHAIN" run --quiet "${PROFILE_ARGS[@]}" \
  --manifest-path "$HARNESS_DIR/Cargo.toml" -- "$BRIDGE_LIB" "$PLAIN_IEC" plain

echo "AURORA HARLETTY LIVE RUNTIME CONTRACT PASS"
