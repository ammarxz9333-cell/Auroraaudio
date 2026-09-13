//! Experimental offline TrueHD channel-PCM integration.
//!
//! Aurora does not vendor `truehdd`. With the `truehdd-process` feature enabled this crate can
//! invoke a user-supplied `truehdd` executable for complete-file decoding and import a decoded
//! channel presentation as planar F32 PCM. Native Atmos object-scene ingestion is deliberately
//! not exposed until the DAMF audio/metadata set is mapped into Aurora with complete object/channel
//! semantics.

/// Whether this source revision exposes TrueHD/Atmos native object-scene decoding.
pub const TRUEHDD_OBJECT_SCENE_AVAILABLE: bool = false;

/// Explains why TrueHD/Atmos object-scene decoding is not currently exposed.
pub const fn truehdd_object_scene_unavailable_reason() -> &'static str {
    "DAMF object audio/metadata is not yet mapped to Aurora object-to-PCM bindings"
}

#[cfg(feature = "truehdd-process")]
mod channel_pcm {
    use std::{
        fs,
        path::{Path, PathBuf},
        process::Command,
        sync::atomic::{AtomicU64, Ordering},
    };

    use aurora_core::{AudioBlock, AudioFormat, SampleType};
    use aurora_decoder_api::{
        DecodedFrame, Decoder, DecoderError, DecoderInfo, DecoderOutputSemantics,
    };
    use serde_json::Value;

    const MAX_INPUT_BYTES: usize = 512 * 1024 * 1024;
    const MAX_OUTPUT_BYTES: u64 = 2 * 1024 * 1024 * 1024;
    const PCM_BYTES_PER_SAMPLE: usize = 3;
    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

    /// Complete-file TrueHD channel decoder backed by the external `truehdd` CLI.
    ///
    /// The adapter requests all presentations and chooses the highest indexed decoded channel
    /// presentation. Presentation 3/DAMF is intentionally not translated into fake objects.
    #[derive(Debug, Clone)]
    pub struct TruehddChannelPcmDecoder {
        executable: PathBuf,
        configured_format: Option<AudioFormat>,
    }

    impl TruehddChannelPcmDecoder {
        /// Creates an adapter using the supplied `truehdd` executable.
        pub fn new(executable: impl Into<PathBuf>) -> Self {
            Self {
                executable: executable.into(),
                configured_format: None,
            }
        }

        /// Returns the configured executable path.
        pub fn executable(&self) -> &Path {
            &self.executable
        }
    }

    impl Decoder for TruehddChannelPcmDecoder {
        fn info(&self) -> DecoderInfo {
            DecoderInfo {
                name: "truehdd complete-file channel-PCM decoder",
                production_ready: false,
                maturity: "experimental-offline",
                output_semantics: DecoderOutputSemantics::ChannelPcm,
            }
        }

        fn configure(&mut self, output_format: AudioFormat) -> Result<(), DecoderError> {
            if output_format.channel_count == 0 {
                return Err(DecoderError::UnsupportedInput(
                    "truehdd channel decode requires at least one output channel",
                ));
            }
            if output_format.sample_type != SampleType::F32 {
                return Err(DecoderError::UnsupportedInput(
                    "truehdd channel decode requires Aurora F32 output",
                ));
            }
            if output_format.block_size == 0 {
                return Err(DecoderError::UnsupportedInput(
                    "truehdd channel decode requires a non-zero block size",
                ));
            }
            self.configured_format = Some(output_format);
            Ok(())
        }

