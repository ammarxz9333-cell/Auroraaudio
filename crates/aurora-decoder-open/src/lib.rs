//! Aurora universal open audio decoder fabric.
//!
//! Design goals:
//! - no proprietary codec DLLs/blobs or paid runtime licences;
//! - byte-level auto detection for raw/eARC input;
//! - independent open implementations for E-AC-3/JOC admission and rendering;
//! - one Aurora-owned PCM/object boundary for every backend;
//! - backend replacement without changing the realtime/DSP/output engine.

pub mod framing;
pub mod iec61937;
pub mod joc_access_unit;
pub mod joc_probe;
pub mod native_ac3;
pub mod openjoc_native;
pub mod sniff;
pub mod worker;

use std::collections::VecDeque;

use aurora_core::AudioFormat;
use aurora_decoder_api::{DecodedFrame, Decoder, DecoderError, DecoderInfo};
use framing::SyncFramer;
use iec61937::Iec61937Depacketizer;
use joc_access_unit::JocAccessUnitAssembler;
use joc_probe::{JocAdmission, JocAdmissionProbe};
use native_ac3::{JocPresentation, NativeAc3Decoder};
use openjoc_native::{JocRenderInfo, OpenJocNativeRenderer};
use sniff::{probe, CodecKind, Encapsulation};
use worker::OpenWorkerDecoder;

pub use sniff::{CodecKind as OpenCodecKind, Encapsulation as OpenEncapsulation, ProbeResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendClass {
    /// Rust-native permissive backend integrated in-process.
    NativeOpen,
    /// Open-source external worker (FFmpeg/libavcodec class).
    OpenWorker,
    /// Raw PCM can bypass codec decode when its exact format is declared.
    Passthrough,
    /// No admitted open backend is wired yet.
    Unavailable,
}

/// Route table kept separate from byte probing. This is the architectural
/// contract for broad codec support; adding a backend never changes callers.
pub const fn backend_class(codec: CodecKind) -> BackendClass {
    match codec {
        CodecKind::Ac3 | CodecKind::Eac3 | CodecKind::Eac3Joc => BackendClass::NativeOpen,
        CodecKind::Pcm
        | CodecKind::TrueHd
        | CodecKind::Mlp
        | CodecKind::DolbyMat
        | CodecKind::Dts
        | CodecKind::DtsHd
        | CodecKind::AacAdts
        | CodecKind::AacLatm
        | CodecKind::Flac
        | CodecKind::Opus
        | CodecKind::Vorbis
        | CodecKind::Speex
        | CodecKind::Mp3
        | CodecKind::Alac
        | CodecKind::WavPack
        | CodecKind::MonkeyAudio
        | CodecKind::Tta
        | CodecKind::Musepack
        | CodecKind::AmrNb
        | CodecKind::AmrWb
        | CodecKind::Sbc
        | CodecKind::OggUnknown => BackendClass::OpenWorker,
        CodecKind::Unknown => BackendClass::Unavailable,
    }
}

/// Universal open decoder configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenDecoderConfig {
    /// Optional caller-supplied codec hint. `None` enables byte probing.
    /// The transport is still probed, so a hinted E-AC-3 stream arriving in
    /// IEC 61937 is depacketized correctly rather than treated as raw E-AC-3.
    pub codec_hint: Option<CodecKind>,
    /// If true, 2-channel JOC validation can additionally use OxideAV's open
    /// stereo reference renderer. Normal immersive playback keeps this false.
    pub joc_stereo_reference: bool,
    /// Optional OpenJOC preset such as `7.1.4`, `9.1.6` or `22.2`. `None`
    /// derives an unambiguous standard preset from the Aurora channel count.
    pub joc_layout_hint: Option<&'static str>,
}

impl Default for OpenDecoderConfig {
    fn default() -> Self {
        Self {
            codec_hint: None,
            joc_stereo_reference: false,
            joc_layout_hint: None,
        }
    }
}

enum Transport {
    Undecided,
    Elementary,
    Iec61937(Iec61937Depacketizer),
}

