//! Experimental three-dimensional vector-base amplitude panning.
//!
//! The renderer builds a deterministic triangulated convex hull around the
//! listener's acoustic head centre. It excludes LFE from spatial panning,
//! preserves unit spatial power before speaker trims, and falls back to the
//! nearest loudspeaker direction outside an open hull.

use std::cmp::Ordering;

use aurora_core::{ChannelRole, Listener, Speaker, Vector3};
use aurora_renderer_api::{
    RenderObject, Renderer, RendererError, RendererScratch, RendererScratchSize, SpeakerGain,
};

const GEOMETRY_EPSILON: f64 = 1.0e-8;
const FACET_EPSILON: f64 = 1.0e-7;
const GAIN_EPSILON: f64 = 1.0e-7;
const DEFAULT_SPEED_OF_SOUND_METERS_PER_SECOND: f64 = 343.0;
const MAX_SPATIAL_SPEAKERS: usize = 32;

/// One triangulated convex-hull loudspeaker base active for the prepared listener.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ValidatedTriplet {
    speaker_indices: [usize; 3],
    determinant_abs: f64,
    span_radians: f64,
}

impl ValidatedTriplet {
    /// Returns configured output indices participating in this triplet.
    pub const fn speaker_indices(&self) -> [usize; 3] {
        self.speaker_indices
    }

    /// Returns the absolute listener-relative direction determinant.
    pub const fn determinant_abs(&self) -> f64 {
        self.determinant_abs
    }

    /// Returns the largest pairwise angular separation in this triplet.
    pub const fn span_radians(&self) -> f64 {
        self.span_radians
    }
}

#[derive(Debug, Clone, Copy)]
struct SupportingPlane {
    normal: [f64; 3],
    offset: f64,
    has_strict_interior_side: bool,
}

/// Allocation-free steady-state 3D VBAP renderer.
///
/// `Listener::ear_height` is part of the acoustic listener position. This is
/// intentionally consistent with Aurora's geometric binaural renderer: the
/// head centre is `listener.position + (0, 0, ear_height)`.
///
/// The convex-hull topology is rebuilt when that acoustic position changes.
/// All buffers required for rebuilding are reserved during `configure`, so a
/// warmed-up render and a listener-position topology rebuild allocate no heap
/// memory.
#[derive(Debug, Clone)]
pub struct Vbap3dRenderer {
    layout: Vec<Speaker>,
    spatial_indices: Vec<usize>,
    direction_cache: Vec<[f64; 3]>,
    active_triplets: Vec<ValidatedTriplet>,
    face_vertices: Vec<usize>,
    sample_rate: u32,
    block_size: usize,
    max_objects: usize,
    smoothing_alpha: f32,
    previous_gains: Vec<f32>,
    prepared_listener_center: Option<Vector3>,
    prepared_topology_valid: bool,
    listener_inside_hull: bool,
    configured: bool,
}

impl Default for Vbap3dRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Vbap3dRenderer {
    /// Creates an unconfigured renderer with block smoothing disabled.
    pub fn new() -> Self {
        Self {
            layout: Vec::new(),
            spatial_indices: Vec::new(),
            direction_cache: Vec::new(),
            active_triplets: Vec::new(),
            face_vertices: Vec::new(),
            sample_rate: 48_000,
            block_size: 256,
            max_objects: 0,
            smoothing_alpha: 1.0,
            previous_gains: Vec::new(),
            prepared_listener_center: None,
            prepared_topology_valid: false,
            listener_inside_hull: false,
            configured: false,
        }
    }

    /// Sets block-to-block smoothing alpha, clamped to `0.0..=1.0`.
    ///
    /// A non-finite value selects the unsmoothed default `1.0`.
    pub fn with_smoothing(mut self, smoothing_alpha: f32) -> Self {
        self.smoothing_alpha = if smoothing_alpha.is_finite() {
            smoothing_alpha.clamp(0.0, 1.0)
        } else {
            1.0
        };
        self
    }

    /// Prepares listener-relative directions and the triangulated convex hull.
    ///
    /// This method is optional for normal rendering because `render_gains`
    /// prepares automatically. It is exposed so offline evaluation can inspect
    /// the exact topology before generating artifacts.
    pub fn prepare_listener(&mut self, listener: &Listener) -> Result<(), RendererError> {
        if !self.configured {
            return Err(RendererError::NotConfigured);
        }
        if self.prepare_listener_internal(listener) {
            Ok(())
        } else {
            Err(RendererError::InvalidConfiguration(
                "3D VBAP could not build a listener-relative loudspeaker hull".to_owned(),
            ))
        }
    }

    /// Returns the active listener-relative hull triplets.
    ///
    /// The slice is populated after `prepare_listener` or the first successful
    /// `render_gains` call for a listener position.
    pub fn validated_triplets(&self) -> &[ValidatedTriplet] {
        &self.active_triplets
    }