        fn decode_chunk(&mut self, input: &[u8]) -> Result<Option<DecodedFrame>, DecoderError> {
            let format = self
                .configured_format
                .ok_or(DecoderError::UnsupportedInput(
                    "truehdd channel decoder must be configured before decode",
                ))?;
            if input.is_empty() {
                return Err(DecoderError::UnsupportedInput(
                    "truehdd channel decode requires a complete non-empty TrueHD bitstream",
                ));
            }
            if input.len() > MAX_INPUT_BYTES {
                return Err(DecoderError::UnsupportedInput(
                    "TrueHD bitstream exceeds the offline decoder byte limit",
                ));
            }

            let workdir = TempWorkdir::create().map_err(process_io_error)?;
            let input_path = workdir.path.join("input.thd");
            let output_base = workdir.path.join("decoded");
            fs::write(&input_path, input).map_err(process_io_error)?;

            let output = Command::new(&self.executable)
                .arg("--loglevel")
                .arg("off")
                .arg("decode")
                .arg("--json")
                .arg("--format")
                .arg("pcm")
                .arg("--presentation")
                .arg("all")
                .arg("--output-path")
                .arg(&output_base)
                .arg(&input_path)
                .output()
                .map_err(|error| {
                    DecoderError::ExternalProcess(format!(
                        "failed to launch truehdd '{}': {error}",
                        self.executable.display()
                    ))
                })?;

            if !output.status.success() {
                return Err(DecoderError::ExternalProcess(format!(
                    "truehdd exited with {}: {}",
                    output.status,
                    bounded_text(&output.stderr)
                )));
            }

            let summary: Value = serde_json::from_slice(&output.stdout).map_err(|error| {
                DecoderError::ExternalProcess(format!(
                    "truehdd --json returned invalid JSON: {error}; stdout={}",
                    bounded_text(&output.stdout)
                ))
            })?;
            let selected = select_channel_presentation(&summary)?;

            if selected.sample_rate != format.sample_rate {
                return Err(DecoderError::ExternalProcess(format!(
                    "truehdd decoded {} Hz but Aurora was configured for {} Hz",
                    selected.sample_rate, format.sample_rate
                )));
            }
            if selected.channels != format.channel_count {
                return Err(DecoderError::ExternalProcess(format!(
                    "truehdd decoded {} channels but Aurora was configured for {}",
                    selected.channels, format.channel_count
                )));
            }

            let pcm_path = if selected.file.is_absolute() {
                selected.file
            } else {
                workdir.path.join(selected.file)
            };
            let metadata = fs::metadata(&pcm_path).map_err(process_io_error)?;
            if metadata.len() == 0 || metadata.len() > MAX_OUTPUT_BYTES {
                return Err(DecoderError::ExternalProcess(format!(
                    "truehdd PCM output size {} is outside the accepted bounds",
                    metadata.len()
                )));
            }
            let pcm = fs::read(&pcm_path).map_err(process_io_error)?;
            let audio = parse_pcm24le_interleaved(&pcm, format)?;

            if let Some(expected_frames) = selected.samples {
                if expected_frames != audio.frame_count as u64 {
                    return Err(DecoderError::ExternalProcess(format!(
                        "truehdd JSON reported {expected_frames} samples but PCM contains {} frames",
                        audio.frame_count
                    )));
                }
            }

            Ok(Some(DecodedFrame {
                audio,
                objects: Vec::new(),
            }))
        }

        fn reset(&mut self) {
            self.configured_format = None;
        }
    }

    /// Compatibility name retained for callers that used the old adapter type.
    pub type TruehddDecoderAdapter = TruehddChannelPcmDecoder;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct SelectedPresentation {
        index: u64,
        sample_rate: u32,
        channels: usize,
        samples: Option<u64>,
        file: PathBuf,
    }

