//! IAMF decoder integration.
//!
//! The executable scope in this crate is deliberately narrow and truthful: when the
//! `libiamf-process` feature is enabled Aurora can invoke a reviewed `iamfdec` executable
//! for complete-file decoding and import its rendered channel PCM. Aurora does not expose
//! an IAMF object-scene decoder until a backend can provide source object/audio-element
//! metadata together with complete object-to-PCM bindings.

/// Whether this source revision exposes a native IAMF object-scene decoder.
///
/// `false` is a capability fact, not a callable decoder implementation. Callers that require
/// native objects must fail capability negotiation before constructing a decoder.
pub const IAMF_OBJECT_SCENE_AVAILABLE: bool = false;

/// Explains why native IAMF object-scene decoding is not currently exposed.
pub const fn iamf_object_scene_unavailable_reason() -> &'static str {
    "no reviewed IAMF backend exposes complete source object metadata and object-to-PCM bindings"
}

#[cfg(feature = "libiamf-process")]
mod rendered_pcm {
    use std::{
        fs,
        path::PathBuf,
        process::Command,
        sync::atomic::{AtomicU64, Ordering},
    };

    use aurora_core::{AudioBlock, AudioFormat, SampleType};
    use aurora_decoder_api::{
        DecodedFrame, Decoder, DecoderError, DecoderInfo, DecoderOutputSemantics,
    };

    const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;
    const MAX_OUTPUT_BYTES: u64 = 256 * 1024 * 1024;
    const OUTPUT_SAMPLE_RATE: u32 = 48_000;
    const OUTPUT_CHANNELS: usize = 2;
    const OUTPUT_BITS_PER_SAMPLE: u16 = 32;
    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

    /// Complete-file IAMF decoder backed by an external `iamfdec` executable.
    ///
    /// The backend renders IAMF before Aurora receives it, therefore this adapter reports
    /// [`DecoderOutputSemantics::ChannelPcm`] and never invents source objects.
    #[derive(Debug, Clone)]
    pub struct IamfRenderedPcmDecoder {
        executable: PathBuf,
        configured_format: Option<AudioFormat>,
    }

    impl IamfRenderedPcmDecoder {
        /// Creates an adapter using the supplied `iamfdec` executable.
        pub fn new(executable: impl Into<PathBuf>) -> Self {
            Self {
                executable: executable.into(),
                configured_format: None,
            }
        }

        /// Returns the configured executable path.
        pub fn executable(&self) -> &std::path::Path {
            &self.executable
        }
    }

    impl Decoder for IamfRenderedPcmDecoder {
        fn info(&self) -> DecoderInfo {
            DecoderInfo {
                name: "libiamf iamfdec rendered-PCM decoder",
                production_ready: false,
                maturity: "experimental-offline-reference",
                output_semantics: DecoderOutputSemantics::ChannelPcm,
            }
        }

        fn configure(&mut self, output_format: AudioFormat) -> Result<(), DecoderError> {
            if output_format.sample_rate != OUTPUT_SAMPLE_RATE {
                return Err(DecoderError::UnsupportedInput(
                    "IAMF rendered-PCM decode currently requires 48 kHz output",
                ));
            }
            if output_format.channel_count != OUTPUT_CHANNELS {
                return Err(DecoderError::UnsupportedInput(
                    "IAMF rendered-PCM decode currently requires stereo output",
                ));
            }
            if output_format.sample_type != SampleType::F32 {
                return Err(DecoderError::UnsupportedInput(
                    "IAMF rendered-PCM decode requires Aurora F32 output",
                ));
            }
            if output_format.block_size == 0 {
                return Err(DecoderError::UnsupportedInput(
                    "IAMF rendered-PCM decode requires a non-zero block size",
                ));
            }
            self.configured_format = Some(output_format);
            Ok(())
        }

