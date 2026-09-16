#![forbid(unsafe_code)]
//! Native Configuration v4 speaker-layout assembly.
//!
//! This crate converts already-validated elevation-aware Aurora configuration
//! into the same immutable prepared layout contracts used by the existing
//! runtime assembly. It performs no I/O, probing, renderer construction, or
//! realtime work.

use aurora_config::{
    LayoutKindV4, SpeakerConfiguration, SpeakerLayoutConfigurationV4, ValidatedConfigurationV4,
};
use aurora_core::{ChannelRole, StandardLayout, Vector3};
use aurora_runtime_assembly::{
    PreparedLayoutKind, PreparedLayoutPlan, PreparedSpeaker, RuntimeInvariant,
    RuntimePreparationError,
};

/// Derives one immutable prepared speaker layout from validated Configuration v4 intent.
pub fn prepare_layout_v4(
    configuration: &ValidatedConfigurationV4,
) -> Result<PreparedLayoutPlan, RuntimePreparationError> {
    prepare_speaker_layout_v4(&configuration.config().speaker_layout)
}

/// Derives a prepared layout directly from an already-validated v4 layout value.
///
/// Callers normally use [`prepare_layout_v4`]. This lower-level entry point is
/// useful for assembly composition after validation has already established the
/// v4 invariants.
pub fn prepare_speaker_layout_v4(
    layout: &SpeakerLayoutConfigurationV4,
) -> Result<PreparedLayoutPlan, RuntimePreparationError> {
    let kind = match layout.kind {
        LayoutKindV4::Stereo => PreparedLayoutKind::Standard(StandardLayout::Stereo),
        LayoutKindV4::Surround51 => PreparedLayoutKind::Standard(StandardLayout::FiveOne),
        LayoutKindV4::Surround71 => PreparedLayoutKind::Standard(StandardLayout::SevenOne),
        LayoutKindV4::Surround714 => {
            PreparedLayoutKind::Standard(StandardLayout::SevenOneFour)
        }
        LayoutKindV4::CustomHorizontal => PreparedLayoutKind::CustomHorizontal,
    };

    let mut speakers = layout
        .speakers
        .iter()
        .map(prepare_speaker)
        .collect::<Result<Vec<_>, _>>()?;

    if layout.kind == LayoutKindV4::Surround714 {
        speakers.sort_by(|left, right| {
            canonical_714_rank(left.channel_role())
                .cmp(&canonical_714_rank(right.channel_role()))
                .then_with(|| left.id().cmp(right.id()))
        });
    }

    PreparedLayoutPlan::new(kind, speakers)
}

fn prepare_speaker(
    speaker: &SpeakerConfiguration,
) -> Result<PreparedSpeaker, RuntimePreparationError> {
    PreparedSpeaker::new(
        speaker.id.clone(),
        speaker.label.clone(),
        prepare_role(&speaker.role),
        spherical_unit_direction(
            speaker.azimuth_degrees,
            speaker.elevation_degrees.unwrap_or(0.0),
        )?,
        speaker.active,
    )
}

fn prepare_role(role: &str) -> ChannelRole {
    match role {
        "FL" | "front-left" => ChannelRole::FrontLeft,
        "FR" | "front-right" => ChannelRole::FrontRight,
        "FC" | "front-center" => ChannelRole::FrontCenter,
        "LFE" | "lfe" | "low-frequency-effects" => ChannelRole::LowFrequencyEffects,
        "SL" | "surround-left" => ChannelRole::SurroundLeft,
        "SR" | "surround-right" => ChannelRole::SurroundRight,
        "SBL" | "surround-back-left" => ChannelRole::SurroundBackLeft,
        "SBR" | "surround-back-right" => ChannelRole::SurroundBackRight,
        "TFL" | "top-front-left" => ChannelRole::TopFrontLeft,
        "TFR" | "top-front-right" => ChannelRole::TopFrontRight,
        "TRL" | "TBL" | "top-rear-left" | "top-back-left" => ChannelRole::TopRearLeft,
        "TRR" | "TBR" | "top-rear-right" | "top-back-right" => ChannelRole::TopRearRight,
        custom => ChannelRole::Custom(custom.to_owned()),
    }
}

fn canonical_714_rank(role: &ChannelRole) -> usize {
    StandardLayout::SevenOneFour
        .canonical_roles()
        .iter()
        .position(|canonical| canonical == role)
        .unwrap_or(usize::MAX)
}

