//! Core data model for Aurora spatial-audio scenes and PCM blocks.

pub mod capability;

pub use capability::*;

use serde::{de::Error as DeError, Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// Audio sample representation used by an [`AudioFormat`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SampleType {
    /// Planar 32-bit floating-point samples.
    F32,
}

/// PCM stream description.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioFormat {
    /// Samples per second, such as `48000`.
    pub sample_rate: u32,
    /// Number of planar audio channels.
    pub channel_count: usize,
    /// Sample representation for each channel.
    pub sample_type: SampleType,
    /// Preferred processing block size in frames.
    pub block_size: usize,
}

/// Three-dimensional position or direction in meters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vector3 {
    /// Horizontal axis in meters.
    pub x: f32,
    /// Depth axis in meters.
    pub y: f32,
    /// Height axis in meters.
    pub z: f32,
}

impl Vector3 {
    /// Origin vector.
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);

    /// Creates a vector from meter-space components.
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// Returns Euclidean distance to another vector.
    pub fn distance_to(self, other: Self) -> f32 {
        (self - other).length()
    }

    /// Returns Euclidean vector length.
    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    /// Returns squared Euclidean vector length.
    pub fn length_squared(self) -> f32 {
        self.x
            .mul_add(self.x, self.y.mul_add(self.y, self.z * self.z))
    }
}

impl std::ops::Sub for Vector3 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

/// A physical output speaker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Speaker {
    /// Stable speaker identifier used by renderers and fixtures.
    pub id: String,
    /// Human-readable speaker label.
    pub label: String,
    /// Explicit semantic output channel role.
    pub channel_role: ChannelRole,
    /// Speaker position in meters.
    pub position: Vector3,
    /// Speaker forward orientation vector.
    pub orientation: Vector3,
    /// Static speaker trim in decibels.
    pub gain_db: f32,
    /// Static speaker delay in samples.
    pub delay_samples: f32,
    /// Whether the speaker participates in rendering.
    pub enabled: bool,
}

/// Semantic speaker/channel role used for deterministic output ordering.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChannelRole {
    /// Front left channel.
    FrontLeft,
    /// Front right channel.
    FrontRight,
    /// Front center channel.
    FrontCenter,
    /// Low-frequency effects channel.
    LowFrequencyEffects,
    /// Surround left channel.
    SurroundLeft,
    /// Surround right channel.
    SurroundRight,
    /// Surround back left channel.
    SurroundBackLeft,
    /// Surround back right channel.
    SurroundBackRight,
    /// Top front left height channel.
    TopFrontLeft,
    /// Top front right height channel.
    TopFrontRight,
    /// Top rear/back left height channel.
    TopRearLeft,
    /// Top rear/back right height channel.
    TopRearRight,
    /// Wide left surround channel.
    WideLeft,
    /// Wide right surround channel.
    WideRight,
    /// Top side left height channel.
    TopSideLeft,
    /// Top side right height channel.
    TopSideRight,
    /// Extensible role for custom layouts.
    Custom(String),
}

impl ChannelRole {
    /// Returns the stable fixture string for a channel role.
    pub fn as_str(&self) -> &str {
        match self {
            Self::FrontLeft => "front-left",
            Self::FrontRight => "front-right",
            Self::FrontCenter => "front-center",
            Self::LowFrequencyEffects => "low-frequency-effects",
            Self::SurroundLeft => "surround-left",
            Self::SurroundRight => "surround-right",
            Self::SurroundBackLeft => "surround-back-left",
            Self::SurroundBackRight => "surround-back-right",
            Self::TopFrontLeft => "top-front-left",
            Self::TopFrontRight => "top-front-right",
            Self::TopRearLeft => "top-rear-left",
            Self::TopRearRight => "top-rear-right",
            Self::WideLeft => "wide-left",
            Self::WideRight => "wide-right",
            Self::TopSideLeft => "top-side-left",
            Self::TopSideRight => "top-side-right",
            Self::Custom(value) => value,
        }
    }

    /// Returns the Windows speaker mask bit for roles that have a standard bit.
    pub fn wav_channel_mask_bit(&self) -> Option<u32> {
        match self {
            Self::FrontLeft => Some(0x1),
            Self::FrontRight => Some(0x2),
            Self::FrontCenter => Some(0x4),
            Self::LowFrequencyEffects => Some(0x8),
            Self::SurroundBackLeft => Some(0x10),
            Self::SurroundBackRight => Some(0x20),
            Self::SurroundLeft => Some(0x200),
            Self::SurroundRight => Some(0x400),
            Self::TopFrontLeft => Some(0x1000),
            Self::TopFrontRight => Some(0x4000),
            Self::TopRearLeft => Some(0x8000),
            Self::TopRearRight => Some(0x20000),
            Self::WideLeft => Some(0x40),
            Self::WideRight => Some(0x80),
            Self::TopSideLeft => Some(0x800),
            Self::TopSideRight => Some(0x2000),
            Self::Custom(_) => None,
        }
    }
}

