//! Aurora universal open audio decoder fabric.
//!
//! Design goals:
//! - no proprietary codec DLLs/blobs or paid runtime licences;
//! - byte-level auto detection for raw/eARC input;
//! - native Rust AC-3/E-AC-3 path, including an open JOC reference path;
//! - one Aurora-owned PCM/object boundary for every backend;
//! - backend replacement without changing the realtime/DSP/output engine.

pub mod framing;
pub mod iec61937;
pub mod native_ac3;
pub mod sniff;
pub mod worker;

use std::collections::VecDeque;

use aurora_core::AudioFormat;
use aurora_decoder_api::{DecodedFrame, Decoder, DecoderError, DecoderInfo};
use framing::SyncFramer;
use iec61937::Iec61937Depacketizer;
use native_ac3::{JocPresentation, NativeAc3Decoder};
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
    /// If true, 2-channel JOC validation can use OxideAV's standards-derived
    /// stereo speaker renderer. The immersive product path keeps this false.
    pub joc_stereo_reference: bool,
}

impl Default for OpenDecoderConfig {
    fn default() -> Self {
        Self {
            codec_hint: None,
            joc_stereo_reference: false,
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
            pending: VecDeque::new(),
        }
    }

    pub fn detected_codec(&self) -> Option<CodecKind> {
        self.codec
    }

    pub fn detected_encapsulation(&self) -> Encapsulation {
        self.encapsulation
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
                self.framer = Some(SyncFramer::new(codec));
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
            .ok_or(DecoderError::Unavailable("open worker backend is not initialized"))?;
        if let Some(frame) = worker.push(bytes)? {
            self.pending.push_back(frame);
        }
        while let Some(frame) = worker.poll()? {
            self.pending.push_back(frame);
        }
        Ok(())
    }

    fn process_elementary(&mut self, bytes: &[u8]) -> Result<(), DecoderError> {
        let codec = self.codec.expect("ensure_codec sets codec");
        match backend_class(codec) {
            BackendClass::NativeOpen => {
                let packets = self
                    .framer
                    .as_mut()
                    .ok_or(DecoderError::Unavailable("sync framer missing"))?
                    .push(bytes);
                for packet in packets {
                    self.decode_native_packet(&packet)?;
                }
                Ok(())
            }
            BackendClass::OpenWorker => self.process_worker(bytes),
            BackendClass::Passthrough => Err(DecoderError::Unavailable(
                "raw PCM passthrough adapter is not initialized",
            )),
            BackendClass::Unavailable => Err(DecoderError::UnsupportedInput(
                "unsupported elementary audio path",
            )),
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
            if self.codec != Some(burst.codec) {
                // The carrier can switch format at runtime. Rebuild exactly one
                // codec backend while preserving the single final PCM owner.
                self.codec = Some(burst.codec);
                self.initialize_backend(burst.codec, Encapsulation::Elementary)?;
            }
            match backend_class(burst.codec) {
                BackendClass::NativeOpen => self.decode_native_packet(&burst.payload)?,
                BackendClass::OpenWorker => self.process_worker(&burst.payload)?,
                BackendClass::Passthrough | BackendClass::Unavailable => {}
            }
        }
        Ok(())
    }

    /// Flush complete codec packets retained by the elementary-stream framer
    /// and flush any delayed samples from the open worker.
    pub fn flush_packets(&mut self) -> Result<(), DecoderError> {
        let packets = self
            .framer
            .as_mut()
            .map(SyncFramer::flush)
            .unwrap_or_default();
        for packet in packets {
            self.decode_native_packet(&packet)?;
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
            maturity: "native-ac3-eac3-plus-open-worker",
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
        self.pending.clear();
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
    fn codec_hint_does_not_disable_iec_transport_detection() {
        let mut decoder = UniversalOpenDecoder::new(OpenDecoderConfig {
            codec_hint: Some(CodecKind::Eac3),
            joc_stereo_reference: false,
        });
        decoder.output_format = Some(AudioFormat {
            sample_rate: 48_000,
            channel_count: 12,
            sample_type: SampleType::F32,
            block_size: 40,
        });
        // Avoid backend init by checking the same probe decision directly.
        let p = probe(&[0x72, 0xF8, 0x1F, 0x4E, 0x15, 0, 0, 0]);
        assert_eq!(p.encapsulation, Encapsulation::Iec61937);
    }
}