/// Decoder front door used by Aurora. One instance owns detection, transport
/// depacketizing, codec framing, backend state and decoded-frame queuing.
pub struct UniversalOpenDecoder {
    config: OpenDecoderConfig,
    output_format: Option<AudioFormat>,
    codec: Option<CodecKind>,
    encapsulation: Encapsulation,
    transport: Transport,
    framer: Option<SyncFramer>,
    native: Option<NativeAc3Decoder>,
    worker: Option<OpenWorkerDecoder>,
    joc_assembler: Option<JocAccessUnitAssembler>,
    joc_probe: JocAdmissionProbe,
    joc_renderer: Option<OpenJocNativeRenderer>,
    last_joc_error: Option<String>,
    pending: VecDeque<DecodedFrame>,
}

impl UniversalOpenDecoder {
    pub fn new(config: OpenDecoderConfig) -> Self {
        Self {
            codec: config.codec_hint,
            config,
            output_format: None,
            encapsulation: Encapsulation::Unknown,
            transport: Transport::Undecided,
            framer: None,
            native: None,
            worker: None,
            joc_assembler: None,
            joc_probe: JocAdmissionProbe::new(),
            joc_renderer: None,
            last_joc_error: None,
            pending: VecDeque::new(),
        }
    }

    pub fn detected_codec(&self) -> Option<CodecKind> {
        self.codec
    }

    pub fn detected_encapsulation(&self) -> Encapsulation {
        self.encapsulation
    }

    /// Diagnostics from the admitted immersive renderer, if JOC has actually
    /// passed validation and produced an OpenJOC session.
    pub fn joc_render_info(&self) -> Option<&JocRenderInfo> {
        self.joc_renderer
            .as_ref()
            .map(OpenJocNativeRenderer::render_info)
    }

    /// Most recent JOC admission/render failure. A non-empty value means Aurora
    /// deliberately fell back to the ordinary E-AC-3 bed instead of claiming
    /// Atmos/JOC output.
    pub fn last_joc_error(&self) -> Option<&str> {
        self.last_joc_error.as_deref()
    }

    fn ensure_codec(&mut self, data: &[u8]) -> Result<(), DecoderError> {
        if self.codec.is_some() && !matches!(self.transport, Transport::Undecided) {
            return Ok(());
        }

        let probed = probe(data);
        let codec = self.config.codec_hint.unwrap_or(probed.codec);
        if codec == CodecKind::Unknown {
            return Err(DecoderError::UnsupportedInput(
                "unable to identify compressed audio codec from input prefix",
            ));
        }
        let encapsulation = if probed.encapsulation == Encapsulation::Iec61937 {
            Encapsulation::Iec61937
        } else if probed.encapsulation != Encapsulation::Unknown {
            probed.encapsulation
        } else {
            Encapsulation::Elementary
        };
        self.codec = Some(codec);
        self.encapsulation = encapsulation;
        self.transport = if encapsulation == Encapsulation::Iec61937 {
            Transport::Iec61937(Iec61937Depacketizer::new())
        } else {
            Transport::Elementary
        };
        // An IEC burst is depacketized before the codec backend sees it.
        let backend_encapsulation = if encapsulation == Encapsulation::Iec61937 {
            Encapsulation::Elementary
        } else {
            encapsulation
        };
        self.initialize_backend(codec, backend_encapsulation)
    }

    fn initialize_backend(
        &mut self,
        codec: CodecKind,
        encapsulation: Encapsulation,
    ) -> Result<(), DecoderError> {
        self.native = None;
        self.worker = None;
        self.framer = None;
        self.joc_assembler = None;
        self.joc_renderer = None;
        self.joc_probe.reset();
        match backend_class(codec) {
            BackendClass::NativeOpen => {
                let joc = if self.config.joc_stereo_reference {
                    JocPresentation::StereoReference
                } else {
                    JocPresentation::Bed
                };
                let mut decoder = NativeAc3Decoder::new(codec, joc)?;
                if let Some(format) = self.output_format {
                    decoder.configure(format)?;
                }
                self.native = Some(decoder);
                if codec == CodecKind::Ac3 {
                    self.framer = Some(SyncFramer::new(CodecKind::Ac3));
                } else {
                    self.joc_assembler = Some(JocAccessUnitAssembler::new());
                }
                Ok(())
            }
            BackendClass::OpenWorker => {
                let output = self
                    .output_format
                    .ok_or(DecoderError::Unavailable("decoder is not configured"))?;
                self.worker = Some(OpenWorkerDecoder::spawn(codec, encapsulation, output)?);
                Ok(())
            }
            BackendClass::Passthrough => Err(DecoderError::Unavailable(
                "raw PCM passthrough requires an explicit sample-format adapter",
            )),
            BackendClass::Unavailable => Err(DecoderError::UnsupportedInput(
                "no admitted open decoder backend for this codec",
            )),
        }
    }

