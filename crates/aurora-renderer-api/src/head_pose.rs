//! Allocation-free head-pose timeline primitives for binaural renderers.
//!
//! Tracker-specific clocks, device I/O, calibration and SOFA/HRTF lookup live outside
//! this module. Control-plane code maps accepted tracker samples onto Aurora's logical
//! media-frame timeline; realtime code only resolves already prepared poses.

const QUATERNION_EPSILON: f32 = 1.0e-6;
const SLERP_LINEAR_THRESHOLD: f32 = 0.9995;

/// A finite normalized quaternion representing listener/head orientation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitQuaternion {
    w: f32,
    x: f32,
    y: f32,
    z: f32,
}

impl UnitQuaternion {
    /// Identity orientation.
    pub const IDENTITY: Self = Self {
        w: 1.0,
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    /// Normalizes a finite nonzero quaternion.
    pub fn try_new(w: f32, x: f32, y: f32, z: f32) -> Result<Self, HeadPoseError> {
        if !w.is_finite() || !x.is_finite() || !y.is_finite() || !z.is_finite() {
            return Err(HeadPoseError::InvalidQuaternion);
        }
        let norm_squared = w.mul_add(w, x.mul_add(x, y.mul_add(y, z * z)));
        if !norm_squared.is_finite() || norm_squared <= QUATERNION_EPSILON * QUATERNION_EPSILON {
            return Err(HeadPoseError::InvalidQuaternion);
        }
        let inverse_norm = norm_squared.sqrt().recip();
        Ok(Self {
            w: w * inverse_norm,
            x: x * inverse_norm,
            y: y * inverse_norm,
            z: z * inverse_norm,
        })
    }

    /// Scalar component.
    pub const fn w(self) -> f32 {
        self.w
    }

    /// X component.
    pub const fn x(self) -> f32 {
        self.x
    }

    /// Y component.
    pub const fn y(self) -> f32 {
        self.y
    }

    /// Z component.
    pub const fn z(self) -> f32 {
        self.z
    }

    fn negated(self) -> Self {
        Self {
            w: -self.w,
            x: -self.x,
            y: -self.y,
            z: -self.z,
        }
    }

    fn dot(self, other: Self) -> f32 {
        self.w.mul_add(
            other.w,
            self.x
                .mul_add(other.x, self.y.mul_add(other.y, self.z * other.z)),
        )
    }

    /// Shortest-arc spherical interpolation with a bounded `0.0..=1.0` fraction.
    pub fn slerp(self, mut other: Self, fraction: f32) -> Result<Self, HeadPoseError> {
        if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
            return Err(HeadPoseError::InvalidInterpolationFraction);
        }
        let mut dot = self.dot(other).clamp(-1.0, 1.0);
        if dot < 0.0 {
            other = other.negated();
            dot = -dot;
        }

        if dot > SLERP_LINEAR_THRESHOLD {
            return Self::try_new(
                self.w + (other.w - self.w) * fraction,
                self.x + (other.x - self.x) * fraction,
                self.y + (other.y - self.y) * fraction,
                self.z + (other.z - self.z) * fraction,
            );
        }

        let theta = dot.acos();
        let sin_theta = theta.sin();
        if sin_theta.abs() <= QUATERNION_EPSILON {
            return Ok(self);
        }
        let left = ((1.0 - fraction) * theta).sin() / sin_theta;
        let right = (fraction * theta).sin() / sin_theta;
        Self::try_new(
            self.w * left + other.w * right,
            self.x * left + other.x * right,
            self.y * left + other.y * right,
            self.z * left + other.z * right,
        )
    }
}

/// One tracker sample already mapped onto Aurora's logical media-frame timeline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeadPoseSample {
    /// Monotonic tracker/source sequence. It proves freshness; orientation may remain unchanged.
    pub sequence: u64,
    /// Aurora logical media frame represented by this sample.
    pub media_frame: u64,
    /// Head orientation at `media_frame`.
    pub orientation: UnitQuaternion,
}

/// Bounds for interpolation and short pose holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeadPosePolicy {
    max_interpolation_gap_frames: u64,
    max_stale_frames: u64,
}

impl HeadPosePolicy {
    /// Creates nonzero interpolation and stale/hold budgets.
    pub fn new(
        max_interpolation_gap_frames: u64,
        max_stale_frames: u64,
    ) -> Result<Self, HeadPoseError> {
        if max_interpolation_gap_frames == 0 || max_stale_frames == 0 {
            return Err(HeadPoseError::InvalidPolicy);
        }
        Ok(Self {
            max_interpolation_gap_frames,
            max_stale_frames,
        })
    }

    /// Largest source-sample gap across which SLERP is permitted.
    pub const fn max_interpolation_gap_frames(self) -> u64 {
        self.max_interpolation_gap_frames
    }

