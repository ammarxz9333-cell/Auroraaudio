//! Functional offline out-of-process CamillaDSP integration.
//!
//! This crate does not fork or vendor CamillaDSP. It discovers an installed executable,
//! generates Aurora-owned YAML, invokes CamillaDSP for file processing, and validates the
//! produced WAV. It deliberately does not implement Aurora's in-place `DspEngine`: spawning a
//! process and doing filesystem I/O is an offline/control-plane operation, not a realtime block
//! processor.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use aurora_audio_io::read_wav;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const ENV_CAMILLADSP_PATH: &str = "AURORA_CAMILLADSP_PATH";

/// Aurora-owned DSP configuration for CamillaDSP YAML generation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuroraDspConfig {
    pub channel_count: usize,
    pub sample_rate: u32,
    #[serde(default = "default_chunk_size")]
    pub chunk_size: usize,
    pub channels: Vec<ChannelDspConfig>,
}

impl AuroraDspConfig {
    pub fn from_json_file(path: &Path) -> Result<Self, CamillaDspError> {
        let text = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&text)?)
    }
}

/// Per-channel controls supported by the offline adapter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChannelDspConfig {
    pub channel: usize,
    #[serde(default)]
    pub gain_db: f32,
    #[serde(default)]
    pub mute: bool,
    #[serde(default)]
    pub polarity_invert: bool,
    #[serde(default)]
    pub delay_ms: f32,
    #[serde(default)]
    pub high_pass: Option<CutoffFilter>,
    #[serde(default)]
    pub low_pass: Option<CutoffFilter>,
    #[serde(default)]
    pub parametric_eq: Vec<ParametricEq>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CutoffFilter {
    pub frequency_hz: f32,
    #[serde(default = "default_filter_order")]
    pub order: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParametricEq {
    pub frequency_hz: f32,
    pub gain_db: f32,
    pub q: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CamillaDspExecutable {
    pub path: PathBuf,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CamillaDspCommand {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CamillaDspRunReport {
    pub command: CamillaDspCommand,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessedWavInfo {
    pub format_tag: u16,
    pub channel_count: usize,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    pub frame_count: usize,
}

#[derive(Debug, Error)]
pub enum CamillaDspError {
    #[error("CamillaDSP executable not found; set an explicit path, {ENV_CAMILLADSP_PATH}, or PATH")]
    MissingExecutable,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("wav validation error: {0}")]
    Wav(#[from] aurora_audio_io::AudioIoError),
    #[error("CamillaDSP failed with status {status}: {stderr}")]
    ProcessFailed { status: String, stderr: String },
    #[error("CamillaDSP timed out after {timeout:?}")]
    Timeout { timeout: Duration },
    #[error("invalid CamillaDSP adapter config: {0}")]
    InvalidConfig(String),
    #[error("invalid processed WAV: {0}")]
    InvalidOutputWav(String),
}

/// Executable offline adapter. It has no in-place/realtime processing method by design.
#[derive(Debug, Clone)]
pub struct CamillaDspOfflineAdapter {
    executable: CamillaDspExecutable,
}

impl CamillaDspOfflineAdapter {
    /// Discovers CamillaDSP and creates an executable offline adapter.
    pub fn discover(explicit_path: Option<&Path>) -> Result<Self, CamillaDspError> {
        Ok(Self {
            executable: discover_camilladsp(explicit_path)?,
        })
    }

    /// Creates an adapter from an already validated executable descriptor.
    pub fn new(executable: CamillaDspExecutable) -> Self {
        Self { executable }
    }

    pub fn executable(&self) -> &CamillaDspExecutable {
        &self.executable
    }

    /// Processes one complete WAV file through CamillaDSP.
    pub fn process_wav(
        &self,
        input_path: &Path,
        output_path: &Path,
        config: &AuroraDspConfig,
        keep_temp: bool,
        timeout: Duration,
    ) -> Result<CamillaDspRunReport, CamillaDspError> {
        process_offline_wav(
            &self.executable.path,
            input_path,
            output_path,
            config,
            keep_temp,
            timeout,
        )
    }
}

pub fn discover_camilladsp(
    explicit_path: Option<&Path>,
) -> Result<CamillaDspExecutable, CamillaDspError> {
    let path = if let Some(path) = explicit_path {
        path.to_path_buf()
    } else if let Ok(path) = std::env::var(ENV_CAMILLADSP_PATH) {
        PathBuf::from(path)
    } else {
        find_on_path("camilladsp").ok_or(CamillaDspError::MissingExecutable)?
    };

    if !path.is_file() {
        return Err(CamillaDspError::MissingExecutable);
    }

    let version = Command::new(&path)
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .ok()
        .map(|output| {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            if stdout.is_empty() { stderr } else { stdout }
        })
        .filter(|value| !value.is_empty());

    Ok(CamillaDspExecutable { path, version })
}

pub fn generate_camilladsp_yaml(
    config: &AuroraDspConfig,
    input_path: &Path,
    output_path: &Path,
) -> Result<String, CamillaDspError> {
    validate_config(config)?;

    let mut yaml = String::new();
    yaml.push_str("devices:\n");
    yaml.push_str(&format!("  samplerate: {}\n", config.sample_rate));
    yaml.push_str(&format!("  chunksize: {}\n", config.chunk_size));
    yaml.push_str("  capture:\n    type: WavFile\n");
    yaml.push_str(&format!("    filename: \"{}\"\n", yaml_path(input_path)));
    yaml.push_str("  playback:\n    type: File\n");
    yaml.push_str(&format!("    channels: {}\n", config.channel_count));
    yaml.push_str(&format!("    filename: \"{}\"\n", yaml_path(output_path)));
    yaml.push_str("    format: F32_LE\n    wav_header: true\nfilters:\n");
    for channel in &config.channels {
        append_channel_filters(&mut yaml, channel, config.sample_rate);
    }
    yaml.push_str("pipeline:\n");
    for channel in &config.channels {
        append_channel_pipeline(&mut yaml, channel);
    }
    Ok(yaml)
}

pub fn build_camilladsp_command(executable: &Path, config_path: &Path) -> CamillaDspCommand {
    CamillaDspCommand {
        executable: executable.to_path_buf(),
        args: vec![config_path.as_os_str().to_os_string()],
    }
}

pub fn process_offline_wav(
    executable: &Path,
    input_path: &Path,
    output_path: &Path,
    config: &AuroraDspConfig,
    keep_temp: bool,
    timeout: Duration,
) -> Result<CamillaDspRunReport, CamillaDspError> {
    validate_config(config)?;
    let input_wav = read_wav(input_path)?;
    if input_wav.format.channel_count != config.channel_count {
        return Err(CamillaDspError::InvalidConfig(format!(
            "config channel_count {} does not match input channel count {}",
            config.channel_count, input_wav.format.channel_count
        )));
    }
    if input_wav.format.sample_rate != config.sample_rate {
        return Err(CamillaDspError::InvalidConfig(format!(
            "config sample_rate {} does not match input sample rate {}",
            config.sample_rate, input_wav.format.sample_rate
        )));
    }

    let temp_dir = std::env::temp_dir().join(format!(
        "aurora_camilladsp_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir_all(&temp_dir)?;
    let config_path = temp_dir.join("camilladsp.yml");
    std::fs::write(
        &config_path,
        generate_camilladsp_yaml(config, input_path, output_path)?,
    )?;

    let command = build_camilladsp_command(executable, &config_path);
    let mut child = Command::new(&command.executable)
        .args(&command.args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let start = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            if !keep_temp {
                let _ = std::fs::remove_dir_all(&temp_dir);
            }
            return Err(CamillaDspError::Timeout { timeout });
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    let output = child.wait_with_output()?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !output.status.success() {
        if !keep_temp {
            let _ = std::fs::remove_dir_all(&temp_dir);
        }
        return Err(CamillaDspError::ProcessFailed {
            status: output.status.to_string(),
            stderr,
        });
    }

    finalize_streaming_wav(output_path)?;
    if !keep_temp {
        std::fs::remove_dir_all(&temp_dir)?;
    }

    Ok(CamillaDspRunReport {
        command,
        stdout,
        stderr,
    })
}

pub fn inspect_processed_wav(path: &Path) -> Result<ProcessedWavInfo, CamillaDspError> {
    let bytes = std::fs::read(path)?;
    if bytes.len() < 44 {
        return Err(CamillaDspError::InvalidOutputWav(
            "file is too short for WAV header".to_owned(),
        ));
    }
    if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(CamillaDspError::InvalidOutputWav(
            "missing RIFF/WAVE signature".to_owned(),
        ));
    }

    let mut offset = 12usize;
    let mut format_tag = None;
    let mut channel_count = None;
    let mut sample_rate = None;
    let mut bits_per_sample = None;
    let mut data_start = None;
    let mut data_size = None;
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let size = u32::from_le_bytes([
            bytes[offset + 4], bytes[offset + 5], bytes[offset + 6], bytes[offset + 7],
        ]) as usize;
        let chunk_start = offset + 8;
        if id == b"fmt " {
            if chunk_start + 16 > bytes.len() {
                return Err(CamillaDspError::InvalidOutputWav("truncated fmt chunk".to_owned()));
            }
            format_tag = Some(u16::from_le_bytes([bytes[chunk_start], bytes[chunk_start + 1]]));
            channel_count = Some(u16::from_le_bytes([
                bytes[chunk_start + 2], bytes[chunk_start + 3],
            ]) as usize);
            sample_rate = Some(u32::from_le_bytes([
                bytes[chunk_start + 4], bytes[chunk_start + 5], bytes[chunk_start + 6], bytes[chunk_start + 7],
            ]));
            bits_per_sample = Some(u16::from_le_bytes([
                bytes[chunk_start + 14], bytes[chunk_start + 15],
            ]));
        } else if id == b"data" {
            data_start = Some(chunk_start);
            data_size = Some(size);
            break;
        }
        offset = chunk_start + size + (size % 2);
    }

    let channel_count = channel_count
        .ok_or_else(|| CamillaDspError::InvalidOutputWav("missing channel count".to_owned()))?;
    let bits_per_sample = bits_per_sample
        .ok_or_else(|| CamillaDspError::InvalidOutputWav("missing bits per sample".to_owned()))?;
    let data_start = data_start
        .ok_or_else(|| CamillaDspError::InvalidOutputWav("missing data chunk".to_owned()))?;
    let declared_data_size = data_size.unwrap_or(0);
    let actual_data_size = bytes.len().saturating_sub(data_start);
    let usable_data_size = if declared_data_size == u32::MAX as usize {
        actual_data_size
    } else {
        declared_data_size.min(actual_data_size)
    };
    let bytes_per_frame = channel_count * usize::from(bits_per_sample / 8);
    if bytes_per_frame == 0 || usable_data_size % bytes_per_frame != 0 {
        return Err(CamillaDspError::InvalidOutputWav(
            "data length is not a whole number of frames".to_owned(),
        ));
    }

    Ok(ProcessedWavInfo {
        format_tag: format_tag.unwrap_or(0),
        channel_count,
        sample_rate: sample_rate.unwrap_or(0),
        bits_per_sample,
        frame_count: usable_data_size / bytes_per_frame,
    })
}

pub fn finalize_streaming_wav(path: &Path) -> Result<ProcessedWavInfo, CamillaDspError> {
    let mut bytes = std::fs::read(path)?;
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(CamillaDspError::InvalidOutputWav(
            "missing or truncated RIFF/WAVE header".to_owned(),
        ));
    }

    let mut offset = 12usize;
    let mut data_size_offset = None;
    let mut data_start = None;
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let size = u32::from_le_bytes([
            bytes[offset + 4], bytes[offset + 5], bytes[offset + 6], bytes[offset + 7],
        ]) as usize;
        let chunk_start = offset + 8;
        if id == b"data" {
            data_size_offset = Some(offset + 4);
            data_start = Some(chunk_start);
            break;
        }
        offset = chunk_start + size + (size % 2);
    }

    let data_size_offset = data_size_offset
        .ok_or_else(|| CamillaDspError::InvalidOutputWav("missing data chunk".to_owned()))?;
    let data_start = data_start
        .ok_or_else(|| CamillaDspError::InvalidOutputWav("missing data chunk".to_owned()))?;
    let riff_size = bytes.len().checked_sub(8)
        .ok_or_else(|| CamillaDspError::InvalidOutputWav("invalid RIFF size".to_owned()))?;
    let data_size = bytes.len().checked_sub(data_start)
        .ok_or_else(|| CamillaDspError::InvalidOutputWav("invalid data size".to_owned()))?;
    if riff_size > u32::MAX as usize || data_size > u32::MAX as usize {
        return Err(CamillaDspError::InvalidOutputWav(
            "WAV is too large for RIFF chunk sizes".to_owned(),
        ));
    }

    bytes[4..8].copy_from_slice(&(riff_size as u32).to_le_bytes());
    bytes[data_size_offset..data_size_offset + 4]
        .copy_from_slice(&(data_size as u32).to_le_bytes());
    std::fs::write(path, bytes)?;
    inspect_processed_wav(path)
}

fn validate_config(config: &AuroraDspConfig) -> Result<(), CamillaDspError> {
    if config.channel_count == 0 {
        return Err(CamillaDspError::InvalidConfig(
            "channel_count must be greater than zero".to_owned(),
        ));
    }
    if config.sample_rate == 0 || config.chunk_size == 0 {
        return Err(CamillaDspError::InvalidConfig(
            "sample_rate and chunk_size must be greater than zero".to_owned(),
        ));
    }
    for channel in &config.channels {
        if channel.channel >= config.channel_count {
            return Err(CamillaDspError::InvalidConfig(format!(
                "channel {} is outside channel_count {}",
                channel.channel, config.channel_count
            )));
        }
        if !channel.gain_db.is_finite() || !channel.delay_ms.is_finite() || channel.delay_ms < 0.0 {
            return Err(CamillaDspError::InvalidConfig(format!(
                "channel {} has non-finite gain/delay or negative delay",
                channel.channel
            )));
        }
        for eq in &channel.parametric_eq {
            if !eq.frequency_hz.is_finite() || eq.frequency_hz <= 0.0 || !eq.gain_db.is_finite() || !eq.q.is_finite() || eq.q <= 0.0 {
                return Err(CamillaDspError::InvalidConfig(format!(
                    "channel {} has invalid parametric EQ values",
                    channel.channel
                )));
            }
        }
    }
    Ok(())
}

fn append_channel_filters(yaml: &mut String, channel: &ChannelDspConfig, sample_rate: u32) {
    yaml.push_str(&format!("  ch{}_gain:\n    type: Gain\n    parameters:\n", channel.channel));
    yaml.push_str(&format!("      gain: {:.6}\n", effective_gain_db(channel)));
    yaml.push_str(&format!("      inverted: {}\n", channel.polarity_invert));

    if channel.delay_ms > 0.0 {
        yaml.push_str(&format!("  ch{}_delay:\n    type: Delay\n    parameters:\n", channel.channel));
        yaml.push_str(&format!("      delay: {:.6}\n", delay_ms_to_samples(channel.delay_ms, sample_rate)));
        yaml.push_str("      unit: samples\n      subsample: true\n");
    }
    if let Some(filter) = &channel.high_pass {
        yaml.push_str(&format!("  ch{}_highpass:\n    type: Biquad\n    parameters:\n      type: Highpass\n", channel.channel));
        yaml.push_str(&format!("      freq: {:.3}\n      q: 0.707000\n", filter.frequency_hz));
    }
    if let Some(filter) = &channel.low_pass {
        yaml.push_str(&format!("  ch{}_lowpass:\n    type: Biquad\n    parameters:\n      type: Lowpass\n", channel.channel));
        yaml.push_str(&format!("      freq: {:.3}\n      q: 0.707000\n", filter.frequency_hz));
    }
    for (index, eq) in channel.parametric_eq.iter().enumerate() {
        yaml.push_str(&format!("  ch{}_peq{}:\n    type: Biquad\n    parameters:\n      type: Peaking\n", channel.channel, index));
        yaml.push_str(&format!("      freq: {:.3}\n      gain: {:.6}\n      q: {:.6}\n", eq.frequency_hz, eq.gain_db, eq.q));
    }
}

fn append_channel_pipeline(yaml: &mut String, channel: &ChannelDspConfig) {
    let channel_id = channel.channel;
    yaml.push_str("  - type: Filter\n");
    yaml.push_str(&format!("    channels: [{channel_id}]\n"));
    yaml.push_str(&format!("    names: [ch{channel_id}_gain"));
    if channel.delay_ms > 0.0 { yaml.push_str(&format!(", ch{channel_id}_delay")); }
    if channel.high_pass.is_some() { yaml.push_str(&format!(", ch{channel_id}_highpass")); }
    if channel.low_pass.is_some() { yaml.push_str(&format!(", ch{channel_id}_lowpass")); }
    for index in 0..channel.parametric_eq.len() {
        yaml.push_str(&format!(", ch{channel_id}_peq{index}"));
    }
    yaml.push_str("]\n");
}

pub fn effective_gain_db(channel: &ChannelDspConfig) -> f32 {
    if channel.mute { -120.0 } else { channel.gain_db }
}

pub fn delay_ms_to_samples(delay_ms: f32, sample_rate: u32) -> f32 {
    delay_ms / 1000.0 * sample_rate as f32
}

fn yaml_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    let candidates = executable_names(name);
    for dir in std::env::split_paths(&path_var) {
        for candidate in &candidates {
            let path = dir.join(candidate);
            if path.is_file() { return Some(path); }
        }
    }
    None
}

fn executable_names(name: &str) -> Vec<String> {
    if cfg!(windows) {
        vec![format!("{name}.exe"), name.to_owned()]
    } else {
        vec![name.to_owned()]
    }
}

fn default_chunk_size() -> usize { 1024 }
fn default_filter_order() -> u8 { 2 }

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_audio_io::{read_wav, write_wav_f32_with_channel_roles};
    use aurora_core::ChannelRole;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn sample_config() -> AuroraDspConfig {
        AuroraDspConfig {
            channel_count: 6,
            sample_rate: 48_000,
            chunk_size: 1024,
            channels: vec![
                ChannelDspConfig {
                    channel: 0,
                    gain_db: -3.0,
                    mute: false,
                    polarity_invert: false,
                    delay_ms: 2.5,
                    high_pass: None,
                    low_pass: None,
                    parametric_eq: vec![ParametricEq {
                        frequency_hz: 1000.0,
                        gain_db: 1.5,
                        q: 0.707,
                    }],
                },
                ChannelDspConfig {
                    channel: 4,
                    gain_db: 0.0,
                    mute: false,
                    polarity_invert: true,
                    delay_ms: 0.0,
                    high_pass: Some(CutoffFilter { frequency_hz: 80.0, order: 2 }),
                    low_pass: None,
                    parametric_eq: vec![],
                },
            ],
        }
    }

    #[test]
    fn aurora_config_generates_yaml() {
        let yaml = generate_camilladsp_yaml(&sample_config(), Path::new("input.wav"), Path::new("output.wav")).unwrap();
        assert!(yaml.contains("samplerate: 48000"));
        assert!(yaml.contains("filename: \"input.wav\""));
        assert!(yaml.contains("ch0_gain"));
        assert!(yaml.contains("ch4_highpass"));
    }

    #[test]
    fn channel_mapping_uses_zero_based_channel_numbers() {
        let yaml = generate_camilladsp_yaml(&sample_config(), Path::new("input.wav"), Path::new("output.wav")).unwrap();
        assert!(yaml.contains("channels: [0]"));
        assert!(yaml.contains("channels: [4]"));
    }

    #[test]
    fn gain_delay_and_filter_conversion_are_deterministic() {
        let mut channel = sample_config().channels[0].clone();
        assert_eq!(effective_gain_db(&channel), -3.0);
        channel.mute = true;
        assert_eq!(effective_gain_db(&channel), -120.0);
        assert_eq!(delay_ms_to_samples(2.5, 48_000), 120.0);
        let yaml = generate_camilladsp_yaml(&sample_config(), Path::new("input.wav"), Path::new("output.wav")).unwrap();
        assert!(yaml.contains("type: Highpass"));
        assert!(yaml.contains("type: Peaking"));
    }

    #[test]
    fn invalid_config_fails_closed() {
        let mut config = sample_config();
        config.channels[0].delay_ms = f32::NAN;
        assert!(matches!(
            generate_camilladsp_yaml(&config, Path::new("in.wav"), Path::new("out.wav")),
            Err(CamillaDspError::InvalidConfig(_))
        ));
    }

    #[test]
    fn missing_executable_returns_structured_error() {
        let missing = Path::new("Z:/definitely/not/camilladsp.exe");
        assert!(matches!(discover_camilladsp(Some(missing)), Err(CamillaDspError::MissingExecutable)));
    }

    #[test]
    fn command_construction_uses_executable_and_config_without_shell() {
        let command = build_camilladsp_command(Path::new("camilladsp"), Path::new("C:/tmp/camilladsp.yml"));
        assert_eq!(command.executable, PathBuf::from("camilladsp"));
        assert_eq!(command.args, vec![OsString::from("C:/tmp/camilladsp.yml")]);
    }

    #[test]
    fn streaming_wav_finalizer_rewrites_sizes_for_strict_parser() {
        let path = temp_wav_path("streaming_finalizer");
        write_wav_f32_with_channel_roles(
            &path,
            48_000,
            &[vec![0.0, 0.25, -0.25], vec![0.5, -0.5, 0.0]],
            &[ChannelRole::FrontLeft, ChannelRole::FrontRight],
        ).unwrap();
        let mut bytes = fs::read(&path).unwrap();
        bytes[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        let data_size_offset = find_chunk_size_offset(&bytes, b"data").unwrap();
        bytes[data_size_offset..data_size_offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        fs::write(&path, bytes).unwrap();

        let info = finalize_streaming_wav(&path).unwrap();
        let wav = read_wav(&path).unwrap();
        assert_eq!(info.sample_rate, 48_000);
        assert_eq!(info.channel_count, 2);
        assert_eq!(info.frame_count, 3);
        assert_eq!(wav.format.channel_count, 2);
        assert_eq!(wav.frame_count, 3);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn integration_processes_wav_when_camilladsp_is_installed() {
        let Ok(path) = std::env::var(ENV_CAMILLADSP_PATH) else { return; };
        let adapter = CamillaDspOfflineAdapter::discover(Some(Path::new(&path))).unwrap();
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).unwrap();
        let config = AuroraDspConfig::from_json_file(&workspace.join("fixtures/dsp/basic_5_1.json")).unwrap();
        let output = std::env::temp_dir().join("aurora_camilladsp_integration_processed.wav");
        let report = adapter.process_wav(
            &workspace.join("output/scene_5_1.wav"),
            &output,
            &config,
            false,
            Duration::from_secs(30),
        ).unwrap();
        let info = inspect_processed_wav(&output).unwrap();
        assert!(report.command.executable.is_file());
        assert_eq!(info.sample_rate, 48_000);
        assert_eq!(info.channel_count, 6);
        let _ = std::fs::remove_file(output);
    }

    fn temp_wav_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("aurora_camilladsp_{name}_{nonce}.wav"))
    }

    fn find_chunk_size_offset(bytes: &[u8], chunk_id: &[u8; 4]) -> Option<usize> {
        let mut offset = 12usize;
        while offset + 8 <= bytes.len() {
            let id = &bytes[offset..offset + 4];
            let size = u32::from_le_bytes([
                bytes[offset + 4], bytes[offset + 5], bytes[offset + 6], bytes[offset + 7],
            ]) as usize;
            if id == chunk_id { return Some(offset + 4); }
            offset += 8 + size + (size % 2);
        }
        None
    }
}