    fn decode_native_packet(&mut self, packet: &[u8]) -> Result<(), DecoderError> {
        let native = self
            .native
            .as_mut()
            .ok_or(DecoderError::Unavailable("native backend is not initialized"))?;
        if let Some(frame) = native.decode_chunk(packet)? {
            self.pending.push_back(frame);
        }
        Ok(())
    }

    fn process_worker(&mut self, bytes: &[u8]) -> Result<(), DecoderError> {
        let worker = self
            .worker
            .as_mut()
            .ok_or(DecoderError::Unavailable(
                "open worker backend is not initialized",
            ))?;
        if let Some(frame) = worker.push(bytes)? {
            self.pending.push_back(frame);
        }
        while let Some(frame) = worker.poll()? {
            self.pending.push_back(frame);
        }
        Ok(())
    }

    fn process_eac3_stream(&mut self, bytes: &[u8]) -> Result<(), DecoderError> {
        let units = self
            .joc_assembler
            .as_mut()
            .ok_or(DecoderError::Unavailable(
                "E-AC-3 access-unit assembler missing",
            ))?
            .push(bytes)?;
        for unit in units {
            self.process_eac3_access_unit(&unit)?;
        }
        Ok(())
    }

    fn process_eac3_access_unit(&mut self, unit: &[u8]) -> Result<(), DecoderError> {
        match self.joc_probe.inspect(unit) {
            JocAdmission::Validated => {
                let output = self
                    .output_format
                    .ok_or(DecoderError::Unavailable("decoder is not configured"))?;
                if self.joc_renderer.is_none() {
                    match OpenJocNativeRenderer::new(output, self.config.joc_layout_hint) {
                        Ok(renderer) => self.joc_renderer = Some(renderer),
                        Err(error) => {
                            self.last_joc_error = Some(error.to_string());
                            self.codec = Some(CodecKind::Eac3);
                            return self.decode_eac3_bed_access_unit(unit);
                        }
                    }
                }
                let render = self
                    .joc_renderer
                    .as_mut()
                    .expect("renderer initialized above")
                    .push_access_unit(unit);
                match render {
                    Ok(()) => {
                        self.codec = Some(CodecKind::Eac3Joc);
                        self.last_joc_error = None;
                        let renderer = self.joc_renderer.as_mut().expect("renderer exists");
                        while let Some(frame) = renderer.take_block() {
                            self.pending.push_back(frame);
                        }
                        Ok(())
                    }
                    Err(error) => {
                        self.last_joc_error = Some(error.to_string());
                        self.joc_renderer = None;
                        self.codec = Some(CodecKind::Eac3);
                        self.decode_eac3_bed_access_unit(unit)
                    }
                }
            }
            JocAdmission::SignalledButInvalid => {
                self.last_joc_error = Some(
                    "EC-3 Extension Type A was present but EMDF/OAMD/JOC validation failed"
                        .to_owned(),
                );
                self.joc_renderer = None;
                self.codec = Some(CodecKind::Eac3);
                self.decode_eac3_bed_access_unit(unit)
            }
            JocAdmission::NotJoc => {
                // Ordinary E-AC-3 starts a non-JOC state immediately. Do not
                // leave an earlier malformed-JOC fallback reason or renderer
                // attached to the current stream classification.
                self.last_joc_error = None;
                self.joc_renderer = None;
                self.codec = Some(CodecKind::Eac3);
                self.decode_eac3_bed_access_unit(unit)
            }
        }
    }