    /// Largest age for which the most recent pose may be held.
    pub const fn max_stale_frames(self) -> u64 {
        self.max_stale_frames
    }
}

/// How a resolved orientation was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadPoseResolution {
    /// Target frame exactly matched a source sample.
    Exact,
    /// Target frame was interpolated between two accepted samples.
    Interpolated,
    /// Latest sample was held forward within the explicit stale budget.
    Held,
}

/// Allocation-free pose result returned to a renderer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedHeadPose {
    /// Resolved normalized orientation.
    pub orientation: UnitQuaternion,
    /// Resolution mode used for this target frame.
    pub resolution: HeadPoseResolution,
}

/// Two-sample fixed-storage timeline window for realtime head-pose resolution.
#[derive(Debug, Clone, Copy)]
pub struct HeadPoseState {
    policy: HeadPosePolicy,
    previous: Option<HeadPoseSample>,
    current: Option<HeadPoseSample>,
}

impl HeadPoseState {
    /// Creates an empty fixed-storage state.
    pub const fn new(policy: HeadPosePolicy) -> Self {
        Self {
            policy,
            previous: None,
            current: None,
        }
    }

    /// Accepts one newer mapped sample at a control/block boundary.
    ///
    /// Sequence and media frame must both advance. Repeated orientation values are valid:
    /// a stationary listener is not treated as a frozen tracker while freshness advances.
    pub fn commit(&mut self, sample: HeadPoseSample) -> Result<(), HeadPoseError> {
        if let Some(current) = self.current {
            if sample.sequence <= current.sequence {
                return Err(HeadPoseError::NonMonotonicSequence {
                    previous: current.sequence,
                    actual: sample.sequence,
                });
            }
            if sample.media_frame <= current.media_frame {
                return Err(HeadPoseError::NonMonotonicMediaFrame {
                    previous: current.media_frame,
                    actual: sample.media_frame,
                });
            }
            self.previous = Some(current);
        }
        self.current = Some(sample);
        Ok(())
    }

    /// Resolves the orientation for one Aurora logical media frame without allocation.
    pub fn resolve(&self, target_frame: u64) -> Result<ResolvedHeadPose, HeadPoseError> {
        let current = self.current.ok_or(HeadPoseError::MissingPose)?;

        if target_frame == current.media_frame {
            return Ok(ResolvedHeadPose {
                orientation: current.orientation,
                resolution: HeadPoseResolution::Exact,
            });
        }

        if target_frame > current.media_frame {
            let age = target_frame - current.media_frame;
            if age > self.policy.max_stale_frames {
                return Err(HeadPoseError::StalePose {
                    age_frames: age,
                    maximum_frames: self.policy.max_stale_frames,
                });
            }
            return Ok(ResolvedHeadPose {
                orientation: current.orientation,
                resolution: HeadPoseResolution::Held,
            });
        }

        let previous = self.previous.ok_or(HeadPoseError::BeforePreparedWindow)?;
        if target_frame < previous.media_frame {
            return Err(HeadPoseError::BeforePreparedWindow);
        }
        if target_frame == previous.media_frame {
            return Ok(ResolvedHeadPose {
                orientation: previous.orientation,
                resolution: HeadPoseResolution::Exact,
            });
        }

        let gap = current.media_frame - previous.media_frame;
        if gap > self.policy.max_interpolation_gap_frames {
            return Err(HeadPoseError::InterpolationGapTooLarge {
                gap_frames: gap,
                maximum_frames: self.policy.max_interpolation_gap_frames,
            });
        }
        let offset = target_frame - previous.media_frame;
        let fraction = offset as f32 / gap as f32;
        Ok(ResolvedHeadPose {
            orientation: previous.orientation.slerp(current.orientation, fraction)?,
            resolution: HeadPoseResolution::Interpolated,
        })
    }

    /// Most recently accepted sample, if any.
    pub const fn current(self) -> Option<HeadPoseSample> {
        self.current
    }
}

