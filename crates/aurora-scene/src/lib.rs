//! JSON scene loading and trajectory sampling for offline rendering.

use std::fs;
use std::path::Path;

use aurora_core::{AudioObject, ChannelRole, Listener, Speaker, StandardLayout, Vector3};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Offline render scene loaded from JSON fixtures.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderScene {
    /// Declared output layout for canonical ordering and validation.
    pub layout: StandardLayout,
    /// Listener pose.
    pub listener: Listener,
    /// Speaker layout to render into.
    pub speakers: Vec<Speaker>,
    /// Moving object definition.
    pub object: SceneObject,
    /// Object trajectory.
    pub trajectory: Trajectory,
    /// Preferred renderer block size.
    #[serde(default = "default_block_size")]
    pub block_size: usize,
}

impl RenderScene {
    /// Samples the scene object at a presentation time.
    pub fn object_at_time(&self, time_seconds: f64) -> AudioObject {
        let position = self.trajectory.position_at_time(time_seconds);
        AudioObject {
            id: self.object.id.clone(),
            position,
            velocity: Vector3::ZERO,
            gain_db: self.object.gain_db,
            spread: self.object.spread,
            start_time_seconds: None,
            end_time_seconds: None,
        }
    }

    /// Returns speakers in canonical output order for standard layouts.
    pub fn ordered_speakers(&self) -> Result<Vec<Speaker>, SceneError> {
        if self.layout == StandardLayout::Custom {
            return Ok(self.speakers.clone());
        }

        self.layout
            .canonical_roles()
            .iter()
            .map(|role| {
                self.speakers
                    .iter()
                    .find(|speaker| speaker.channel_role == *role)
                    .cloned()
                    .ok_or_else(|| SceneError::MissingChannelRole(role.clone()))
            })
            .collect()
    }
}

/// Static object properties supplied by a scene fixture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneObject {
    /// Stable object identifier.
    pub id: String,
    /// Object gain in decibels.
    #[serde(default)]
    pub gain_db: f32,
    /// Spatial spread from `0.0` to `1.0`.
    #[serde(default)]
    pub spread: f32,
}

/// Supported offline fixture trajectories.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Trajectory {
    /// Circular motion around the listener in the horizontal plane.
    Circle {
        /// Center of the circle in meters.
        center: Vector3,
        /// Radius in meters.
        radius: f32,
        /// Height in meters.
        z: f32,
        /// Starting angle in degrees.
        #[serde(default)]
        start_degrees: f32,
        /// Full rotations per second.
        revolutions_per_second: f32,
    },
}

impl Trajectory {
    /// Samples the trajectory at a presentation time.
    pub fn position_at_time(&self, time_seconds: f64) -> Vector3 {
        match self {
            Self::Circle {
                center,
                radius,
                z,
                start_degrees,
                revolutions_per_second,
            } => {
                let angle = start_degrees.to_radians()
                    + (time_seconds as f32) * revolutions_per_second * std::f32::consts::TAU;
                Vector3::new(
                    center.x + radius * angle.cos(),
                    center.y + radius * angle.sin(),
                    *z,
                )
            }
        }
    }
}