    fn decode_eac3_bed_access_unit(&mut self, unit: &[u8]) -> Result<(), DecoderError> {
        // OxideAV consumes an independent frame plus its dependents. Split a
        // six-block AU into those programme sets without changing the live
        // Aurora decoder state between sets.
        let mut framer = SyncFramer::new(CodecKind::Eac3);
        let mut packets = framer.push(unit);
        packets.extend(framer.flush());
        if packets.is_empty() {
            return Err(DecoderError::UnsupportedInput(
                "E-AC-3 access unit contained no decodable programme set",
            ));
        }
        for packet in packets {
            self.decode_native_packet(&packet)?;
        }
        Ok(())
    }

    fn process_elementary(&mut self, bytes: &[u8]) -> Result<(), DecoderError> {
        let codec = self.codec.expect("ensure_codec sets codec");
        match codec {
            CodecKind::Ac3 => {
                let packets = self
                    .framer
                    .as_mut()
                    .ok_or(DecoderError::Unavailable("AC-3 sync framer missing"))?
                    .push(bytes);
                for packet in packets {
                    self.decode_native_packet(&packet)?;
                }
                Ok(())
            }
            CodecKind::Eac3 | CodecKind::Eac3Joc => self.process_eac3_stream(bytes),
            _ => match backend_class(codec) {
                BackendClass::OpenWorker => self.process_worker(bytes),
                BackendClass::Passthrough => Err(DecoderError::Unavailable(
                    "raw PCM passthrough adapter is not initialized",
                )),
                BackendClass::NativeOpen | BackendClass::Unavailable => Err(
                    DecoderError::UnsupportedInput("unsupported elementary audio path"),
                ),
            },
        }
    }

