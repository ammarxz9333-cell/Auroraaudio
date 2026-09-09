use std::collections::BTreeMap;

use aurora_core::Vector3;
use thiserror::Error;

use crate::spatial_ir::{
    BedSignalBinding, CoordinateSpace, SpatialDecodedFrame, SpatialObjectUpdate, SpatialPosition,
};

/// Explicit transform from codec-normalized room coordinates into Aurora meter space.
///
/// `RoomNormalized` x/y values map from `0..=1` into `min..=max`; z maps from
/// `-1..=1` into the same meter-space bounds. Keeping the transform explicit
/// prevents a codec adapter from silently inventing physical room dimensions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoomTransform {
    pub min: Vector3,
    pub max: Vector3,
}

impl RoomTransform {
    pub fn new(min: Vector3, max: Vector3) -> Result<Self, SceneTimelineError> {
        let transform = Self { min, max };
        transform.validate()?;
        Ok(transform)
    }

    pub fn from_dimensions(dimensions: Vector3) -> Result<Self, SceneTimelineError> {
        Self::new(Vector3::ZERO, dimensions)
    }

    fn validate(self) -> Result<(), SceneTimelineError> {
        if !finite_vector(self.min)
            || !finite_vector(self.max)
            || self.max.x <= self.min.x
            || self.max.y <= self.min.y
            || self.max.z <= self.min.z
        {
            return Err(SceneTimelineError::InvalidRoomTransform);
        }
        Ok(())
    }

    fn normalized_to_meters(self, x: f32, y: f32, z: f32) -> Vector3 {
        Vector3::new(
            lerp(self.min.x, self.max.x, x),
            lerp(self.min.y, self.max.y, y),
            lerp(self.min.z, self.max.z, (z + 1.0) * 0.5),
        )
    }
}

/// Codec-neutral render state at one point on an object trajectory.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectRenderState {
    pub position_meters: Vector3,
    pub gain_db: f32,
    pub spread: f32,
    pub priority: Option<f32>,
}

/// One analytic object trajectory segment valid for a render span.
///
/// `transition_*` are the boundaries of this already-resolved segment, not the
/// original codec ramp. `from` and `to` are therefore exact endpoint states for
/// the same interval. This prevents a partial codec ramp from being interpolated
/// a second time after the scene planner has clipped it to an access-unit span.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectRenderCurve {
    pub object_id: String,
    pub pcm_channel_index: usize,
    pub transition_start_absolute_sample: u64,
    pub transition_end_absolute_sample: u64,
    pub from: ObjectRenderState,
    pub to: ObjectRenderState,
}

impl ObjectRenderCurve {
    pub fn state_at_absolute_sample(&self, sample: u64) -> ObjectRenderState {
        if self.transition_end_absolute_sample <= self.transition_start_absolute_sample
            || sample >= self.transition_end_absolute_sample
        {
            return self.to.clone();
        }
        if sample <= self.transition_start_absolute_sample {
            return self.from.clone();
        }
        let numerator = sample - self.transition_start_absolute_sample;
        let denominator = self.transition_end_absolute_sample - self.transition_start_absolute_sample;
        let t = numerator as f32 / denominator as f32;
        interpolate_state(&self.from, &self.to, t)
    }
}

/// Render interval over which the active object set and each object's analytic
/// transition are stable. A renderer may subdivide this span without reparsing
/// codec metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneSpan {
    pub start_sample_offset: u32,
    pub end_sample_offset: u32,
    pub objects: Vec<ObjectRenderCurve>,
}

/// Complete codec-neutral render plan for one decoded access unit.
#[derive(Debug, Clone, PartialEq)]
pub struct SpatialRenderPlan {
    pub absolute_start_sample: u64,
    pub frame_count: usize,
    pub bed_signals: Vec<BedSignalBinding>,
    pub spans: Vec<SceneSpan>,
}

#[derive(Debug, Clone)]
struct RuntimeObject {
    active: bool,
    curve: Option<RuntimeCurve>,
}

#[derive(Debug, Clone)]
struct RuntimeCurve {
    start_absolute_sample: u64,
    end_absolute_sample: u64,
    from: ObjectRenderState,
    to: ObjectRenderState,
}

impl RuntimeCurve {
    fn state_at(&self, sample: u64) -> ObjectRenderState {
        if self.end_absolute_sample <= self.start_absolute_sample || sample >= self.end_absolute_sample {
            return self.to.clone();
        }
        if sample <= self.start_absolute_sample {
            return self.from.clone();
        }
        let numerator = sample - self.start_absolute_sample;
        let denominator = self.end_absolute_sample - self.start_absolute_sample;
        interpolate_state(&self.from, &self.to, numerator as f32 / denominator as f32)
    }
}