    fn select_channel_presentation(summary: &Value) -> Result<SelectedPresentation, DecoderError> {
        let sample_rate = summary
            .get("sampleRate")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| {
                DecoderError::ExternalProcess(
                    "truehdd JSON is missing a valid sampleRate".to_owned(),
                )
            })?;
        let samples = summary.get("samples").and_then(Value::as_u64);
        let presentations = summary
            .get("presentations")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                DecoderError::ExternalProcess(
                    "truehdd JSON is missing presentations".to_owned(),
                )
            })?;

        let mut best: Option<SelectedPresentation> = None;
        for presentation in presentations {
            if presentation.get("format").and_then(Value::as_str) != Some("pcm") {
                continue;
            }
            let Some(index) = presentation.get("index").and_then(Value::as_u64) else {
                continue;
            };
            let Some(channels) = presentation
                .get("channels")
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
            else {
                continue;
            };
            if channels == 0 {
                continue;
            }
            let Some(file) = presentation
                .get("files")
                .and_then(Value::as_array)
                .and_then(|files| {
                    files.iter().filter_map(Value::as_str).find(|path| {
                        path.to_ascii_lowercase().ends_with(".pcm")
                    })
                })
            else {
                continue;
            };

            let candidate = SelectedPresentation {
                index,
                sample_rate,
                channels,
                samples,
                file: PathBuf::from(file),
            };
            if best.as_ref().is_none_or(|current| candidate.index > current.index) {
                best = Some(candidate);
            }
        }

        best.ok_or(DecoderError::UnsupportedInput(
            "TrueHD input exposes no decoded channel presentation; native Atmos object presentation is not mapped into Aurora",
        ))
    }

    fn parse_pcm24le_interleaved(
        bytes: &[u8],
        format: AudioFormat,
    ) -> Result<AudioBlock, DecoderError> {
        let bytes_per_frame = format
            .channel_count
            .checked_mul(PCM_BYTES_PER_SAMPLE)
            .ok_or_else(|| {
                DecoderError::ExternalProcess("truehdd PCM frame size overflow".to_owned())
            })?;
        if bytes.is_empty() || bytes_per_frame == 0 || bytes.len() % bytes_per_frame != 0 {
            return Err(DecoderError::ExternalProcess(
                "truehdd raw PCM is empty or not frame aligned".to_owned(),
            ));
        }

        let frame_count = bytes.len() / bytes_per_frame;
        let mut channels = (0..format.channel_count)
            .map(|_| Vec::with_capacity(frame_count))
            .collect::<Vec<_>>();
        for frame in bytes.chunks_exact(bytes_per_frame) {
            for (channel_index, channel) in channels.iter_mut().enumerate() {
                let offset = channel_index * PCM_BYTES_PER_SAMPLE;
                let raw = u32::from(frame[offset])
                    | (u32::from(frame[offset + 1]) << 8)
                    | (u32::from(frame[offset + 2]) << 16);
                let signed = if raw & 0x0080_0000 != 0 {
                    (raw | 0xff00_0000) as i32
                } else {
                    raw as i32
                };
                channel.push((f64::from(signed) / 8_388_608.0) as f32);
            }
        }

        let audio = AudioBlock {
            channels,
            frame_count,
            presentation_time_seconds: 0.0,
            discontinuity: false,
        };
        audio.validate().map_err(|error| {
            DecoderError::ExternalProcess(format!("invalid imported TrueHD PCM block: {error}"))
        })?;
        Ok(audio)
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
                let path = root.join(format!("aurora-truehdd-{pid}-{id}"));
                match fs::create_dir(&path) {
                    Ok(()) => return Ok(Self { path }),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => return Err(error),
                }
            }
            Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "could not allocate a unique truehdd work directory",
            ))
        }
    }

    impl Drop for TempWorkdir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn process_io_error(error: std::io::Error) -> DecoderError {
        DecoderError::ExternalProcess(format!("truehdd decoder I/O failed: {error}"))
    }

    fn bounded_text(bytes: &[u8]) -> String {
        const MAX_DIAGNOSTIC_BYTES: usize = 4096;
        let end = bytes.len().min(MAX_DIAGNOSTIC_BYTES);
        String::from_utf8_lossy(&bytes[..end]).into_owned()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn format(channels: usize) -> AudioFormat {
            AudioFormat {
                sample_rate: 48_000,
                channel_count: channels,
                sample_type: SampleType::F32,
                block_size: 256,
            }
        }

        #[test]
        fn summary_selects_highest_channel_presentation_and_ignores_damf() {
            let value: Value = serde_json::from_str(
                r#"{
                    "samples":2,
                    "sampleRate":48000,
                    "presentations":[
                        {"index":0,"format":"pcm","channels":2,"files":["out_p0.pcm"]},
                        {"index":2,"format":"pcm","channels":6,"files":["out_p2.pcm"]},
                        {"index":3,"format":"damf","channels":12,"files":["out.atmos","out.atmos.audio","out.atmos.metadata"]}
                    ]
                }"#,
            )
            .unwrap();
            let selected = select_channel_presentation(&value).unwrap();
            assert_eq!(selected.index, 2);
            assert_eq!(selected.channels, 6);
            assert_eq!(selected.file, PathBuf::from("out_p2.pcm"));
        }

        #[test]
        fn pcm24_parser_sign_extends_and_deinterleaves() {
            let bytes = [
                0x00, 0x00, 0x00, 0xff, 0xff, 0x7f, // 0, +max
                0x00, 0x00, 0x80, 0x00, 0x00, 0x40, // -min, +0.5
            ];
            let audio = parse_pcm24le_interleaved(&bytes, format(2)).unwrap();
            assert_eq!(audio.frame_count, 2);
            assert_eq!(audio.channels[0], vec![0.0, -1.0]);
            assert!((audio.channels[1][0] - 0.999_999_9).abs() < 1e-6);
            assert_eq!(audio.channels[1][1], 0.5);
        }

        #[test]
        fn object_only_summary_fails_closed() {
            let value: Value = serde_json::from_str(
                r#"{"sampleRate":48000,"presentations":[{"index":3,"format":"damf","channels":12,"files":["out.atmos"]}]}"#,
            )
            .unwrap();
            assert!(matches!(
                select_channel_presentation(&value),
                Err(DecoderError::UnsupportedInput(_))
            ));
        }

        #[test]
        fn missing_process_fails_closed() {
            let mut decoder = TruehddChannelPcmDecoder::new(
                "__aurora_truehdd_executable_that_does_not_exist__",
            );
            decoder.configure(format(2)).unwrap();
            assert!(matches!(
                decoder.decode_chunk(&[1, 2, 3]),
                Err(DecoderError::ExternalProcess(_))
            ));
        }
    }
}

#[cfg(feature = "truehdd-process")]
pub use channel_pcm::{TruehddChannelPcmDecoder, TruehddDecoderAdapter};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_object_scene_support_is_not_overclaimed() {
        assert!(!TRUEHDD_OBJECT_SCENE_AVAILABLE);
        assert!(truehdd_object_scene_unavailable_reason().contains("object-to-PCM"));
    }
}