/// Errors returned by scene loading.
#[derive(Debug, Error)]
pub enum SceneError {
    /// File IO failed.
    #[error("scene io error: {0}")]
    Io(#[from] std::io::Error),
    /// JSON parsing failed.
    #[error("scene json error: {0}")]
    Json(#[from] serde_json::Error),
    /// Scene content is invalid.
    #[error("invalid scene: {0}")]
    Invalid(String),
    /// A standard layout is missing a required role.
    #[error("missing required channel role: {0}")]
    MissingChannelRole(ChannelRole),
    /// A standard layout contains a duplicate role.
    #[error("duplicate channel role: {0}")]
    DuplicateChannelRole(ChannelRole),
    /// Custom roles are not allowed in standard layouts.
    #[error("custom channel role is not allowed in standard layout: {0}")]
    CustomRoleInStandardLayout(String),
}

/// Loads and validates a render scene from JSON.
pub fn load_render_scene<P: AsRef<Path>>(path: P) -> Result<RenderScene, SceneError> {
    let json = fs::read_to_string(path)?;
    let scene = serde_json::from_str::<RenderScene>(&json)?;
    validate_scene(&scene)?;
    Ok(scene)
}

fn validate_scene(scene: &RenderScene) -> Result<(), SceneError> {
    if scene.block_size == 0 {
        return Err(SceneError::Invalid(
            "block_size must be greater than zero".to_owned(),
        ));
    }
    if !scene.speakers.iter().any(|speaker| speaker.enabled) {
        return Err(SceneError::Invalid(
            "at least one speaker must be enabled".to_owned(),
        ));
    }
    validate_roles(scene)?;
    Ok(())
}

fn validate_roles(scene: &RenderScene) -> Result<(), SceneError> {
    let mut seen = Vec::<ChannelRole>::new();
    for speaker in &scene.speakers {
        if let ChannelRole::Custom(value) = &speaker.channel_role {
            if scene.layout.is_standard() {
                return Err(SceneError::CustomRoleInStandardLayout(value.clone()));
            }
        }
        if seen.contains(&speaker.channel_role) {
            return Err(SceneError::DuplicateChannelRole(
                speaker.channel_role.clone(),
            ));
        }
        seen.push(speaker.channel_role.clone());
    }

    if scene.layout.is_standard() {
        for role in scene.layout.canonical_roles() {
            if !seen.contains(role) {
                return Err(SceneError::MissingChannelRole(role.clone()));
            }
        }
    }

    Ok(())
}

fn default_block_size() -> usize {
    256
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_trajectory_samples_expected_cardinal_positions() {
        let trajectory = Trajectory::Circle {
            center: Vector3::ZERO,
            radius: 1.0,
            z: 0.0,
            start_degrees: 0.0,
            revolutions_per_second: 1.0,
        };

        let start = trajectory.position_at_time(0.0);
        let quarter = trajectory.position_at_time(0.25);

        assert!((start.x - 1.0).abs() < 0.0001);
        assert!(start.y.abs() < 0.0001);
        assert!(quarter.x.abs() < 0.0001);
        assert!((quarter.y - 1.0).abs() < 0.0001);
    }

    #[test]
    fn bundled_scene_fixtures_cover_standard_layouts() {
        let stereo = serde_json::from_str::<RenderScene>(include_str!(
            "../../../fixtures/scenes/stereo_circle.json"
        ))
        .unwrap();
        let five_one = serde_json::from_str::<RenderScene>(include_str!(
            "../../../fixtures/scenes/circle.json"
        ))
        .unwrap();
        let seven_one = serde_json::from_str::<RenderScene>(include_str!(
            "../../../fixtures/scenes/circle_7_1.json"
        ))
        .unwrap();
        let five_one_two = serde_json::from_str::<RenderScene>(include_str!(
            "../../../fixtures/scenes/5_1_2_upfiring.json"
        ))
        .unwrap();

        assert_eq!(stereo.speakers.len(), 2);
        assert_eq!(five_one.speakers.len(), 6);
        assert_eq!(seven_one.speakers.len(), 8);
        assert_eq!(five_one_two.speakers.len(), 8);
    }

    #[test]
    fn canonical_channel_order_for_standard_layouts() {
        assert_eq!(
            StandardLayout::Stereo.canonical_roles(),
            &[ChannelRole::FrontLeft, ChannelRole::FrontRight]
        );
        assert_eq!(
            StandardLayout::FiveOne.canonical_roles(),
            &[
                ChannelRole::FrontLeft,
                ChannelRole::FrontRight,
                ChannelRole::FrontCenter,
                ChannelRole::LowFrequencyEffects,
                ChannelRole::SurroundLeft,
                ChannelRole::SurroundRight,
            ]
        );
        assert_eq!(
            StandardLayout::SevenOne.canonical_roles(),
            &[
                ChannelRole::FrontLeft,
                ChannelRole::FrontRight,
                ChannelRole::FrontCenter,
                ChannelRole::LowFrequencyEffects,
                ChannelRole::SurroundLeft,
                ChannelRole::SurroundRight,
                ChannelRole::SurroundBackLeft,
                ChannelRole::SurroundBackRight,
            ]
        );
        assert_eq!(
            StandardLayout::FiveOneTwo.canonical_roles(),
            &[
                ChannelRole::FrontLeft,
                ChannelRole::FrontRight,
                ChannelRole::FrontCenter,
                ChannelRole::LowFrequencyEffects,
                ChannelRole::SurroundLeft,
                ChannelRole::SurroundRight,
                ChannelRole::TopFrontLeft,
                ChannelRole::TopFrontRight,
            ]
        );
    }

    #[test]
    fn duplicate_channel_roles_are_rejected() {
        let mut scene = serde_json::from_str::<RenderScene>(include_str!(
            "../../../fixtures/scenes/stereo_circle.json"
        ))
        .unwrap();
        scene.speakers[1].channel_role = ChannelRole::FrontLeft;

        let error = validate_roles(&scene).unwrap_err();

        assert!(matches!(
            error,
            SceneError::DuplicateChannelRole(ChannelRole::FrontLeft)
        ));
    }

    #[test]
    fn missing_required_roles_are_rejected() {
        let mut scene = serde_json::from_str::<RenderScene>(include_str!(
            "../../../fixtures/scenes/stereo_circle.json"
        ))
        .unwrap();
        scene.speakers.pop();

        let error = validate_roles(&scene).unwrap_err();

        assert!(matches!(
            error,
            SceneError::MissingChannelRole(ChannelRole::FrontRight)
        ));
    }

    #[test]
    fn scene_vector_order_does_not_control_standard_output_order() {
        let mut scene = serde_json::from_str::<RenderScene>(include_str!(
            "../../../fixtures/scenes/circle.json"
        ))
        .unwrap();
        scene.speakers.reverse();

        let ordered_roles = scene
            .ordered_speakers()
            .unwrap()
            .into_iter()
            .map(|speaker| speaker.channel_role)
            .collect::<Vec<_>>();

        assert_eq!(ordered_roles, StandardLayout::FiveOne.canonical_roles());
    }

    #[test]
    fn custom_roles_are_allowed_only_in_custom_layouts() {
        let mut scene = serde_json::from_str::<RenderScene>(include_str!(
            "../../../fixtures/scenes/stereo_circle.json"
        ))
        .unwrap();
        scene.speakers[0].channel_role = ChannelRole::Custom("wide-left".to_owned());

        let error = validate_roles(&scene).unwrap_err();
        assert!(matches!(error, SceneError::CustomRoleInStandardLayout(_)));

        scene.layout = StandardLayout::Custom;
        validate_roles(&scene).unwrap();
    }
}