/// Stateful scene assembler bridging Spatial IR into renderer-ready time spans.
#[derive(Debug, Default, Clone)]
pub struct SceneTimeline {
    absolute_sample_cursor: u64,
    objects: BTreeMap<String, RuntimeObject>,
}

impl SceneTimeline {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn absolute_sample_cursor(&self) -> u64 {
        self.absolute_sample_cursor
    }

    pub fn reset(&mut self) {
        self.absolute_sample_cursor = 0;
        self.objects.clear();
    }

    /// Convert one validated Spatial IR frame into deterministic render spans.
    pub fn plan_frame(
        &mut self,
        frame: &SpatialDecodedFrame,
        room: RoomTransform,
    ) -> Result<SpatialRenderPlan, SceneTimelineError> {
        room.validate()?;
        frame
            .validate()
            .map_err(|error| SceneTimelineError::InvalidSpatialIr(error.to_string()))?;

        if frame.decoded.audio.discontinuity {
            self.objects.clear();
        }

        let frame_count_u32 = u32::try_from(frame.decoded.audio.frame_count)
            .map_err(|_| SceneTimelineError::FrameTooLarge)?;
        let frame_start = self.absolute_sample_cursor;
        let frame_end = frame_start
            .checked_add(frame.decoded.audio.frame_count as u64)
            .ok_or(SceneTimelineError::SampleClockOverflow)?;

        let signal_lanes = frame
            .spatial
            .object_signals
            .iter()
            .map(|signal| (signal.id.as_str(), signal.pcm_channel_index))
            .collect::<BTreeMap<_, _>>();

        let mut updates = frame.spatial.object_updates.iter().collect::<Vec<_>>();
        updates.sort_by_key(|update| update.metadata_sample_offset);
        let mut update_index = 0usize;
        let mut local_cursor = 0u32;
        let mut spans = Vec::new();

        loop {
            while update_index < updates.len()
                && updates[update_index].metadata_sample_offset == local_cursor
            {
                self.apply_update(
                    updates[update_index],
                    frame_start + u64::from(local_cursor),
                    room,
                )?;
                update_index += 1;
            }

            if local_cursor == frame_count_u32 {
                break;
            }

            let next_update = updates
                .get(update_index)
                .map(|update| update.metadata_sample_offset)
                .unwrap_or(frame_count_u32);
            let next_ramp_end = self
                .objects
                .values()
                .filter(|object| object.active)
                .filter_map(|object| object.curve.as_ref())
                .filter_map(|curve| {
                    if curve.end_absolute_sample > frame_start + u64::from(local_cursor)
                        && curve.end_absolute_sample < frame_end
                    {
                        u32::try_from(curve.end_absolute_sample - frame_start).ok()
                    } else {
                        None
                    }
                })
                .min()
                .unwrap_or(frame_count_u32);
            let next_boundary = next_update.min(next_ramp_end).min(frame_count_u32);
            if next_boundary <= local_cursor {
                return Err(SceneTimelineError::NonAdvancingTimeline);
            }

            let objects = self.snapshot_curves(
                frame_start + u64::from(local_cursor),
                frame_start + u64::from(next_boundary),
                &signal_lanes,
            )?;
            spans.push(SceneSpan {
                start_sample_offset: local_cursor,
                end_sample_offset: next_boundary,
                objects,
            });
            local_cursor = next_boundary;
        }

        self.absolute_sample_cursor = frame_end;
        Ok(SpatialRenderPlan {
            absolute_start_sample: frame_start,
            frame_count: frame.decoded.audio.frame_count,
            bed_signals: frame.spatial.bed_signals.clone(),
            spans,
        })
    }

    fn apply_update(
        &mut self,
        update: &SpatialObjectUpdate,
        absolute_sample: u64,
        room: RoomTransform,
    ) -> Result<(), SceneTimelineError> {
        let target = update_to_state(update, room)?;
        let entry = self
            .objects
            .entry(update.object_id.clone())
            .or_insert(RuntimeObject {
                active: false,
                curve: None,
            });

        if !update.active {
            entry.active = false;
            entry.curve = None;
            return Ok(());
        }

        let was_active = entry.active;
        let from = if was_active {
            entry
                .curve
                .as_ref()
                .map(|curve| curve.state_at(absolute_sample))
                .unwrap_or_else(|| target.clone())
        } else {
            target.clone()
        };
        let end_absolute_sample = if was_active && update.ramp_duration_samples > 0 {
            absolute_sample
                .checked_add(u64::from(update.ramp_duration_samples))
                .ok_or(SceneTimelineError::SampleClockOverflow)?
        } else {
            absolute_sample
        };
        entry.active = true;
        entry.curve = Some(RuntimeCurve {
            start_absolute_sample: absolute_sample,
            end_absolute_sample,
            from,
            to: target,
        });
        Ok(())
    }