/// Converts clockwise-positive azimuth and positive-up elevation into a unit vector.
///
/// Aurora uses `+X` right, `+Y` front, and `+Z` up. Therefore 0° azimuth points
/// forward, +90° points right, and +90° elevation points straight up.
fn spherical_unit_direction(
    azimuth_degrees: f32,
    elevation_degrees: f32,
) -> Result<Vector3, RuntimePreparationError> {
    if !azimuth_degrees.is_finite() || !elevation_degrees.is_finite() {
        return Err(RuntimePreparationError::InternalInvariantViolation {
            invariant: RuntimeInvariant::NonFiniteSpeakerGeometry,
        });
    }

    let azimuth = azimuth_degrees.to_radians();
    let elevation = elevation_degrees.to_radians();
    let elevation_cos = elevation.cos();
    let vector = Vector3::new(
        canonical_axis(elevation_cos * azimuth.sin()),
        canonical_axis(elevation_cos * azimuth.cos()),
        canonical_axis(elevation.sin()),
    );

    if !vector.x.is_finite() || !vector.y.is_finite() || !vector.z.is_finite() {
        return Err(RuntimePreparationError::InternalInvariantViolation {
            invariant: RuntimeInvariant::NonFiniteSpeakerGeometry,
        });
    }
    Ok(vector)
}

fn canonical_axis(value: f32) -> f32 {
    if value.abs() < 1.0e-6 {
        0.0
    } else if (value - 1.0).abs() < 1.0e-6 {
        1.0
    } else if (value + 1.0).abs() < 1.0e-6 {
        -1.0
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_config::migrate_v3_to_v4;

    const SURROUND_714: &[u8] = include_bytes!("../../../fixtures/config/surround-7-1-4-v4.json");
    const STEREO_V3: &[u8] = include_bytes!("../../../fixtures/config/stereo-basic-v3.json");

    #[test]
    fn native_7_1_4_is_canonical_and_three_dimensional() {
        let configuration = ValidatedConfigurationV4::from_json(SURROUND_714).unwrap();
        let plan = prepare_layout_v4(&configuration).unwrap();

        assert_eq!(
            plan.kind(),
            PreparedLayoutKind::Standard(StandardLayout::SevenOneFour)
        );
        let actual_roles = plan
            .speakers()
            .iter()
            .map(|speaker| speaker.channel_role().clone())
            .collect::<Vec<_>>();
        assert_eq!(actual_roles, StandardLayout::SevenOneFour.canonical_roles());

        for speaker in plan.speakers() {
            let position = speaker.position();
            let length = position.length();
            assert!((length - 1.0).abs() < 1.0e-5);
            match speaker.channel_role() {
                ChannelRole::TopFrontLeft
                | ChannelRole::TopFrontRight
                | ChannelRole::TopRearLeft
                | ChannelRole::TopRearRight => assert!(position.z > 0.0),
                _ => assert_eq!(position.z, 0.0),
            }
        }
    }

    #[test]
    fn canonical_height_geometry_has_expected_quadrants() {
        let configuration = ValidatedConfigurationV4::from_json(SURROUND_714).unwrap();
        let plan = prepare_layout_v4(&configuration).unwrap();

        let tfl = plan
            .speakers()
            .iter()
            .find(|speaker| speaker.channel_role() == &ChannelRole::TopFrontLeft)
            .unwrap()
            .position();
        let trr = plan
            .speakers()
            .iter()
            .find(|speaker| speaker.channel_role() == &ChannelRole::TopRearRight)
            .unwrap()
            .position();

        assert!(tfl.x < 0.0 && tfl.y > 0.0 && tfl.z > 0.0);
        assert!(trr.x > 0.0 && trr.y < 0.0 && trr.z > 0.0);
    }

    #[test]
    fn migrated_horizontal_layout_remains_on_zero_elevation_plane() {
        let migrated = migrate_v3_to_v4(STEREO_V3).unwrap();
        let plan = prepare_layout_v4(&migrated.configuration).unwrap();

        assert_eq!(
            plan.kind(),
            PreparedLayoutKind::Standard(StandardLayout::Stereo)
        );
        assert!(plan
            .speakers()
            .iter()
            .all(|speaker| speaker.position().z == 0.0));
    }

    #[test]
    fn spherical_axes_follow_aurora_coordinates() {
        assert_eq!(spherical_unit_direction(0.0, 0.0).unwrap(), Vector3::new(0.0, 1.0, 0.0));
        assert_eq!(spherical_unit_direction(90.0, 0.0).unwrap(), Vector3::new(1.0, 0.0, 0.0));
        assert_eq!(spherical_unit_direction(-90.0, 0.0).unwrap(), Vector3::new(-1.0, 0.0, 0.0));
        assert_eq!(spherical_unit_direction(0.0, 90.0).unwrap(), Vector3::new(0.0, 0.0, 1.0));
    }
}
