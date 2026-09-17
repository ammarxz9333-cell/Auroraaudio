//! Coordinate transforms for Aurora head-pose quaternions.
//!
//! Aurora uses `+X` right, `+Y` front and `+Z` up. A [`UnitQuaternion`] represents
//! head-local orientation in world space: it rotates a head-local direction into world
//! coordinates. HRTF lookup normally needs the inverse operation, world-to-head.

use aurora_core::Vector3;
use thiserror::Error;

use crate::UnitQuaternion;

const DIRECTION_EPSILON: f32 = 1.0e-6;

/// Fail-closed direction-transform errors.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum HeadPoseTransformError {
    /// Direction contains NaN or infinity.
    #[error("head-pose direction must be finite")]
    NonFiniteDirection,
    /// Direction magnitude is too close to zero for HRTF lookup.
    #[error("head-pose direction must have nonzero magnitude")]
    ZeroDirection,
}

/// Rotates one head-local direction into Aurora world coordinates.
///
/// The input magnitude is intentionally discarded: HRTF direction and source distance are
/// separate concepts. The returned vector is normalized and dimensionless.
pub fn head_to_world_direction(
    orientation: UnitQuaternion,
    head_direction: Vector3,
) -> Result<Vector3, HeadPoseTransformError> {
    let direction = normalized(head_direction)?;
    normalized(rotate(
        orientation.w(),
        orientation.x(),
        orientation.y(),
        orientation.z(),
        direction,
    ))
}

/// Rotates one Aurora world direction into head-local coordinates for HRTF lookup.
///
/// This applies the inverse (conjugate) of the normalized head-to-world orientation.
/// The returned vector is normalized and dimensionless.
pub fn world_to_head_direction(
    orientation: UnitQuaternion,
    world_direction: Vector3,
) -> Result<Vector3, HeadPoseTransformError> {
    let direction = normalized(world_direction)?;
    normalized(rotate(
        orientation.w(),
        -orientation.x(),
        -orientation.y(),
        -orientation.z(),
        direction,
    ))
}

fn normalized(direction: Vector3) -> Result<Vector3, HeadPoseTransformError> {
    if !direction.x.is_finite() || !direction.y.is_finite() || !direction.z.is_finite() {
        return Err(HeadPoseTransformError::NonFiniteDirection);
    }
    let length = direction.length();
    if !length.is_finite() {
        return Err(HeadPoseTransformError::NonFiniteDirection);
    }
    if length <= DIRECTION_EPSILON {
        return Err(HeadPoseTransformError::ZeroDirection);
    }
    Ok(Vector3::new(
        direction.x / length,
        direction.y / length,
        direction.z / length,
    ))
}

fn rotate(w: f32, x: f32, y: f32, z: f32, vector: Vector3) -> Vector3 {
    let dot = x.mul_add(vector.x, y.mul_add(vector.y, z * vector.z));
    let axis_length_squared = x.mul_add(x, y.mul_add(y, z * z));
    let cross = Vector3::new(
        y * vector.z - z * vector.y,
        z * vector.x - x * vector.z,
        x * vector.y - y * vector.x,
    );
    let vector_scale = w * w - axis_length_squared;
    Vector3::new(
        2.0 * dot * x + vector_scale * vector.x + 2.0 * w * cross.x,
        2.0 * dot * y + vector_scale * vector.y + 2.0 * w * cross.y,
        2.0 * dot * z + vector_scale * vector.z + 2.0 * w * cross.z,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yaw_left_90() -> UnitQuaternion {
        let half = std::f32::consts::FRAC_PI_4;
        UnitQuaternion::try_new(half.cos(), 0.0, 0.0, half.sin()).unwrap()
    }

    fn assert_direction(actual: Vector3, expected: Vector3) {
        assert!((actual.x - expected.x).abs() < 1.0e-5);
        assert!((actual.y - expected.y).abs() < 1.0e-5);
        assert!((actual.z - expected.z).abs() < 1.0e-5);
    }

    #[test]
    fn identity_preserves_direction_and_normalizes_magnitude() {
        let direction = head_to_world_direction(
            UnitQuaternion::IDENTITY,
            Vector3::new(0.0, 4.0, 0.0),
        )
        .unwrap();
        assert_direction(direction, Vector3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn positive_z_yaw_turns_head_front_toward_world_left() {
        let world = head_to_world_direction(yaw_left_90(), Vector3::new(0.0, 1.0, 0.0))
            .unwrap();
        assert_direction(world, Vector3::new(-1.0, 0.0, 0.0));
    }

    #[test]
    fn world_front_moves_to_head_right_after_left_yaw() {
        let head = world_to_head_direction(yaw_left_90(), Vector3::new(0.0, 1.0, 0.0))
            .unwrap();
        assert_direction(head, Vector3::new(1.0, 0.0, 0.0));
    }

    #[test]
    fn head_world_roundtrip_preserves_arbitrary_direction() {
        let orientation = UnitQuaternion::try_new(0.8, 0.2, -0.3, 0.4).unwrap();
        let original = normalized(Vector3::new(2.0, -3.0, 1.5)).unwrap();
        let world = head_to_world_direction(orientation, original).unwrap();
        let recovered = world_to_head_direction(orientation, world).unwrap();
        assert_direction(recovered, original);
    }

    #[test]
    fn invalid_directions_fail_closed() {
        assert_eq!(
            world_to_head_direction(UnitQuaternion::IDENTITY, Vector3::ZERO),
            Err(HeadPoseTransformError::ZeroDirection)
        );
        assert_eq!(
            world_to_head_direction(
                UnitQuaternion::IDENTITY,
                Vector3::new(f32::NAN, 0.0, 1.0),
            ),
            Err(HeadPoseTransformError::NonFiniteDirection)
        );
    }
}