    /// Reports whether the prepared acoustic listener point is inside the hull.
    ///
    /// A closed hull guarantees that every source direction has a VBAP base.
    /// Open layouts remain usable through the deterministic nearest-direction
    /// fallback, but are not equivalent to full-sphere 3D coverage.
    pub const fn listener_inside_hull(&self) -> bool {
        self.listener_inside_hull
    }

    fn prepare_listener_internal(&mut self, listener: &Listener) -> bool {
        let Some(listener_center) = acoustic_listener_center(listener) else {
            self.invalidate_prepared_topology();
            return false;
        };
        if self.prepared_listener_center == Some(listener_center) {
            return self.prepared_topology_valid;
        }

        let mut directions_valid = true;
        for (index, speaker) in self.layout.iter().enumerate() {
            let direction = normalize(subtract(speaker.position, listener_center));
            if let Some(direction) = direction {
                self.direction_cache[index] = direction;
            } else {
                self.direction_cache[index] = [0.0; 3];
                if self.spatial_indices.contains(&index) {
                    directions_valid = false;
                }
            }
        }

        self.prepared_listener_center = Some(listener_center);
        if !directions_valid {
            self.active_triplets.clear();
            self.listener_inside_hull = false;
            self.prepared_topology_valid = false;
            return false;
        }

        self.rebuild_convex_hull_topology();
        self.prepared_topology_valid = !self.active_triplets.is_empty();
        self.prepared_topology_valid
    }

    fn invalidate_prepared_topology(&mut self) {
        self.prepared_listener_center = None;
        self.active_triplets.clear();
        self.listener_inside_hull = false;
        self.prepared_topology_valid = false;
        self.direction_cache.fill([0.0; 3]);
    }

    fn rebuild_convex_hull_topology(&mut self) {
        self.active_triplets.clear();
        self.listener_inside_hull = true;

        let spatial_count = self.spatial_indices.len();
        for first_position in 0..spatial_count - 2 {
            for second_position in first_position + 1..spatial_count - 1 {
                for third_position in second_position + 1..spatial_count {
                    let candidate = [
                        self.spatial_indices[first_position],
                        self.spatial_indices[second_position],
                        self.spatial_indices[third_position],
                    ];
                    let Some(plane) = supporting_plane(
                        candidate,
                        &self.direction_cache,
                        &self.spatial_indices,
                    ) else {
                        continue;
                    };

                    self.collect_coplanar_face_vertices(plane);
                    if self.face_vertices.len() < 3 {
                        continue;
                    }
                    self.face_vertices.sort_unstable_by(|first, second| {
                        compare_speakers(&self.layout, *first, *second)
                    });
                    let Some(trigger) = canonical_face_trigger(
                        &self.face_vertices,
                        &self.direction_cache,
                        &self.layout,
                    ) else {
                        continue;
                    };
                    if sorted_triplet_by_speaker_id(candidate, &self.layout) != trigger {
                        continue;
                    }

                    if !plane.has_strict_interior_side || plane.offset < -FACET_EPSILON {
                        self.listener_inside_hull = false;
                    }
                    self.triangulate_current_face(plane.normal);
                }
            }
        }

        if self.active_triplets.is_empty() {
            self.listener_inside_hull = false;
        }
    }

    fn collect_coplanar_face_vertices(&mut self, plane: SupportingPlane) {
        self.face_vertices.clear();
        for index in self.spatial_indices.iter().copied() {
            let distance = dot(plane.normal, self.direction_cache[index]) - plane.offset;
            if distance.abs() <= FACET_EPSILON {
                self.face_vertices.push(index);
            }
        }
    }

    fn triangulate_current_face(&mut self, normal: [f64; 3]) {
        if self.face_vertices.len() == 3 {
            self.push_validated_triplet([
                self.face_vertices[0],
                self.face_vertices[1],
                self.face_vertices[2],
            ]);
            return;
        }

        let mut centroid = [0.0; 3];
        for index in self.face_vertices.iter().copied() {
            centroid = add(centroid, self.direction_cache[index]);
        }
        centroid = scale(centroid, 1.0 / self.face_vertices.len() as f64);

        let mut basis_u = None;
        for index in self.face_vertices.iter().copied() {
            if let Some(candidate) = normalize(subtract_arrays(self.direction_cache[index], centroid))
            {
                basis_u = Some(candidate);
                break;
            }
        }
        let Some(basis_u) = basis_u else {
            return;
        };
        let Some(basis_v) = normalize(cross(normal, basis_u)) else {
            return;
        };

        self.face_vertices.sort_unstable_by(|first, second| {
            let first_relative = subtract_arrays(self.direction_cache[*first], centroid);
            let second_relative = subtract_arrays(self.direction_cache[*second], centroid);
            let first_angle = dot(first_relative, basis_v).atan2(dot(first_relative, basis_u));
            let second_angle =
                dot(second_relative, basis_v).atan2(dot(second_relative, basis_u));
            first_angle
                .total_cmp(&second_angle)
                .then_with(|| compare_speakers(&self.layout, *first, *second))
        });

        let Some(anchor_position) = self
            .face_vertices
            .iter()
            .enumerate()
            .min_by(|(_, first), (_, second)| compare_speakers(&self.layout, **first, **second))
            .map(|(position, _)| position)
        else {
            return;
        };
        let vertex_count = self.face_vertices.len();
        let anchor = self.face_vertices[anchor_position];
        for offset in 1..vertex_count - 1 {
            let second = self.face_vertices[(anchor_position + offset) % vertex_count];
            let third = self.face_vertices[(anchor_position + offset + 1) % vertex_count];
            self.push_validated_triplet([anchor, second, third]);
        }
    }