        fn decode_chunk(&mut self, input: &[u8]) -> Result<Option<DecodedFrame>, DecoderError> {
            let format = self
                .configured_format
                .ok_or(DecoderError::UnsupportedInput(
                    "IAMF rendered-PCM decoder must be configured before decode",
                ))?;
            if input.is_empty() {
                return Err(DecoderError::UnsupportedInput(
                    "IAMF rendered-PCM decode requires a complete non-empty IAMF bitstream",
                ));
            }
            if input.len() > MAX_INPUT_BYTES {
                return Err(DecoderError::UnsupportedInput(
                    "IAMF bitstream exceeds the offline decoder byte limit",
                ));
            }

            let workdir = TempWorkdir::create().map_err(process_io_error)?;
            let input_path = workdir.path.join("input.iamf");
            let output_path = workdir.path.join("rendered.wav");
            fs::write(&input_path, input).map_err(process_io_error)?;

            let output = Command::new(&self.executable)
                .arg("-o3")
                .arg(&output_path)
                .arg("-s0")
                .arg("-r")
                .arg(OUTPUT_SAMPLE_RATE.to_string())
                .arg("-d")
                .arg(OUTPUT_BITS_PER_SAMPLE.to_string())
                .arg(&input_path)
                .output()
                .map_err(|error| {
                    DecoderError::ExternalProcess(format!(
                        "failed to launch iamfdec '{}': {error}",
                        self.executable.display()
                    ))
                })?;

            if !output.status.success() {
                return Err(DecoderError::ExternalProcess(format!(
                    "iamfdec exited with {}: {}",
                    output.status,
                    bounded_text(&output.stderr)
                )));
            }

            let metadata = fs::metadata(&output_path).map_err(process_io_error)?;
            if metadata.len() == 0 || metadata.len() > MAX_OUTPUT_BYTES {
                return Err(DecoderError::ExternalProcess(format!(
                    "iamfdec output size {} is outside the accepted bounds",
                    metadata.len()
                )));
            }

            let wav = fs::read(&output_path).map_err(process_io_error)?;
            let audio = parse_pcm32_wave(&wav, format)?;
            Ok(Some(DecodedFrame {
                audio,
                objects: Vec::new(),
            }))
        }

        fn reset(&mut self) {
            self.configured_format = None;
        }
    }

    /// Compatibility name retained for existing validation tooling.
    pub type IamfRenderedPcmReferenceDecoder = IamfRenderedPcmDecoder;

    struct TempWorkdir {
        path: PathBuf,
    }