impl std::fmt::Display for ChannelRole {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for ChannelRole {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ChannelRole {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(match value.as_str() {
            "front-left" | "FL" => Self::FrontLeft,
            "front-right" | "FR" => Self::FrontRight,
            "front-center" | "FC" => Self::FrontCenter,
            "low-frequency-effects" | "lfe" | "LFE" => Self::LowFrequencyEffects,
            "surround-left" | "SL" => Self::SurroundLeft,
            "surround-right" | "SR" => Self::SurroundRight,
            "surround-back-left" | "SBL" => Self::SurroundBackLeft,
            "surround-back-right" | "SBR" => Self::SurroundBackRight,
            "top-front-left" | "TFL" => Self::TopFrontLeft,
            "top-front-right" | "TFR" => Self::TopFrontRight,
            "top-rear-left" | "top-back-left" | "TRL" | "TBL" => Self::TopRearLeft,
            "top-rear-right" | "top-back-right" | "TRR" | "TBR" => Self::TopRearRight,
            "wide-left" | "WL" => Self::WideLeft,
            "wide-right" | "WR" => Self::WideRight,
            "top-side-left" | "TSL" => Self::TopSideLeft,
            "top-side-right" | "TSR" => Self::TopSideRight,
            "" => return Err(D::Error::custom("channel role must not be empty")),
            _ => Self::Custom(value),
        })
    }
}

/// Canonical standard output layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StandardLayout {
    /// Stereo: FL, FR.
    #[serde(rename = "stereo")]
    Stereo,
    /// 5.1: FL, FR, FC, LFE, SL, SR.
    #[serde(rename = "5.1")]
    FiveOne,
    /// 7.1: FL, FR, FC, LFE, SL, SR, SBL, SBR.
    #[serde(rename = "7.1")]
    SevenOne,
    /// 5.1.2: FL, FR, FC, LFE, SL, SR, TFL, TFR.
    #[serde(rename = "5.1.2")]
    FiveOneTwo,
    /// 7.1.4: FL, FR, FC, LFE, SL, SR, SBL, SBR, TFL, TFR, TRL, TRR.
    #[serde(rename = "7.1.4")]
    SevenOneFour,
    /// 11.1.4: FL, FR, FC, LFE, SL, SR, SBL, SBR, WL, WR, TFL, TFR, TRL, TRR, TSL, TSR.
    #[serde(rename = "11.1.4")]
    ElevenOneFour,
    /// Custom layout with fixture-defined ordering.
    #[serde(rename = "custom")]
    Custom,
}

impl StandardLayout {
    /// Returns the canonical Aurora channel order for a standard layout.
    pub fn canonical_roles(self) -> &'static [ChannelRole] {
        match self {
            Self::Stereo => &[ChannelRole::FrontLeft, ChannelRole::FrontRight],
            Self::FiveOne => &[
                ChannelRole::FrontLeft,
                ChannelRole::FrontRight,
                ChannelRole::FrontCenter,
                ChannelRole::LowFrequencyEffects,
                ChannelRole::SurroundLeft,
                ChannelRole::SurroundRight,
            ],
            Self::SevenOne => &[
                ChannelRole::FrontLeft,
                ChannelRole::FrontRight,
                ChannelRole::FrontCenter,
                ChannelRole::LowFrequencyEffects,
                ChannelRole::SurroundLeft,
                ChannelRole::SurroundRight,
                ChannelRole::SurroundBackLeft,
                ChannelRole::SurroundBackRight,
            ],
            Self::FiveOneTwo => &[
                ChannelRole::FrontLeft,
                ChannelRole::FrontRight,
                ChannelRole::FrontCenter,
                ChannelRole::LowFrequencyEffects,
                ChannelRole::SurroundLeft,
                ChannelRole::SurroundRight,
                ChannelRole::TopFrontLeft,
                ChannelRole::TopFrontRight,
            ],
            Self::SevenOneFour => &[
                ChannelRole::FrontLeft,
                ChannelRole::FrontRight,
                ChannelRole::FrontCenter,
                ChannelRole::LowFrequencyEffects,
                ChannelRole::SurroundLeft,
                ChannelRole::SurroundRight,
                ChannelRole::SurroundBackLeft,
                ChannelRole::SurroundBackRight,
                ChannelRole::TopFrontLeft,
                ChannelRole::TopFrontRight,
                ChannelRole::TopRearLeft,
                ChannelRole::TopRearRight,
            ],
            Self::ElevenOneFour => &[
                ChannelRole::FrontLeft,
                ChannelRole::FrontRight,
                ChannelRole::FrontCenter,
                ChannelRole::LowFrequencyEffects,
                ChannelRole::SurroundLeft,
                ChannelRole::SurroundRight,
                ChannelRole::SurroundBackLeft,
                ChannelRole::SurroundBackRight,
                ChannelRole::WideLeft,
                ChannelRole::WideRight,
                ChannelRole::TopFrontLeft,
                ChannelRole::TopFrontRight,
                ChannelRole::TopRearLeft,
                ChannelRole::TopRearRight,
                ChannelRole::TopSideLeft,
                ChannelRole::TopSideRight,
            ],
            Self::Custom => &[],
        }
    }

    /// Returns true for deterministic standard layouts.
    pub fn is_standard(self) -> bool {
        !matches!(self, Self::Custom)
    }
}