    fn push_validated_triplet(&mut self, indices: [usize; 3]) {
        let first = self.direction_cache[indices[0]];
        let second = self.direction_cache[indices[1]];
        let third = self.direction_cache[indices[2]];
        let determinant_abs = determinant(first, second, third).abs();
        if determinant_abs <= GEOMETRY_EPSILON || !determinant_abs.is_finite() {
            return;
        }
        self.active_triplets.push(ValidatedTriplet {
            speaker_indices: indices,
            determinant_abs,
            span_radians: maximum_pair_angle(first, second, third),
        });
    }

    fn render_object(
        &mut self,
        listener_center: Option<Vector3>,
        object_index: usize,
        object: RenderObject,
        weights: &mut [f32],
        output: &mut [SpeakerGain],
    ) -> Result<(), RendererError> {
        weights.fill(0.0);
        let runtime_input_is_finite = listener_center.is_some()
            && vector_is_finite(object.position)
            && object.gain.is_finite();

        if let Some(listener_center) = listener_center.filter(|_| runtime_input_is_finite) {
            let source_delta = subtract(object.position, listener_center);
            let source_length = length(source_delta);
            if source_length <= GEOMETRY_EPSILON {
                equal_power_spatial(weights, &self.spatial_indices);
            } else {
                let source_direction = scale(source_delta, 1.0 / source_length);
                if !self.select_triplet(source_direction, weights) {
                    nearest_direction(
                        &self.layout,
                        &self.spatial_indices,
                        source_direction,
                        &self.direction_cache,
                        weights,
                    );
                }
            }
        }

        let speaker_count = self.layout.len();
        let history_len = self.previous_gains.len();
        for (speaker_index, ((speaker, weight), result)) in self
            .layout
            .iter()
            .zip(weights.iter())
            .zip(output.iter_mut())
            .enumerate()
        {
            let target = if runtime_input_is_finite {
                finite_f32(*weight as f64 * db_to_gain(speaker.gain_db) * object.gain as f64)
            } else {
                0.0
            };
            let history_index = object_index * speaker_count + speaker_index;
            let previous_gain = self.previous_gains.get_mut(history_index).ok_or(
                RendererError::OutputBufferSize {
                    required: self.max_objects * speaker_count,
                    actual: history_len,
                },
            )?;
            let smoothed = if runtime_input_is_finite {
                finite_f32(
                    *previous_gain as f64
                        + (target as f64 - *previous_gain as f64) * self.smoothing_alpha as f64,
                )
            } else {
                0.0
            };
            *previous_gain = smoothed;
            let distance = if runtime_input_is_finite {
                distance(object.position, speaker.position)
            } else {
                0.0
            };
            *result = SpeakerGain {
                speaker_index,
                gain: smoothed,
                distance_meters: finite_f32(distance),
                delay_samples: finite_f32(
                    speaker.delay_samples as f64
                        + distance / DEFAULT_SPEED_OF_SOUND_METERS_PER_SECOND
                            * self.sample_rate as f64,
                ),
            };
        }
        Ok(())
    }

    fn select_triplet(&self, source_direction: [f64; 3], weights: &mut [f32]) -> bool {
        for triplet in &self.active_triplets {
            let [first, second, third] = triplet.speaker_indices;
            let Some(mut gains) = solve_triplet(
                self.direction_cache[first],
                self.direction_cache[second],
                self.direction_cache[third],
                source_direction,
            ) else {
                continue;
            };
            if gains.iter().any(|gain| *gain < -GAIN_EPSILON) {
                continue;
            }
            for gain in &mut gains {
                *gain = gain.max(0.0);
            }
            let norm = length(gains);
            if norm <= GEOMETRY_EPSILON || !norm.is_finite() {
                continue;
            }
            weights[first] = finite_f32(gains[0] / norm);
            weights[second] = finite_f32(gains[1] / norm);
            weights[third] = finite_f32(gains[2] / norm);
            return true;
        }
        false
    }

