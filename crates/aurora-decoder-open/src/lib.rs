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

use std::collections::VecDeque;

use aurora_core::AudioFormat;
use aurora_decoder_api::{DecodedFrame, Decoder, DecoderError, DecoderInfo};
use framing::SyncFramer;
use iec61937::Iec61937Depacketizer;
use native_ac3::{JocPresentation, NativeAc3Decoder};
use sniff::{probe, CodecKind, Encapsulation};

pub use sniff::{CodecKind as OpenCodecKind, Encapsulation as OpenEncapsulation, ProbeResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendClass {
    /// Rust-native permissive backend integrated in-process.
    NativeOpen,
    /// Open-source external worker (FFmpeg/libavcodec class).
    OpenWorker,
    /// Raw PCM needs no codec decode.
    Passthrough,
    /// No admitted open backend is wired yet.
    Unavailable,
}

/// Route table kept separate from byte probing. This is the architectural
/// contract for broad codec support; adding a backend never changes callers.
pub const fn backend_class(codec: CodecKind) -> BackendClass {
    match codec {
        CodecKind::Ac3 | CodecKind::Eac3 | CodecKind::Eac3Joc => BackendClass::NativeOpen,
        CodecKind::Pcm => BackendClass::Passthrough,
        CodecKind::TrueHd
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
    transport: Transport,
    framer: Option<SyncFramer>,
    native: Option<NativeAc3Decoder>,
    pending: VecDeque<DecodedFrame>,
}

impl UniversalOpenDecoder {
    pub fn new(config: OpenDecoderConfig) -> Self {
        Self {
            codec: config.codec_hint,
            config,
            output_format: None,
            transport: Transport::Undecided,
            framer: None,
            native: None,
            pending: VecDeque::new(),
        }
    }

    pub fn detected_codec(&self) -> Option<CodecKind> {
        self.codec
    }

    fn ensure_codec(&mut self, data: &[u8]) -> Result<(), DecoderError> {
        if self.codec.is_some() && !matches!(self.transport, Transport::Undecided) {
            return Ok(());
        }

        let result = if let Some(codec) = self.codec {
            sniff::ProbeResult {
                codec,
                encapsulation: Encapsulation::Elementary,
                confidence: 100,
                iec61937_data_type: None,
            }
        } else {
            probe(data)
        };
        if result.codec == CodecKind::Unknown {
            return Err(DecoderError::UnsupportedInput(
                "unable to identify compressed audio codec from input prefix",
            ));
        }
        self.codec = Some(result.codec);
        self.transport = match result.encapsulation {
            Encapsulation::Iec61937 => Transport::Iec61937(Iec61937Depacketizer::new()),
            _ => Transport::Elementary,
        };
        self.initialize_backend(result.codec)
    }

    fn initialize_backend(&mut self, codec: CodecKind) -> Result<(), DecoderError> {
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
            BackendClass::OpenWorker => Err(DecoderError::Unavailable(
                "codec identified and routed to the open-worker class, but worker backend is not initialized",
            )),
            BackendClass::Passthrough => Err(DecoderError::Unavailable(
                "PCM passthrough is handled by Aurora audio I/O, not compressed decoder",
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
            BackendClass::OpenWorker => Err(DecoderError::Unavailable(
                "open worker backend not initialized",
            )),
            _ => Err(DecoderError::UnsupportedInput("unsupported elementary audio path")),
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
                // Carrier format may legitimately switch at runtime (PCM/DD+/etc.).
                self.codec = Some(burst.codec);
                self.native = None;
                self.framer = None;
                self.initialize_backend(burst.codec)?;
            }
            match backend_class(burst.codec) {
                BackendClass::NativeOpen => self.decode_native_packet(&burst.payload)?,
                BackendClass::OpenWorker => {
                    return Err(DecoderError::Unavailable(
                        "IEC61937 codec requires open worker backend that is not initialized",
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Flush complete codec packets retained by the elementary-stream framer.
    pub fn flush_packets(&mut self) -> Result<(), DecoderError> {
        let packets = self
            .framer
            .as_mut()
            .map(SyncFramer::flush)
            .unwrap_or_default();
        for packet in packets {
            self.decode_native_packet(&packet)?;
        }
        Ok(())
    }
}

impl Decoder for UniversalOpenDecoder {
    fn info(&self) -> DecoderInfo {
        DecoderInfo {
            name: "Aurora universal open decoder fabric",
            production_ready: false,
            maturity: "native-ac3-eac3-wired-open-worker-next",
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
        self.transport = Transport::Undecided;
        self.framer = None;
        self.native = None;
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
        let err = decoder.decode_chunk(&[0x0B, 0x77, 0, 0, 0, 16 << 3]).unwrap_err();
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
}