/// Fail-closed head-pose preparation/resolution errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadPoseError {
    /// Quaternion was nonfinite or too close to zero length.
    InvalidQuaternion,
    /// SLERP fraction was nonfinite or outside `0..=1`.
    InvalidInterpolationFraction,
    /// Interpolation or hold budget was zero.
    InvalidPolicy,
    /// No pose has been accepted yet.
    MissingPose,
    /// Target frame predates the two-sample prepared window.
    BeforePreparedWindow,
    /// Tracker/source sequence did not strictly advance.
    NonMonotonicSequence { previous: u64, actual: u64 },
    /// Mapped Aurora media frame did not strictly advance.
    NonMonotonicMediaFrame { previous: u64, actual: u64 },
    /// Two accepted poses are too far apart to interpolate safely.
    InterpolationGapTooLarge {
        gap_frames: u64,
        maximum_frames: u64,
    },
    /// Latest pose is older than the explicit hold budget.
    StalePose {
        age_frames: u64,
        maximum_frames: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yaw_90() -> UnitQuaternion {
        let half = std::f32::consts::FRAC_PI_4;
        UnitQuaternion::try_new(half.cos(), 0.0, 0.0, half.sin()).unwrap()
    }

    fn sample(sequence: u64, media_frame: u64, orientation: UnitQuaternion) -> HeadPoseSample {
        HeadPoseSample {
            sequence,
            media_frame,
            orientation,
        }
    }

    #[test]
    fn quaternion_normalization_rejects_invalid_values() {
        assert_eq!(
            UnitQuaternion::try_new(0.0, 0.0, 0.0, 0.0),
            Err(HeadPoseError::InvalidQuaternion)
        );
        assert_eq!(
            UnitQuaternion::try_new(f32::NAN, 0.0, 0.0, 1.0),
            Err(HeadPoseError::InvalidQuaternion)
        );
        let q = UnitQuaternion::try_new(2.0, 0.0, 0.0, 0.0).unwrap();
        assert!((q.w() - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn slerp_uses_shortest_arc_and_hits_expected_half_yaw() {
        let halfway = UnitQuaternion::IDENTITY.slerp(yaw_90(), 0.5).unwrap();
        let expected = std::f32::consts::FRAC_PI_8;
        assert!((halfway.w() - expected.cos()).abs() < 1.0e-5);
        assert!((halfway.z() - expected.sin()).abs() < 1.0e-5);

        let negative_identity = UnitQuaternion::try_new(-1.0, 0.0, 0.0, 0.0).unwrap();
        let same = UnitQuaternion::IDENTITY
            .slerp(negative_identity, 0.5)
            .unwrap();
        assert!((same.w().abs() - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn timeline_interpolates_exactly_and_holds_within_stale_budget() {
        let policy = HeadPosePolicy::new(480, 240).unwrap();
        let mut state = HeadPoseState::new(policy);
        state
            .commit(sample(1, 1_000, UnitQuaternion::IDENTITY))
            .unwrap();
        state.commit(sample(2, 1_240, yaw_90())).unwrap();

        assert_eq!(
            state.resolve(1_000).unwrap().resolution,
            HeadPoseResolution::Exact
        );
        assert_eq!(
            state.resolve(1_120).unwrap().resolution,
            HeadPoseResolution::Interpolated
        );
        assert_eq!(
            state.resolve(1_300).unwrap().resolution,
            HeadPoseResolution::Held
        );
        assert_eq!(
            state.resolve(1_481),
            Err(HeadPoseError::StalePose {
                age_frames: 241,
                maximum_frames: 240,
            })
        );
    }

    #[test]
    fn large_interpolation_gap_fails_closed() {
        let policy = HeadPosePolicy::new(100, 50).unwrap();
        let mut state = HeadPoseState::new(policy);
        state
            .commit(sample(1, 1_000, UnitQuaternion::IDENTITY))
            .unwrap();
        state.commit(sample(2, 1_200, yaw_90())).unwrap();
        assert_eq!(
            state.resolve(1_100),
            Err(HeadPoseError::InterpolationGapTooLarge {
                gap_frames: 200,
                maximum_frames: 100,
            })
        );
    }

    #[test]
    fn frozen_or_reordered_source_is_rejected_by_freshness_not_orientation() {
        let policy = HeadPosePolicy::new(256, 256).unwrap();
        let mut state = HeadPoseState::new(policy);
        state
            .commit(sample(7, 1_000, UnitQuaternion::IDENTITY))
            .unwrap();

        assert_eq!(
            state.commit(sample(7, 1_100, UnitQuaternion::IDENTITY)),
            Err(HeadPoseError::NonMonotonicSequence {
                previous: 7,
                actual: 7,
            })
        );
        assert_eq!(
            state.commit(sample(8, 1_000, UnitQuaternion::IDENTITY)),
            Err(HeadPoseError::NonMonotonicMediaFrame {
                previous: 1_000,
                actual: 1_000,
            })
        );

        state
            .commit(sample(8, 1_100, UnitQuaternion::IDENTITY))
            .unwrap();
        assert_eq!(state.current().unwrap().sequence, 8);
    }

    #[test]
    fn before_window_and_missing_pose_are_explicit() {
        let policy = HeadPosePolicy::new(256, 256).unwrap();
        let mut state = HeadPoseState::new(policy);
        assert_eq!(state.resolve(0), Err(HeadPoseError::MissingPose));
        state
            .commit(sample(1, 100, UnitQuaternion::IDENTITY))
            .unwrap();
        assert_eq!(state.resolve(99), Err(HeadPoseError::BeforePreparedWindow));
    }
}