    fn validate_render_buffers(
        &self,
        object_count: usize,
        output_count: usize,
        scratch: &mut RendererScratch,
    ) -> Result<(), RendererError> {
        if !self.configured {
            return Err(RendererError::NotConfigured);
        }
        if object_count > self.max_objects {
            return Err(RendererError::TooManyObjects {
                maximum: self.max_objects,
                actual: object_count,
            });
        }
        let required_output = object_count.saturating_mul(self.layout.len());
        if output_count != required_output {
            return Err(RendererError::OutputBufferSize {
                required: required_output,
                actual: output_count,
            });
        }
        let actual_scratch = scratch.floats_mut().len();
        if actual_scratch < self.layout.len() {
            return Err(RendererError::ScratchBufferSize {
                required: self.layout.len(),
                actual: actual_scratch,
            });
        }
        Ok(())
    }
}

impl Renderer for Vbap3dRenderer {
    fn configure(
        &mut self,
        layout: Vec<Speaker>,
        sample_rate: u32,
        block_size: usize,
        max_objects: usize,
    ) -> Result<(), RendererError> {
        if sample_rate == 0 || block_size == 0 || max_objects == 0 {
            return Err(RendererError::InvalidConfiguration(
                "sample rate, block size, and max objects must be greater than zero".to_owned(),
            ));
        }
        let enabled_layout = layout
            .into_iter()
            .filter(|speaker| speaker.enabled)
            .collect::<Vec<_>>();
        if enabled_layout.is_empty() {
            return Err(RendererError::NoEnabledSpeakers);
        }
        if enabled_layout.iter().any(|speaker| {
            !vector_is_finite(speaker.position)
                || !speaker.gain_db.is_finite()
                || !speaker.delay_samples.is_finite()
        }) {
            return Err(RendererError::InvalidConfiguration(
                "enabled speaker positions, gains, and delays must be finite".to_owned(),
            ));
        }

        let spatial_indices = enabled_layout
            .iter()
            .enumerate()
            .filter_map(|(index, speaker)| {
                (!matches!(speaker.channel_role, ChannelRole::LowFrequencyEffects)).then_some(index)
            })
            .collect::<Vec<_>>();
        if spatial_indices.len() < 3 {
            return Err(RendererError::InvalidConfiguration(
                "3D VBAP requires at least three enabled non-LFE speakers".to_owned(),
            ));
        }
        if spatial_indices.len() > MAX_SPATIAL_SPEAKERS {
            return Err(RendererError::InvalidConfiguration(format!(
                "3D VBAP supports at most {MAX_SPATIAL_SPEAKERS} spatial speakers"
            )));
        }

        let triplet_capacity = combination_count_three(spatial_indices.len()).ok_or_else(|| {
            RendererError::InvalidConfiguration("3D VBAP triplet capacity overflow".to_owned())
        })?;
        let history_len = enabled_layout
            .len()
            .checked_mul(max_objects)
            .ok_or_else(|| {
                RendererError::InvalidConfiguration(
                    "enabled speaker and object capacity product is too large".to_owned(),
                )
            })?;

        self.layout = enabled_layout;
        self.spatial_indices = spatial_indices;
        self.direction_cache = vec![[0.0; 3]; self.layout.len()];
        self.active_triplets = Vec::with_capacity(triplet_capacity);
        self.face_vertices = Vec::with_capacity(self.spatial_indices.len());
        self.sample_rate = sample_rate;
        self.block_size = block_size;
        self.max_objects = max_objects;
        self.previous_gains = vec![0.0; history_len];
        self.prepared_listener_center = None;
        self.prepared_topology_valid = false;
        self.listener_inside_hull = false;
        self.configured = true;
        Ok(())
    }

    fn required_scratch_size(&self) -> Result<RendererScratchSize, RendererError> {
        if !self.configured {
            return Err(RendererError::NotConfigured);
        }
        Ok(RendererScratchSize {
            float_count: self.layout.len(),
        })
    }