    fn snapshot_curves(
        &self,
        span_start: u64,
        span_end: u64,
        signal_lanes: &BTreeMap<&str, usize>,
    ) -> Result<Vec<ObjectRenderCurve>, SceneTimelineError> {
        let mut result = Vec::new();
        for (id, object) in &self.objects {
            if !object.active {
                continue;
            }
            let lane = signal_lanes
                .get(id.as_str())
                .copied()
                .ok_or_else(|| SceneTimelineError::ActiveObjectMissingSignal { id: id.clone() })?;
            let curve = object
                .curve
                .as_ref()
                .ok_or_else(|| SceneTimelineError::ActiveObjectMissingState { id: id.clone() })?;
            result.push(ObjectRenderCurve {
                object_id: id.clone(),
                pcm_channel_index: lane,
                transition_start_absolute_sample: span_start,
                transition_end_absolute_sample: span_end,
                from: curve.state_at(span_start),
                to: curve.state_at(span_end),
            });
        }
        Ok(result)
    }
}

#[derive(Debug, Error, Clone, PartialEq)]
pub enum SceneTimelineError {
    #[error("room transform must contain finite, strictly increasing bounds")]
    InvalidRoomTransform,
    #[error("Spatial IR rejected before scene assembly: {0}")]
    InvalidSpatialIr(String),
    #[error("decoded frame is too large for 32-bit metadata offsets")]
    FrameTooLarge,
    #[error("scene sample clock overflowed")]
    SampleClockOverflow,
    #[error("scene timeline failed to advance")]
    NonAdvancingTimeline,
    #[error("active object '{id}' has no PCM signal binding in this frame")]
    ActiveObjectMissingSignal { id: String },
    #[error("active object '{id}' has no resolved render state")]
    ActiveObjectMissingState { id: String },
    #[error("spherical coordinates require finite azimuth/distance, elevation -90..90 degrees, and non-negative distance")]
    InvalidSphericalCoordinates,
    #[error("RoomNormalized coordinates are outside the admitted x/y=0..1, z=-1..1 bounds")]
    NormalizedCoordinatesOutOfRange,
}

fn update_to_state(
    update: &SpatialObjectUpdate,
    room: RoomTransform,
) -> Result<ObjectRenderState, SceneTimelineError> {
    let position_meters = match (update.coordinate_space, update.position) {
        (
            CoordinateSpace::AuroraMeters,
            SpatialPosition::Cartesian { x, y, z },
        ) => Vector3::new(x, y, z),
        (
            CoordinateSpace::RoomNormalized,
            SpatialPosition::Cartesian { x, y, z },
        ) => {
            if !(0.0..=1.0).contains(&x)
                || !(0.0..=1.0).contains(&y)
                || !(-1.0..=1.0).contains(&z)
            {
                return Err(SceneTimelineError::NormalizedCoordinatesOutOfRange);
            }
            room.normalized_to_meters(x, y, z)
        }
        (
            CoordinateSpace::SphericalDegrees,
            SpatialPosition::Spherical {
                azimuth_degrees,
                elevation_degrees,
                distance,
            },
        ) => spherical_to_meters(azimuth_degrees, elevation_degrees, distance)?,
        _ => {
            return Err(SceneTimelineError::InvalidSpatialIr(
                "coordinate representation changed after Spatial IR validation".into(),
            ))
        }
    };
    Ok(ObjectRenderState {
        position_meters,
        gain_db: update.gain_db,
        spread: update.spread,
        priority: update.priority,
    })
}

