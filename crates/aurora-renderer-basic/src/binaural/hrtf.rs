//! Control-thread direction selection over already decoded, delay-baked HRTF FIRs.
//! No SOFA parser or reference library is linked into the runtime.

pub mod scheduler;

use aurora_core::Vector3;
use aurora_renderer_api::{
    world_to_head_direction, HeadPoseError, HeadPoseState, HeadPoseTransformError, UnitQuaternion,
};

use super::{Error, Filters, Input};

/// One externally decoded measurement in canonical SOFA Cartesian coordinates:
/// +X front, +Y left, +Z up. ListenerView/ListenerUp must already be canonicalized.
#[derive(Debug)]
pub struct SofaMeasurement {
    /// Dimensionless direction; distance dependence is not represented by this bank.
    pub direction: Vector3,
    /// Left taps followed by right taps, with all delays baked into the coefficients.
    pub coefficients: Vec<f32>,
}

/// Preparation rejects malformed banks, uncovered directions and unavailable poses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreparationError {
    /// Invalid bank dimensions, duplicate directions, or angular tolerance.
    Contract,
    /// Existing FIR validation failed.
    Filter(Error),
    /// Pose missing, stale, or outside the prepared interpolation window.
    Pose(HeadPoseError),
    /// Invalid world or measurement direction.
    Direction(HeadPoseTransformError),
    /// Nearest measurement exceeds the caller's explicit angular error budget.
    UncoveredDirection,
}

/// Canonical SOFA direction to Aurora (+X right, +Y front, +Z up).
pub fn sofa_to_aurora_direction(direction: Vector3) -> Result<Vector3, PreparationError> {
    world_to_head_direction(
        UnitQuaternion::IDENTITY,
        Vector3::new(-direction.y, direction.x, direction.z),
    )
    .map_err(PreparationError::Direction)
}

/// Immutable, bounded nearest-direction bank prepared exclusively on the control thread.
/// Selection is nearest neighbour, without spatial interpolation or distance attenuation.
#[derive(Debug)]
pub struct DirectionalHrtf {
    sample_rate: u32,
    taps: usize,
    minimum_dot: f32,
    directions: Vec<Vector3>,
    responses: Vec<Filters>,
}

impl DirectionalHrtf {
    /// Validates at most 4096 measurements. Angular error is explicit in radians,
    /// strictly positive and no greater than pi. Ties choose the first measurement.
    /// Parsing, sample-rate conversion and delay baking must happen before this call.
    pub fn prepare(
        sample_rate: u32,
        taps: usize,
        maximum_angle_radians: f32,
        measurements: Vec<SofaMeasurement>,
    ) -> Result<Self, PreparationError> {
        if measurements.is_empty()
            || measurements.len() > 4096
            || !maximum_angle_radians.is_finite()
            || maximum_angle_radians <= 0.0
            || maximum_angle_radians > std::f32::consts::PI
        {
            return Err(PreparationError::Contract);
        }
        let mut directions: Vec<Vector3> = Vec::with_capacity(measurements.len());
        let mut responses = Vec::with_capacity(measurements.len());
        for measurement in measurements {
            let direction = sofa_to_aurora_direction(measurement.direction)?;
            if directions.iter().any(|other| {
                (other.x - direction.x).abs() <= 1e-6
                    && (other.y - direction.y).abs() <= 1e-6
                    && (other.z - direction.z).abs() <= 1e-6
            }) {
                return Err(PreparationError::Contract);
            }
            let response = Filters::prepare(
                Input::Objects(1),
                sample_rate,
                1,
                taps,
                measurement.coefficients,
            )
            .map_err(PreparationError::Filter)?;
            directions.push(direction);
            responses.push(response);
        }
        Ok(Self {
            sample_rate,
            taps,
            minimum_dot: maximum_angle_radians.cos(),
            directions,
            responses,
        })
    }

    /// Resolves the pose at an explicit media frame, transforms stable PCM-channel-order
    /// world directions, and builds a complete candidate transactionally. Allocates on
    /// the control thread. Failure never touches an active renderer. Generation is owned
    /// by the caller; `PreparedBinaural::commit` rejects stale candidates at the boundary.
    /// This prepares one snapshot, not a continuously tracked callback or a scheduler.
    pub fn prepare_objects(
        &self,
        poses: &HeadPoseState,
        target_frame: u64,
        world_directions: &[Vector3],
        generation: u64,
    ) -> Result<Filters, PreparationError> {
        let input = Input::Objects(world_directions.len());
        input.channels().map_err(PreparationError::Filter)?;
        if generation == 0 {
            return Err(PreparationError::Filter(Error::Contract));
        }
        let pose = poses
            .resolve(target_frame)
            .map_err(PreparationError::Pose)?;
        let mut coefficients = Vec::with_capacity(world_directions.len() * 2 * self.taps);
        for &world in world_directions {
            let head = world_to_head_direction(pose.orientation, world)
                .map_err(PreparationError::Direction)?;
            let mut best_index = 0;
            let mut best_dot = f32::NEG_INFINITY;
            for (index, direction) in self.directions.iter().enumerate() {
                let dot = head.x * direction.x + head.y * direction.y + head.z * direction.z;
                if dot > best_dot {
                    best_dot = dot;
                    best_index = index;
                }
            }
            // Permit only unit-vector rounding error at the angular boundary.
            if best_dot + 4.0 * f32::EPSILON < self.minimum_dot {
                return Err(PreparationError::UncoveredDirection);
            }
            coefficients.extend_from_slice(&self.responses[best_index].coefficients);
        }
        Filters::prepare(input, self.sample_rate, generation, self.taps, coefficients)
            .map_err(PreparationError::Filter)
    }
}