    fn render_gains(
        &mut self,
        listener: &Listener,
        objects: &[RenderObject],
        output_gains: &mut [SpeakerGain],
        scratch: &mut RendererScratch,
    ) -> Result<(), RendererError> {
        self.validate_render_buffers(objects.len(), output_gains.len(), scratch)?;
        let listener_center = acoustic_listener_center(listener);
        self.prepare_listener_internal(listener);

        let speaker_count = self.layout.len();
        let available = scratch.floats_mut().len();
        let weights = scratch.floats_mut().get_mut(..speaker_count).ok_or(
            RendererError::ScratchBufferSize {
                required: speaker_count,
                actual: available,
            },
        )?;
        for (object_index, (object, object_output)) in objects
            .iter()
            .copied()
            .zip(output_gains.chunks_exact_mut(speaker_count))
            .enumerate()
        {
            self.render_object(
                listener_center,
                object_index,
                object,
                weights,
                object_output,
            )?;
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.previous_gains.fill(0.0);
    }

    fn latency_frames(&self) -> usize {
        let _ = self.block_size;
        0
    }

    fn output_channel_count(&self) -> usize {
        self.layout.len()
    }
}

fn supporting_plane(
    indices: [usize; 3],
    directions: &[[f64; 3]],
    spatial_indices: &[usize],
) -> Option<SupportingPlane> {
    let first = directions[indices[0]];
    let second = directions[indices[1]];
    let third = directions[indices[2]];
    let raw_normal = cross(
        subtract_arrays(second, first),
        subtract_arrays(third, first),
    );
    let mut normal = normalize(raw_normal)?;

    let mut has_positive = false;
    let mut has_negative = false;
    for index in spatial_indices.iter().copied() {
        if indices.contains(&index) {
            continue;
        }
        let side = dot(normal, subtract_arrays(directions[index], first));
        if side > FACET_EPSILON {
            has_positive = true;
        } else if side < -FACET_EPSILON {
            has_negative = true;
        }
        if has_positive && has_negative {
            return None;
        }
    }

    if has_positive {
        normal = scale(normal, -1.0);
    } else if !has_negative && dot(normal, first) < 0.0 {
        normal = scale(normal, -1.0);
    }
    let offset = dot(normal, first);
    Some(SupportingPlane {
        normal,
        offset,
        has_strict_interior_side: has_positive || has_negative,
    })
}

fn canonical_face_trigger(
    vertices: &[usize],
    directions: &[[f64; 3]],
    layout: &[Speaker],
) -> Option<[usize; 3]> {
    for first in 0..vertices.len() - 2 {
        for second in first + 1..vertices.len() - 1 {
            for third in second + 1..vertices.len() {
                let indices = [vertices[first], vertices[second], vertices[third]];
                let area = length(cross(
                    subtract_arrays(directions[indices[1]], directions[indices[0]]),
                    subtract_arrays(directions[indices[2]], directions[indices[0]]),
                ));
                if area > GEOMETRY_EPSILON {
                    return Some(sorted_triplet_by_speaker_id(indices, layout));
                }
            }
        }
    }
    None
}

fn sorted_triplet_by_speaker_id(mut indices: [usize; 3], layout: &[Speaker]) -> [usize; 3] {
    if compare_speakers(layout, indices[1], indices[0]) == Ordering::Less {
        indices.swap(0, 1);
    }
    if compare_speakers(layout, indices[2], indices[1]) == Ordering::Less {
        indices.swap(1, 2);
    }
    if compare_speakers(layout, indices[1], indices[0]) == Ordering::Less {
        indices.swap(0, 1);
    }
    indices
}

fn compare_speakers(layout: &[Speaker], first: usize, second: usize) -> Ordering {
    layout[first]
        .id
        .cmp(&layout[second].id)
        .then_with(|| first.cmp(&second))
}

fn solve_triplet(
    first: [f64; 3],
    second: [f64; 3],
    third: [f64; 3],
    source: [f64; 3],
) -> Option<[f64; 3]> {
    let base_determinant = determinant(first, second, third);
    if base_determinant.abs() <= GEOMETRY_EPSILON || !base_determinant.is_finite() {
        return None;
    }
    let gains = [
        dot(source, cross(second, third)) / base_determinant,
        dot(first, cross(source, third)) / base_determinant,
        dot(first, cross(second, source)) / base_determinant,
    ];
    gains.iter().all(|gain| gain.is_finite()).then_some(gains)
}

fn nearest_direction(
    layout: &[Speaker],
    spatial_indices: &[usize],
    source_direction: [f64; 3],
    directions: &[[f64; 3]],
    weights: &mut [f32],
) {
    let mut best = None::<(usize, f64)>;
    for index in spatial_indices.iter().copied() {
        let direction = directions[index];
        if length(direction) <= GEOMETRY_EPSILON {
            continue;
        }
        let candidate_dot = dot(direction, source_direction);
        let replace = best.is_none_or(|(best_index, best_dot)| {
            candidate_dot > best_dot + GAIN_EPSILON
                || ((candidate_dot - best_dot).abs() <= GAIN_EPSILON
                    && compare_speakers(layout, index, best_index) == Ordering::Less)
        });
        if replace {
            best = Some((index, candidate_dot));
        }
    }
    if let Some((index, _)) = best {
        weights[index] = 1.0;
    } else {
        equal_power_spatial(weights, spatial_indices);
    }
}

fn equal_power_spatial(weights: &mut [f32], spatial_indices: &[usize]) {
    if spatial_indices.is_empty() {
        return;
    }
    let gain = 1.0 / (spatial_indices.len() as f32).sqrt();
    for index in spatial_indices.iter().copied() {
        weights[index] = gain;
    }
}

fn maximum_pair_angle(first: [f64; 3], second: [f64; 3], third: [f64; 3]) -> f64 {
    angle(first, second)
        .max(angle(first, third))
        .max(angle(second, third))
}

fn angle(first: [f64; 3], second: [f64; 3]) -> f64 {
    dot(first, second).clamp(-1.0, 1.0).acos()
}

fn determinant(first: [f64; 3], second: [f64; 3], third: [f64; 3]) -> f64 {
    dot(first, cross(second, third))
}

fn cross(first: [f64; 3], second: [f64; 3]) -> [f64; 3] {
    [
        first[1] * second[2] - first[2] * second[1],
        first[2] * second[0] - first[0] * second[2],
        first[0] * second[1] - first[1] * second[0],
    ]
}

fn dot(first: [f64; 3], second: [f64; 3]) -> f64 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
}