    fn process_iec61937(&mut self, bytes: &[u8]) -> Result<(), DecoderError> {
        let bursts = match &mut self.transport {
            Transport::Iec61937(depacketizer) => depacketizer.push(bytes),
            _ => return Err(DecoderError::UnsupportedInput("input is not IEC61937")),
        };
        for burst in bursts {
            if burst.codec == CodecKind::Unknown {
                continue;
            }
            if !same_codec_family(self.codec, burst.codec) {
                // The carrier can switch format at runtime. Rebuild exactly one
                // codec backend while preserving the single final PCM owner.
                self.codec = Some(burst.codec);
                self.initialize_backend(burst.codec, Encapsulation::Elementary)?;
            }
            match burst.codec {
                CodecKind::Eac3 | CodecKind::Eac3Joc => {
                    self.process_eac3_stream(&burst.payload)?;
                }
                CodecKind::Ac3 => self.decode_native_packet(&burst.payload)?,
                codec if backend_class(codec) == BackendClass::OpenWorker => {
                    self.process_worker(&burst.payload)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Flush complete codec packets retained by the streaming frontends and
    /// flush delayed samples from the native JOC/open-worker renderers.
    pub fn flush_packets(&mut self) -> Result<(), DecoderError> {
        if let Some(framer) = self.framer.as_mut() {
            let packets = framer.flush();
            for packet in packets {
                self.decode_native_packet(&packet)?;
            }
        }
        if let Some(assembler) = self.joc_assembler.as_mut() {
            let units = assembler.finish()?;
            for unit in units {
                self.process_eac3_access_unit(&unit)?;
            }
        }
        if let Some(renderer) = self.joc_renderer.as_mut() {
            for frame in renderer.drain()? {
                self.pending.push_back(frame);
            }
        }
        if let Some(worker) = self.worker.as_mut() {
            for frame in worker.finish()? {
                self.pending.push_back(frame);
            }
        }
        Ok(())
    }
}

impl Decoder for UniversalOpenDecoder {
    fn info(&self) -> DecoderInfo {
        DecoderInfo {
            name: "Aurora universal open decoder fabric",
            production_ready: false,
            maturity: "dual-open-joc-plus-open-worker",
        }
    }

    fn configure(&mut self, output_format: AudioFormat) -> Result<(), DecoderError> {
        self.output_format = Some(output_format);
        if let Some(native) = self.native.as_mut() {
            native.configure(output_format)?;
        }
        Ok(())
    }

    fn decode_chunk(&mut self, input: &[u8]) -> Result<Option<DecodedFrame>, DecoderError> {
        if let Some(frame) = self.pending.pop_front() {
            return Ok(Some(frame));
        }
        if input.is_empty() {
            if let Some(renderer) = self.joc_renderer.as_mut() {
                if let Some(frame) = renderer.take_block() {
                    return Ok(Some(frame));
                }
            }
            if let Some(worker) = self.worker.as_mut() {
                if let Some(frame) = worker.poll()? {
                    return Ok(Some(frame));
                }
            }
            return Ok(None);
        }
        if self.output_format.is_none() {
            return Err(DecoderError::Unavailable("decoder is not configured"));
        }
        self.ensure_codec(input)?;
        match self.transport {
            Transport::Iec61937(_) => self.process_iec61937(input)?,
            Transport::Elementary => self.process_elementary(input)?,
            Transport::Undecided => unreachable!("ensure_codec decides transport"),
        }
        Ok(self.pending.pop_front())
    }

    fn reset(&mut self) {
        self.codec = self.config.codec_hint;
        self.encapsulation = Encapsulation::Unknown;
        self.transport = Transport::Undecided;
        self.framer = None;
        self.native = None;
        self.worker = None;
        self.joc_assembler = None;
        self.joc_probe.reset();
        self.joc_renderer = None;
        self.last_joc_error = None;
        self.pending.clear();
    }
}

fn same_codec_family(current: Option<CodecKind>, incoming: CodecKind) -> bool {
    match (current, incoming) {
        (
            Some(CodecKind::Eac3 | CodecKind::Eac3Joc),
            CodecKind::Eac3 | CodecKind::Eac3Joc,
        ) => true,
        (Some(current), incoming) => current == incoming,
        (None, _) => false,
    }
}

/// Best-effort sample-rate hint from native AC-3/E-AC-3 headers.
pub(crate) fn sample_rate_hint(input: &[u8]) -> Option<u32> {
    if input.len() < 6 || input[0..2] != [0x0B, 0x77] {
        return None;
    }
    let bsid = input[5] >> 3;
    if bsid <= 10 {
        oxideav_ac3::syncinfo::parse(input)
            .ok()
            .map(|info| info.sample_rate)
    } else if bsid <= 16 {
        oxideav_ac3::eac3::bsi::parse(&input[2..])
            .ok()
            .map(|bsi| bsi.sample_rate)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::SampleType;

    #[test]
    fn routing_never_assigns_proprietary_backend() {
        for codec in [
            CodecKind::Ac3,
            CodecKind::Eac3,
            CodecKind::TrueHd,
            CodecKind::DtsHd,
            CodecKind::Flac,
            CodecKind::Opus,
        ] {
            assert_ne!(backend_class(codec), BackendClass::Unavailable);
        }
    }

    #[test]
    fn universal_decoder_requires_configuration_before_audio() {
        let mut decoder = UniversalOpenDecoder::new(OpenDecoderConfig::default());
        let err = decoder
            .decode_chunk(&[0x0B, 0x77, 0, 0, 0, 16 << 3])
            .unwrap_err();
        assert!(err.to_string().contains("not configured"));
    }

    #[test]
    fn accepts_aurora_40_frame_output_contract() {
        let mut decoder = UniversalOpenDecoder::new(OpenDecoderConfig::default());
        decoder
            .configure(AudioFormat {
                sample_rate: 48_000,
                channel_count: 12,
                sample_type: SampleType::F32,
                block_size: 40,
            })
            .unwrap();
    }

    #[test]
    fn eac3_and_joc_are_one_transport_family() {
        assert!(same_codec_family(
            Some(CodecKind::Eac3Joc),
            CodecKind::Eac3
        ));
        assert!(same_codec_family(
            Some(CodecKind::Eac3),
            CodecKind::Eac3Joc
        ));
        assert!(!same_codec_family(Some(CodecKind::Ac3), CodecKind::Eac3));
    }
}