/// Listener-relative spherical convention shared by MPEG-H/CICP and Aurora:
/// azimuth 0° points forward (+Y), positive azimuth points left (-X), negative
/// azimuth points right (+X), and positive elevation points upward (+Z).
fn spherical_to_meters(
    azimuth_degrees: f32,
    elevation_degrees: f32,
    distance: f32,
) -> Result<Vector3, SceneTimelineError> {
    if !azimuth_degrees.is_finite()
        || !elevation_degrees.is_finite()
        || !distance.is_finite()
        || !(-90.0..=90.0).contains(&elevation_degrees)
        || distance < 0.0
    {
        return Err(SceneTimelineError::InvalidSphericalCoordinates);
    }
    let azimuth = azimuth_degrees.to_radians();
    let elevation = elevation_degrees.to_radians();
    let horizontal = elevation.cos() * distance;
    Ok(Vector3::new(
        -azimuth.sin() * horizontal,
        azimuth.cos() * horizontal,
        elevation.sin() * distance,
    ))
}

fn interpolate_state(
    from: &ObjectRenderState,
    to: &ObjectRenderState,
    t: f32,
) -> ObjectRenderState {
    let t = t.clamp(0.0, 1.0);
    ObjectRenderState {
        position_meters: Vector3::new(
            lerp(from.position_meters.x, to.position_meters.x, t),
            lerp(from.position_meters.y, to.position_meters.y, t),
            lerp(from.position_meters.z, to.position_meters.z, t),
        ),
        gain_db: lerp_gain_db(from.gain_db, to.gain_db, t),
        spread: lerp(from.spread, to.spread, t),
        priority: match (from.priority, to.priority) {
            (Some(a), Some(b)) => Some(lerp(a, b, t)),
            (_, target) => target,
        },
    }
}