fn normalize(vector: [f64; 3]) -> Option<[f64; 3]> {
    let vector_length = length(vector);
    (vector_length > GEOMETRY_EPSILON && vector_length.is_finite())
        .then(|| scale(vector, 1.0 / vector_length))
}

fn scale(vector: [f64; 3], scalar: f64) -> [f64; 3] {
    [vector[0] * scalar, vector[1] * scalar, vector[2] * scalar]
}

fn add(first: [f64; 3], second: [f64; 3]) -> [f64; 3] {
    [
        first[0] + second[0],
        first[1] + second[1],
        first[2] + second[2],
    ]
}

fn subtract_arrays(first: [f64; 3], second: [f64; 3]) -> [f64; 3] {
    [
        first[0] - second[0],
        first[1] - second[1],
        first[2] - second[2],
    ]
}

fn subtract(first: Vector3, second: Vector3) -> [f64; 3] {
    [
        first.x as f64 - second.x as f64,
        first.y as f64 - second.y as f64,
        first.z as f64 - second.z as f64,
    ]
}

fn length(vector: [f64; 3]) -> f64 {
    dot(vector, vector).sqrt()
}

fn distance(first: Vector3, second: Vector3) -> f64 {
    length(subtract(first, second))
}

fn acoustic_listener_center(listener: &Listener) -> Option<Vector3> {
    let center = Vector3::new(
        listener.position.x,
        listener.position.y,
        listener.position.z + listener.ear_height,
    );
    (vector_is_finite(center) && listener.ear_height.is_finite()).then_some(center)
}

fn vector_is_finite(vector: Vector3) -> bool {
    vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
}

fn db_to_gain(db: f32) -> f64 {
    10.0_f64.powf(db as f64 / 20.0)
}

fn finite_f32(value: f64) -> f32 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(-(f32::MAX as f64), f32::MAX as f64) as f32
    }
}

