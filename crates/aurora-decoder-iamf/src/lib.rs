//! IAMF decoder adapter boundaries.
//!
//! Aurora deliberately distinguishes a future object-scene integration from the
//! validation-only rendered-PCM reference path implemented here. The reference
//! path invokes a pinned `libiamf` `iamfdec` process and therefore exposes only
//! the rendered channel PCM that process writes; it never fabricates IAMF object
//! metadata or object-to-PCM bindings.

use aurora_core::AudioFormat;
use aurora_decoder_api::{
    DecodedFrame, Decoder, DecoderError, DecoderInfo, DecoderOutputSemantics,
};

/// Preferred open immersive-audio object-scene adapter placeholder.
///
/// This remains intentionally unavailable until Aurora has a reviewed backend
/// that exposes source object metadata and complete object-to-PCM bindings.
#[derive(Debug, Default, Clone)]
pub struct IamfDecoderAdapter {
    configured_format: Option<AudioFormat>,
}

impl IamfDecoderAdapter {
    /// Creates an IAMF object-scene adapter boundary.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Decoder for IamfDecoderAdapter {
    fn info(&self) -> DecoderInfo {
        DecoderInfo {
            name: "libiamf object-scene adapter",
            production_ready: false,
            maturity: "preferred-open-planned",
            output_semantics: DecoderOutputSemantics::ObjectScene,
        }
    }

    fn configure(&mut self, output_format: AudioFormat) -> Result<(), DecoderError> {
        self.configured_format = Some(output_format);
        Ok(())
    }

    fn decode_chunk(&mut self, _input: &[u8]) -> Result<Option<DecodedFrame>, DecoderError> {
        Err(DecoderError::Unavailable(
            "libiamf object-scene integration is not implemented",
        ))
    }

    fn reset(&mut self) {
        self.configured_format = None;
    }
}

#[cfg(feature = "libiamf-process")]
mod rendered_pcm_reference {
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
    const REFERENCE_SAMPLE_RATE: u32 = 48_000;
    const REFERENCE_CHANNELS: usize = 2;
    const REFERENCE_BITS_PER_SAMPLE: u16 = 32;
    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

    /// Validation-only adapter that imports `iamfdec` rendered stereo PCM.
    ///
    /// `decode_chunk` expects one complete standalone IAMF bitstream. It is not a
    /// streaming decoder and its output semantics are explicitly `ChannelPcm`.
    #[derive(Debug, Clone)]
    pub struct IamfRenderedPcmReferenceDecoder {
        executable: PathBuf,
        configured_format: Option<AudioFormat>,
    }

    impl IamfRenderedPcmReferenceDecoder {
        /// Creates a reference adapter for the supplied `iamfdec` executable.
        pub fn new(executable: impl Into<PathBuf>) -> Self {
            Self {
                executable: executable.into(),
                configured_format: None,
            }
        }
    }

    impl Decoder for IamfRenderedPcmReferenceDecoder {
        fn info(&self) -> DecoderInfo {
            DecoderInfo {
                name: "libiamf iamfdec rendered-PCM reference",
                production_ready: false,
                maturity: "validation-reference",
                output_semantics: DecoderOutputSemantics::ChannelPcm,
            }
        }

        fn configure(&mut self, output_format: AudioFormat) -> Result<(), DecoderError> {
            if output_format.sample_rate != REFERENCE_SAMPLE_RATE {
                return Err(DecoderError::UnsupportedInput(
                    "IAMF rendered-PCM reference currently requires 48 kHz output",
                ));
            }
            if output_format.channel_count != REFERENCE_CHANNELS {
                return Err(DecoderError::UnsupportedInput(
                    "IAMF rendered-PCM reference currently requires stereo output",
                ));
            }
            if output_format.sample_type != SampleType::F32 {
                return Err(DecoderError::UnsupportedInput(
                    "IAMF rendered-PCM reference requires Aurora F32 output",
                ));
            }
            if output_format.block_size == 0 {
                return Err(DecoderError::UnsupportedInput(
                    "IAMF rendered-PCM reference requires a non-zero block size",
                ));
            }

            self.configured_format = Some(output_format);
            Ok(())
        }