/// Listener pose and ear height.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Listener {
    /// Listener position in meters.
    pub position: Vector3,
    /// Listener forward orientation vector.
    pub orientation: Vector3,
    /// Ear height above floor in meters.
    pub ear_height: f32,
}

/// A renderable audio object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioObject {
    /// Stable object identifier.
    pub id: String,
    /// Object position in meters.
    pub position: Vector3,
    /// Object velocity in meters per second.
    pub velocity: Vector3,
    /// Object gain in decibels.
    pub gain_db: f32,
    /// Spatial spread from `0.0` to `1.0`.
    pub spread: f32,
    /// Optional object start time in seconds.
    pub start_time_seconds: Option<f64>,
    /// Optional object end time in seconds.
    pub end_time_seconds: Option<f64>,
}

/// Planar PCM audio block with presentation metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioBlock {
    /// Planar channel samples.
    pub channels: Vec<Vec<f32>>,
    /// Number of frames in each channel.
    pub frame_count: usize,
    /// Presentation timestamp in seconds.
    pub presentation_time_seconds: f64,
    /// Whether this block starts after a stream discontinuity.
    pub discontinuity: bool,
}

/// Full render scene description.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    /// Room dimensions in meters.
    pub room_dimensions: Vector3,
    /// Listener pose.
    pub listener: Listener,
    /// Speaker layout.
    pub speakers: Vec<Speaker>,
    /// Audio objects active in the scene.
    pub objects: Vec<AudioObject>,
}

/// Errors returned by core validation helpers.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CoreError {
    /// An audio block channel length does not match the declared frame count.
    #[error("audio block channel {channel} has {actual} frames, expected {expected}")]
    InvalidAudioBlock {
        /// Channel index with the invalid length.
        channel: usize,
        /// Actual sample count found in the channel.
        actual: usize,
        /// Expected sample count from `frame_count`.
        expected: usize,
    },
}

impl AudioBlock {
    /// Verifies that every planar channel has exactly `frame_count` samples.
    pub fn validate(&self) -> Result<(), CoreError> {
        for (channel, samples) in self.channels.iter().enumerate() {
            if samples.len() != self.frame_count {
                return Err(CoreError::InvalidAudioBlock {
                    channel,
                    actual: samples.len(),
                    expected: self.frame_count,
                });
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seven_one_four_has_canonical_twelve_channel_order() {
        assert_eq!(StandardLayout::SevenOneFour.canonical_roles().len(), 12);
        assert_eq!(
            StandardLayout::SevenOneFour.canonical_roles(),
            &[
                ChannelRole::FrontLeft,
                ChannelRole::FrontRight,
                ChannelRole::FrontCenter,
                ChannelRole::LowFrequencyEffects,
                ChannelRole::SurroundLeft,
                ChannelRole::SurroundRight,
                ChannelRole::SurroundBackLeft,
                ChannelRole::SurroundBackRight,
                ChannelRole::TopFrontLeft,
                ChannelRole::TopFrontRight,
                ChannelRole::TopRearLeft,
                ChannelRole::TopRearRight,
            ]
        );
    }

    #[test]
    fn eleven_one_four_has_canonical_sixteen_channel_order() {
        assert_eq!(StandardLayout::ElevenOneFour.canonical_roles().len(), 16);
        assert_eq!(
            StandardLayout::ElevenOneFour.canonical_roles(),
            &[
                ChannelRole::FrontLeft,
                ChannelRole::FrontRight,
                ChannelRole::FrontCenter,
                ChannelRole::LowFrequencyEffects,
                ChannelRole::SurroundLeft,
                ChannelRole::SurroundRight,
                ChannelRole::SurroundBackLeft,
                ChannelRole::SurroundBackRight,
                ChannelRole::WideLeft,
                ChannelRole::WideRight,
                ChannelRole::TopFrontLeft,
                ChannelRole::TopFrontRight,
                ChannelRole::TopRearLeft,
                ChannelRole::TopRearRight,
                ChannelRole::TopSideLeft,
                ChannelRole::TopSideRight,
            ]
        );
    }

    #[test]
    fn top_rear_roles_roundtrip_stably() {
        let left = serde_json::to_string(&ChannelRole::TopRearLeft).unwrap();
        let right = serde_json::to_string(&ChannelRole::TopRearRight).unwrap();
        assert_eq!(left, "\"top-rear-left\"");
        assert_eq!(right, "\"top-rear-right\"");
        assert_eq!(
            serde_json::from_str::<ChannelRole>("\"TBL\"").unwrap(),
            ChannelRole::TopRearLeft
        );
        assert_eq!(
            serde_json::from_str::<ChannelRole>("\"TBR\"").unwrap(),
            ChannelRole::TopRearRight
        );
    }
}