fn lerp_gain_db(from: f32, to: f32, t: f32) -> f32 {
    match (from.is_finite(), to.is_finite()) {
        (true, true) => lerp(from, to, t),
        (false, false) => f32::NEG_INFINITY,
        (false, true) => {
            if t <= 0.0 { f32::NEG_INFINITY } else { to }
        }
        (true, false) => {
            if t >= 1.0 { f32::NEG_INFINITY } else { from }
        }
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn finite_vector(value: Vector3) -> bool {
    value.x.is_finite() && value.y.is_finite() && value.z.is_finite()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_core::AudioBlock;
    use aurora_decoder_api::DecodedFrame;

    use crate::spatial_ir::{
        ObjectSignalBinding, SpatialDomain, SpatialFrameMetadata,
    };

    fn room() -> RoomTransform {
        RoomTransform::from_dimensions(Vector3::new(4.0, 6.0, 3.0)).unwrap()
    }

    fn update(id: &str, active: bool, offset: u32, ramp: u32, x: f32) -> SpatialObjectUpdate {
        SpatialObjectUpdate {
            object_id: id.into(),
            active,
            coordinate_space: CoordinateSpace::RoomNormalized,
            position: SpatialPosition::Cartesian { x, y: 0.5, z: 0.0 },
            gain_db: 0.0,
            spread: 0.0,
            metadata_sample_offset: offset,
            ramp_duration_samples: ramp,
            priority: Some(1.0),
        }
    }

    fn spherical_update(
        id: &str,
        offset: u32,
        ramp: u32,
        azimuth_degrees: f32,
        elevation_degrees: f32,
        distance: f32,
    ) -> SpatialObjectUpdate {
        SpatialObjectUpdate {
            object_id: id.into(),
            active: true,
            coordinate_space: CoordinateSpace::SphericalDegrees,
            position: SpatialPosition::Spherical {
                azimuth_degrees,
                elevation_degrees,
                distance,
            },
            gain_db: 0.0,
            spread: 0.0,
            metadata_sample_offset: offset,
            ramp_duration_samples: ramp,
            priority: Some(1.0),
        }
    }

    fn frame(
        frame_count: usize,
        discontinuity: bool,
        updates: Vec<SpatialObjectUpdate>,
    ) -> SpatialDecodedFrame {
        SpatialDecodedFrame {
            decoded: DecodedFrame {
                audio: AudioBlock {
                    channels: vec![vec![0.0; frame_count]],
                    frame_count,
                    presentation_time_seconds: 0.0,
                    discontinuity,
                },
                objects: Vec::new(),
            },
            spatial: SpatialFrameMetadata {
                domain: SpatialDomain::ObjectSignals,
                bed_signals: Vec::new(),
                object_signals: vec![ObjectSignalBinding {
                    id: "object-0".into(),
                    pcm_channel_index: 0,
                }],
                object_updates: updates,
            },
        }
    }

    #[test]
    fn normalized_room_transform_is_explicit_and_bounded() {
        let transformed = room().normalized_to_meters(0.5, 1.0, 0.0);
        assert_eq!(transformed, Vector3::new(2.0, 6.0, 1.5));
    }

    #[test]
    fn spherical_coordinates_follow_mpegh_listener_axes() {
        let front = spherical_to_meters(0.0, 0.0, 2.0).unwrap();
        let left = spherical_to_meters(90.0, 0.0, 2.0).unwrap();
        let right = spherical_to_meters(-90.0, 0.0, 2.0).unwrap();
        let top = spherical_to_meters(0.0, 90.0, 2.0).unwrap();
        assert!((front.x - 0.0).abs() < 0.0001 && (front.y - 2.0).abs() < 0.0001);
        assert!((left.x + 2.0).abs() < 0.0001 && left.y.abs() < 0.0001);
        assert!((right.x - 2.0).abs() < 0.0001 && right.y.abs() < 0.0001);
        assert!((top.z - 2.0).abs() < 0.0001);
    }

    #[test]
    fn invalid_spherical_coordinates_fail_closed() {
        assert_eq!(
            spherical_to_meters(0.0, 91.0, 1.0),
            Err(SceneTimelineError::InvalidSphericalCoordinates)
        );
        assert_eq!(
            spherical_to_meters(0.0, 0.0, -1.0),
            Err(SceneTimelineError::InvalidSphericalCoordinates)
        );
    }

    #[test]
    fn spherical_object_uses_same_timeline_and_ramp_path() {
        let mut timeline = SceneTimeline::new();
        timeline
            .plan_frame(
                &frame(40, true, vec![spherical_update("object-0", 0, 0, 0.0, 0.0, 1.0)]),
                room(),
            )
            .unwrap();
        let plan = timeline
            .plan_frame(
                &frame(40, false, vec![spherical_update("object-0", 0, 40, 90.0, 0.0, 1.0)]),
                room(),
            )
            .unwrap();
        let curve = &plan.spans[0].objects[0];
        let halfway = curve.state_at_absolute_sample(plan.absolute_start_sample + 20);
        assert!((halfway.position_meters.x + 0.5).abs() < 0.0001);
        assert!((halfway.position_meters.y - 0.5).abs() < 0.0001);
    }

    #[test]
    fn activation_mid_frame_splits_scene_without_fabricating_early_object_audio() {
        let mut timeline = SceneTimeline::new();
        let plan = timeline
            .plan_frame(&frame(40, true, vec![update("object-0", true, 10, 0, 0.25)]), room())
            .unwrap();
        assert_eq!(plan.spans.len(), 2);
        assert_eq!((plan.spans[0].start_sample_offset, plan.spans[0].end_sample_offset), (0, 10));
        assert!(plan.spans[0].objects.is_empty());
        assert_eq!((plan.spans[1].start_sample_offset, plan.spans[1].end_sample_offset), (10, 40));
        assert_eq!(plan.spans[1].objects.len(), 1);
    }

    #[test]
    fn deactivation_mid_frame_removes_object_at_exact_metadata_offset() {
        let mut timeline = SceneTimeline::new();
        timeline
            .plan_frame(&frame(40, true, vec![update("object-0", true, 0, 0, 0.25)]), room())
            .unwrap();
        let plan = timeline
            .plan_frame(&frame(40, false, vec![update("object-0", false, 20, 0, 0.25)]), room())
            .unwrap();
        assert_eq!(plan.spans.len(), 2);
        assert_eq!(plan.spans[0].objects.len(), 1);
        assert!(plan.spans[1].objects.is_empty());
    }

    #[test]
    fn ramp_state_crosses_access_unit_boundary_without_resetting() {
        let mut timeline = SceneTimeline::new();
        timeline
            .plan_frame(&frame(40, true, vec![update("object-0", true, 0, 0, 0.0)]), room())
            .unwrap();
        let first = timeline
            .plan_frame(&frame(40, false, vec![update("object-0", true, 0, 80, 1.0)]), room())
            .unwrap();
        let second = timeline
            .plan_frame(&frame(40, false, Vec::new()), room())
            .unwrap();
        let first_curve = &first.spans[0].objects[0];
        let second_curve = &second.spans[0].objects[0];
        assert!((first_curve.from.position_meters.x - 0.0).abs() < 0.0001);
        assert!((first_curve.to.position_meters.x - 2.0).abs() < 0.0001);
        assert!((first_curve.state_at_absolute_sample(60).position_meters.x - 1.0).abs() < 0.0001);
        assert!((second_curve.from.position_meters.x - 2.0).abs() < 0.0001);
        assert!((second_curve.to.position_meters.x - 4.0).abs() < 0.0001);
        assert!((second_curve.state_at_absolute_sample(100).position_meters.x - 3.0).abs() < 0.0001);
    }
}