fn combination_count_three(count: usize) -> Option<usize> {
    if count < 3 {
        return Some(0);
    }
    count
        .checked_mul(count - 1)?
        .checked_mul(count - 2)?
        .checked_div(6)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use aurora_scene::load_render_scene;

    use super::*;

    fn speaker(id: &str, role: ChannelRole, x: f32, y: f32, z: f32) -> Speaker {
        Speaker {
            id: id.to_owned(),
            label: id.to_owned(),
            channel_role: role,
            position: Vector3::new(x, y, z),
            orientation: Vector3::ZERO,
            gain_db: 0.0,
            delay_samples: 0.0,
            enabled: true,
        }
    }

    fn listener_at_origin() -> Listener {
        Listener {
            position: Vector3::ZERO,
            orientation: Vector3::new(0.0, 1.0, 0.0),
            ear_height: 0.0,
        }
    }

    fn axis_layout() -> Vec<Speaker> {
        vec![
            speaker(
                "axis-x",
                ChannelRole::Custom("axis-x".to_owned()),
                1.0,
                0.0,
                0.0,
            ),
            speaker(
                "axis-y",
                ChannelRole::Custom("axis-y".to_owned()),
                0.0,
                1.0,
                0.0,
            ),
            speaker(
                "axis-z",
                ChannelRole::Custom("axis-z".to_owned()),
                0.0,
                0.0,
                1.0,
            ),
        ]
    }

    fn configured(
        layout: Vec<Speaker>,
        listener: &Listener,
    ) -> (Vbap3dRenderer, RendererScratch) {
        let mut renderer = Vbap3dRenderer::new();
        renderer.configure(layout, 48_000, 256, 1).unwrap();
        renderer.prepare_listener(listener).unwrap();
        let scratch = RendererScratch::new(renderer.required_scratch_size().unwrap());
        (renderer, scratch)
    }

    fn render(
        layout: Vec<Speaker>,
        listener: &Listener,
        position: Vector3,
    ) -> Vec<SpeakerGain> {
        let speaker_count = layout.iter().filter(|speaker| speaker.enabled).count();
        let (mut renderer, mut scratch) = configured(layout, listener);
        let mut output = vec![SpeakerGain::default(); speaker_count];
        renderer
            .render_gains(
                listener,
                &[RenderObject {
                    position,
                    gain: 1.0,
                }],
                &mut output,
                &mut scratch,
            )
            .unwrap();
        output
    }

    fn power(gains: &[SpeakerGain]) -> f32 {
        gains.iter().map(|gain| gain.gain * gain.gain).sum()
    }

    fn five_one_two_fixture() -> aurora_scene::RenderScene {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/scenes/5_1_2_upfiring.json");
        load_render_scene(fixture).unwrap()
    }

    #[test]
    fn three_axis_base_is_validated_and_routes_exact_axis() {
        let listener = listener_at_origin();
        let (renderer, _) = configured(axis_layout(), &listener);
        assert_eq!(renderer.validated_triplets().len(), 1);
        assert!(renderer.validated_triplets()[0].determinant_abs() > 0.99);
        assert!(!renderer.listener_inside_hull());

        let gains = render(
            axis_layout(),
            &listener,
            Vector3::new(1.0, 0.0, 0.0),
        );
        assert!((gains[0].gain - 1.0).abs() < 1.0e-5);
        assert!(gains[1].gain.abs() < 1.0e-5);
        assert!(gains[2].gain.abs() < 1.0e-5);
    }

    #[test]
    fn diagonal_direction_preserves_unit_spatial_power() {
        let gains = render(
            axis_layout(),
            &listener_at_origin(),
            Vector3::new(1.0, 1.0, 1.0),
        );
        let expected = 1.0 / 3.0_f32.sqrt();
        for gain in &gains {
            assert!((gain.gain - expected).abs() < 1.0e-5);
        }
        assert!((power(&gains) - 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn ear_height_is_part_of_listener_relative_elevation() {
        let scene = five_one_two_fixture();
        let mut renderer = Vbap3dRenderer::new();
        renderer
            .configure(scene.speakers.clone(), 48_000, scene.block_size, 1)
            .unwrap();
        renderer.prepare_listener(&scene.listener).unwrap();

        let front_left = scene
            .speakers
            .iter()
            .position(|speaker| speaker.channel_role == ChannelRole::FrontLeft)
            .unwrap();
        assert!(renderer.direction_cache[front_left][2] < 0.0);
        let top_front_left = scene
            .speakers
            .iter()
            .position(|speaker| speaker.channel_role == ChannelRole::TopFrontLeft)
            .unwrap();
        assert!(renderer.direction_cache[top_front_left][2] > 0.0);
    }

    #[test]
    fn five_one_two_fixture_builds_closed_convex_hull() {
        let scene = five_one_two_fixture();
        let mut renderer = Vbap3dRenderer::new();
        renderer
            .configure(scene.speakers.clone(), 48_000, scene.block_size, 1)
            .unwrap();
        renderer.prepare_listener(&scene.listener).unwrap();

        assert!(renderer.listener_inside_hull());
        assert_eq!(renderer.validated_triplets().len(), 10);
        assert!(renderer
            .validated_triplets()
            .iter()
            .all(|triplet| triplet.determinant_abs() > GEOMETRY_EPSILON));
    }

    #[test]
    fn elevated_front_source_uses_height_channels_and_excludes_lfe() {
        let scene = five_one_two_fixture();
        let mut renderer = Vbap3dRenderer::new();
        renderer
            .configure(scene.speakers.clone(), 48_000, scene.block_size, 1)
            .unwrap();
        let mut scratch = RendererScratch::new(renderer.required_scratch_size().unwrap());
        let mut gains = vec![SpeakerGain::default(); renderer.output_channel_count()];
        renderer
            .render_gains(
                &scene.listener,
                &[RenderObject {
                    position: Vector3::new(0.0, 1.1, 2.2),
                    gain: 1.0,
                }],
                &mut gains,
                &mut scratch,
            )
            .unwrap();

        let lfe_index = scene
            .speakers
            .iter()
            .position(|speaker| speaker.channel_role == ChannelRole::LowFrequencyEffects)
            .unwrap();
        assert_eq!(gains[lfe_index].gain, 0.0);
        let top_energy = gains
            .iter()
            .enumerate()
            .filter(|(index, _)| {
                matches!(
                    scene.speakers[*index].channel_role,
                    ChannelRole::TopFrontLeft | ChannelRole::TopFrontRight
                )
            })
            .map(|(_, gain)| gain.gain * gain.gain)
            .sum::<f32>();
        assert!(top_energy > 0.0);
        assert!(gains.iter().all(|gain| gain.gain.is_finite()));
    }

    #[test]
    fn five_one_two_bed_height_circle_is_continuous() {
        let scene = five_one_two_fixture();
        let mut layout = scene.speakers.clone();
        for speaker in &mut layout {
            speaker.gain_db = 0.0;
        }
        let mut renderer = Vbap3dRenderer::new();
        renderer
            .configure(layout, 48_000, scene.block_size, 1)
            .unwrap();
        let mut scratch = RendererScratch::new(renderer.required_scratch_size().unwrap());
        let mut gains = vec![SpeakerGain::default(); renderer.output_channel_count()];
        let mut previous = vec![0.0_f32; renderer.output_channel_count()];
        let mut max_delta = 0.0_f32;

        for step in 0..=360 {
            let angle = std::f32::consts::TAU * step as f32 / 360.0;
            let position = Vector3::new(1.1 * angle.cos(), 1.1 * angle.sin(), 1.0);
            renderer
                .render_gains(
                    &scene.listener,
                    &[RenderObject {
                        position,
                        gain: 1.0,
                    }],
                    &mut gains,
                    &mut scratch,
                )
                .unwrap();
            assert!((power(&gains) - 1.0).abs() < 1.0e-4);
            if step > 0 {
                let delta = gains
                    .iter()
                    .zip(previous.iter())
                    .map(|(current, previous)| (current.gain - previous).powi(2))
                    .sum::<f32>()
                    .sqrt();
                max_delta = max_delta.max(delta);
            }
            for (stored, current) in previous.iter_mut().zip(gains.iter()) {
                *stored = current.gain;
            }
        }

        assert!(max_delta < 0.05, "maximum gain-vector step was {max_delta}");
    }

    #[test]
    fn outside_open_hull_uses_deterministic_nearest_fallback() {
        let listener = listener_at_origin();
        let first = render(
            axis_layout(),
            &listener,
            Vector3::new(-1.0, 0.0, 0.0),
        );
        let second = render(
            axis_layout(),
            &listener,
            Vector3::new(-1.0, 0.0, 0.0),
        );
        assert_eq!(first, second);
        assert_eq!(first.iter().filter(|gain| gain.gain > 0.5).count(), 1);
        assert!((power(&first) - 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn configuration_order_only_permutes_output_channels() {
        let scene = five_one_two_fixture();
        let source = Vector3::new(0.35, 0.9, 1.75);
        let original_layout = scene.speakers.clone();
        let mut reversed_layout = original_layout.clone();
        reversed_layout.reverse();

        let original = render(original_layout.clone(), &scene.listener, source);
        let reversed = render(reversed_layout.clone(), &scene.listener, source);
        let original_by_id = original_layout
            .iter()
            .zip(original.iter())
            .map(|(speaker, gain)| (speaker.id.clone(), gain.gain))
            .collect::<BTreeMap<_, _>>();
        let reversed_by_id = reversed_layout
            .iter()
            .zip(reversed.iter())
            .map(|(speaker, gain)| (speaker.id.clone(), gain.gain))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(original_by_id.len(), reversed_by_id.len());
        for (id, gain) in original_by_id {
            assert!((gain - reversed_by_id[&id]).abs() < 1.0e-5);
        }
    }

    #[test]
    fn non_finite_runtime_source_produces_finite_silence() {
        let gains = render(
            axis_layout(),
            &listener_at_origin(),
            Vector3::new(f32::NAN, 1.0, 1.0),
        );
        assert!(gains.iter().all(|gain| gain.gain == 0.0));
        assert!(gains.iter().all(|gain| gain.distance_meters.is_finite()));
        assert!(gains.iter().all(|gain| gain.delay_samples.is_finite()));
    }

    #[test]
    fn warmed_up_render_and_listener_rebuild_allocate_zero_times() {
        let scene = five_one_two_fixture();
        let mut renderer = Vbap3dRenderer::new();
        renderer
            .configure(scene.speakers.clone(), 48_000, scene.block_size, 1)
            .unwrap();
        let mut scratch = RendererScratch::new(renderer.required_scratch_size().unwrap());
        let mut gains = vec![SpeakerGain::default(); renderer.output_channel_count()];
        let object = RenderObject {
            position: Vector3::new(0.4, 0.8, 1.6),
            gain: 1.0,
        };
        renderer
            .render_gains(
                &scene.listener,
                std::slice::from_ref(&object),
                &mut gains,
                &mut scratch,
            )
            .unwrap();

        let allocations = crate::allocation_audit::count_allocations(|| {
            renderer
                .render_gains(
                    &scene.listener,
                    std::slice::from_ref(&object),
                    &mut gains,
                    &mut scratch,
                )
                .unwrap();

            let moved_listener = Listener {
                position: Vector3::new(0.01, -0.01, 0.0),
                ..scene.listener
            };
            renderer
                .render_gains(
                    &moved_listener,
                    std::slice::from_ref(&object),
                    &mut gains,
                    &mut scratch,
                )
                .unwrap();
        });

        assert_eq!(allocations, 0);
    }
}