    impl TempWorkdir {
        fn create() -> std::io::Result<Self> {
            let root = std::env::temp_dir();
            let pid = std::process::id();
            for _ in 0..16 {
                let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
                let path = root.join(format!("aurora-iamf-{pid}-{id}"));
                match fs::create_dir(&path) {
                    Ok(()) => return Ok(Self { path }),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => return Err(error),
                }
            }
            Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "could not allocate a unique IAMF work directory",
            ))
        }
    }

    impl Drop for TempWorkdir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn process_io_error(error: std::io::Error) -> DecoderError {
        DecoderError::ExternalProcess(format!("IAMF decoder I/O failed: {error}"))
    }

    fn bounded_text(bytes: &[u8]) -> String {
        const MAX_DIAGNOSTIC_BYTES: usize = 4096;
        let end = bytes.len().min(MAX_DIAGNOSTIC_BYTES);
        String::from_utf8_lossy(&bytes[..end]).into_owned()
    }

    fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
        let slice = bytes.get(offset..offset.checked_add(2)?)?;
        Some(u16::from_le_bytes([slice[0], slice[1]]))
    }

    fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
        let slice = bytes.get(offset..offset.checked_add(4)?)?;
        Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
    }

    fn parse_pcm32_wave(bytes: &[u8], format: AudioFormat) -> Result<AudioBlock, DecoderError> {
        if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            return Err(DecoderError::ExternalProcess(
                "iamfdec output is not a RIFF/WAVE file".to_owned(),
            ));
        }

        let mut offset = 12usize;
        let mut wave_format = None;
        let mut data_range = None;
        while offset.saturating_add(8) <= bytes.len() {
            let id = &bytes[offset..offset + 4];
            let size = read_u32(bytes, offset + 4).ok_or_else(|| {
                DecoderError::ExternalProcess("truncated WAVE chunk size".to_owned())
            })? as usize;
            let payload_start = offset + 8;
            let payload_end = payload_start.checked_add(size).ok_or_else(|| {
                DecoderError::ExternalProcess("WAVE chunk size overflow".to_owned())
            })?;
            if payload_end > bytes.len() {
                return Err(DecoderError::ExternalProcess(
                    "truncated WAVE chunk payload".to_owned(),
                ));
            }

            if id == b"fmt " {
                if size < 16 {
                    return Err(DecoderError::ExternalProcess(
                        "WAVE fmt chunk is shorter than 16 bytes".to_owned(),
                    ));
                }
                wave_format = Some((
                    read_u16(bytes, payload_start).unwrap_or(0),
                    read_u16(bytes, payload_start + 2).unwrap_or(0),
                    read_u32(bytes, payload_start + 4).unwrap_or(0),
                    read_u16(bytes, payload_start + 12).unwrap_or(0),
                    read_u16(bytes, payload_start + 14).unwrap_or(0),
                ));
            } else if id == b"data" {
                data_range = Some((payload_start, payload_end));
            }

            offset = payload_end.checked_add(size & 1).ok_or_else(|| {
                DecoderError::ExternalProcess("WAVE chunk padding overflow".to_owned())
            })?;
        }

        let (encoding, channels, sample_rate, block_align, bits_per_sample) = wave_format
            .ok_or_else(|| DecoderError::ExternalProcess("WAVE fmt chunk is missing".to_owned()))?;
        if encoding != 1 {
            return Err(DecoderError::ExternalProcess(format!(
                "unsupported iamfdec WAVE encoding {encoding}; expected integer PCM"
            )));
        }
        if channels as usize != format.channel_count || channels as usize != OUTPUT_CHANNELS {
            return Err(DecoderError::ExternalProcess(format!(
                "iamfdec WAVE has {channels} channels; expected {}",
                format.channel_count
            )));
        }
        if sample_rate != format.sample_rate || sample_rate != OUTPUT_SAMPLE_RATE {
            return Err(DecoderError::ExternalProcess(format!(
                "iamfdec WAVE sample rate is {sample_rate}; expected {}",
                format.sample_rate
            )));
        }
        if bits_per_sample != OUTPUT_BITS_PER_SAMPLE {
            return Err(DecoderError::ExternalProcess(format!(
                "iamfdec WAVE bit depth is {bits_per_sample}; expected {OUTPUT_BITS_PER_SAMPLE}"
            )));
        }
        let expected_block_align = channels
            .checked_mul(OUTPUT_BITS_PER_SAMPLE / 8)
            .ok_or_else(|| {
                DecoderError::ExternalProcess("WAVE block alignment overflow".to_owned())
            })?;
        if block_align != expected_block_align {
            return Err(DecoderError::ExternalProcess(format!(
                "iamfdec WAVE block alignment is {block_align}; expected {expected_block_align}"
            )));
        }

        let (data_start, data_end) = data_range.ok_or_else(|| {
            DecoderError::ExternalProcess("WAVE data chunk is missing".to_owned())
        })?;
        let data = &bytes[data_start..data_end];
        if data.is_empty() || data.len() % block_align as usize != 0 {
            return Err(DecoderError::ExternalProcess(
                "WAVE PCM payload is empty or not frame aligned".to_owned(),
            ));
        }

        let frame_count = data.len() / block_align as usize;
        let mut planar = (0..channels)
            .map(|_| Vec::with_capacity(frame_count))
            .collect::<Vec<_>>();
        for frame in data.chunks_exact(block_align as usize) {
            for (channel_index, channel) in planar.iter_mut().enumerate() {
                let sample_offset = channel_index * 4;
                let sample = i32::from_le_bytes([
                    frame[sample_offset],
                    frame[sample_offset + 1],
                    frame[sample_offset + 2],
                    frame[sample_offset + 3],
                ]);
                let normalized = (f64::from(sample) / 2_147_483_648.0) as f32;
                if !normalized.is_finite() {
                    return Err(DecoderError::ExternalProcess(
                        "iamfdec WAVE produced a non-finite sample".to_owned(),
                    ));
                }
                channel.push(normalized);
            }
        }

        let audio = AudioBlock {
            channels: planar,
            frame_count,
            presentation_time_seconds: 0.0,
            discontinuity: false,
        };
        audio.validate().map_err(|error| {
            DecoderError::ExternalProcess(format!("invalid imported IAMF PCM block: {error}"))
        })?;
        Ok(audio)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn stereo_format() -> AudioFormat {
            AudioFormat {
                sample_rate: 48_000,
                channel_count: 2,
                sample_type: SampleType::F32,
                block_size: 256,
            }
        }

        fn pcm32_wave(samples: &[[i32; 2]]) -> Vec<u8> {
            let data_size = (samples.len() * 2 * 4) as u32;
            let mut bytes = Vec::new();
            bytes.extend_from_slice(b"RIFF");
            bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
            bytes.extend_from_slice(b"WAVEfmt ");
            bytes.extend_from_slice(&16u32.to_le_bytes());
            bytes.extend_from_slice(&1u16.to_le_bytes());
            bytes.extend_from_slice(&2u16.to_le_bytes());
            bytes.extend_from_slice(&48_000u32.to_le_bytes());
            bytes.extend_from_slice(&(48_000u32 * 8).to_le_bytes());
            bytes.extend_from_slice(&8u16.to_le_bytes());
            bytes.extend_from_slice(&32u16.to_le_bytes());
            bytes.extend_from_slice(b"data");
            bytes.extend_from_slice(&data_size.to_le_bytes());
            for frame in samples {
                bytes.extend_from_slice(&frame[0].to_le_bytes());
                bytes.extend_from_slice(&frame[1].to_le_bytes());
            }
            bytes
        }

        #[test]
        fn decoder_reports_channel_pcm_only() {
            let adapter = IamfRenderedPcmDecoder::new("iamfdec");
            let info = adapter.info();
            assert_eq!(info.output_semantics, DecoderOutputSemantics::ChannelPcm);
            assert!(!info.production_ready);
        }

        #[test]
        fn parser_imports_interleaved_pcm32_as_planar_f32() {
            let wav = pcm32_wave(&[[0, i32::MAX], [i32::MIN, 1_073_741_824]]);
            let audio = parse_pcm32_wave(&wav, stereo_format()).expect("valid PCM32 WAVE");
            assert_eq!(audio.frame_count, 2);
            assert_eq!(audio.channels.len(), 2);
            assert_eq!(audio.channels[0], vec![0.0, -1.0]);
            assert!((audio.channels[1][0] - 1.0).abs() < f32::EPSILON);
            assert_eq!(audio.channels[1][1], 0.5);
        }

        #[test]
        fn missing_process_fails_closed() {
            let mut adapter = IamfRenderedPcmDecoder::new(
                "__aurora_iamfdec_executable_that_does_not_exist__",
            );
            adapter.configure(stereo_format()).expect("valid config");
            assert!(matches!(
                adapter.decode_chunk(&[1, 2, 3]),
                Err(DecoderError::ExternalProcess(_))
            ));
        }
    }
}

#[cfg(feature = "libiamf-process")]
pub use rendered_pcm::{IamfRenderedPcmDecoder, IamfRenderedPcmReferenceDecoder};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_object_scene_support_is_not_overclaimed() {
        assert!(!IAMF_OBJECT_SCENE_AVAILABLE);
        assert!(iamf_object_scene_unavailable_reason().contains("object-to-PCM"));
    }
}