        fn decode_chunk(&mut self, input: &[u8]) -> Result<Option<DecodedFrame>, DecoderError> {
            let format = self
                .configured_format
                .ok_or(DecoderError::UnsupportedInput(
                    "IAMF rendered-PCM reference must be configured before decode",
                ))?;
            if input.is_empty() {
                return Err(DecoderError::UnsupportedInput(
                    "IAMF rendered-PCM reference requires a complete non-empty IAMF bitstream",
                ));
            }
            if input.len() > MAX_INPUT_BYTES {
                return Err(DecoderError::UnsupportedInput(
                    "IAMF rendered-PCM reference input exceeds the validation byte limit",
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
                .arg(REFERENCE_SAMPLE_RATE.to_string())
                .arg("-d")
                .arg(REFERENCE_BITS_PER_SAMPLE.to_string())
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
                    "iamfdec output size {} is outside the validation bounds",
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
                "could not allocate unique IAMF validation work directory",
            ))
        }
    }

    impl Drop for TempWorkdir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn process_io_error(error: std::io::Error) -> DecoderError {
        DecoderError::ExternalProcess(format!("IAMF reference I/O failed: {error}"))
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
        Some(u32::from_le_bytes([
            slice[0], slice[1], slice[2], slice[3],
        ]))
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

        let (encoding, channels, sample_rate, block_align, bits_per_sample) =
            wave_format.ok_or_else(|| {
                DecoderError::ExternalProcess("WAVE fmt chunk is missing".to_owned())
            })?;
        if encoding != 1 {
            return Err(DecoderError::ExternalProcess(format!(
                "unsupported iamfdec WAVE encoding {encoding}; expected integer PCM"
            )));
        }
        if channels as usize != format.channel_count || channels as usize != REFERENCE_CHANNELS {
            return Err(DecoderError::ExternalProcess(format!(
                "iamfdec WAVE has {channels} channels; expected {}",
                format.channel_count
            )));
        }
        if sample_rate != format.sample_rate || sample_rate != REFERENCE_SAMPLE_RATE {
            return Err(DecoderError::ExternalProcess(format!(
                "iamfdec WAVE sample rate is {sample_rate}; expected {}",
                format.sample_rate
            )));
        }
        if bits_per_sample != REFERENCE_BITS_PER_SAMPLE {
            return Err(DecoderError::ExternalProcess(format!(
                "iamfdec WAVE bit depth is {bits_per_sample}; expected {REFERENCE_BITS_PER_SAMPLE}"
            )));
        }
        let expected_block_align = channels
            .checked_mul(REFERENCE_BITS_PER_SAMPLE / 8)
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
            bytes.extend_from_slice(b"WAVE");
            bytes.extend_from_slice(b"fmt ");
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
        fn reference_adapter_reports_channel_pcm_only() {
            let adapter = IamfRenderedPcmReferenceDecoder::new("iamfdec");
            let info = adapter.info();
            assert_eq!(info.output_semantics, DecoderOutputSemantics::ChannelPcm);
            assert_eq!(info.maturity, "validation-reference");
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
        fn parser_rejects_float_wave_claim() {
            let mut wav = pcm32_wave(&[[0, 0]]);
            wav[20..22].copy_from_slice(&3u16.to_le_bytes());
            assert!(parse_pcm32_wave(&wav, stereo_format()).is_err());
        }

        #[test]
        fn reference_adapter_fails_closed_when_process_is_missing() {
            let mut adapter = IamfRenderedPcmReferenceDecoder::new(
                "__aurora_iamfdec_executable_that_does_not_exist__",
            );
            adapter.configure(stereo_format()).expect("valid config");
            let error = adapter
                .decode_chunk(&[1, 2, 3])
                .expect_err("must fail closed");
            assert!(matches!(error, DecoderError::ExternalProcess(_)));
        }
    }
}

#[cfg(feature = "libiamf-process")]
pub use rendered_pcm_reference::IamfRenderedPcmReferenceDecoder;

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_decoder_api::Decoder;

    #[test]
    fn iamf_object_scene_adapter_remains_planned() {
        let adapter = IamfDecoderAdapter::new();
        let info = adapter.info();

        assert_eq!(info.maturity, "preferred-open-planned");
        assert_eq!(info.output_semantics, DecoderOutputSemantics::ObjectScene);
        assert!(!info.production_ready);
    }
}
