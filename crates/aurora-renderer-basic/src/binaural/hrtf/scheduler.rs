//! Control-thread scheduling for pose-selected object HRTF snapshots.
//!
//! This module owns pose delivery, stable object/channel identity, candidate generation
//! and candidate lifetime. Filter preparation may allocate on the control thread. The
//! exclusive block-boundary commit only borrows already prepared storage; releasing or
//! cancelling a candidate is a separate control-thread operation so no candidate is
//! deallocated from the realtime boundary.

use aurora_core::Vector3;
use aurora_renderer_api::{HeadPoseError, HeadPosePolicy, HeadPoseSample, HeadPoseState};

use super::{DirectionalHrtf, PreparationError};
use crate::binaural::{Error, Filters, PreparedBinaural};

/// One object direction presented in the exact PCM channel order configured at creation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ObjectDirection<'a> {
    /// Stable object identifier. It must match the configured identifier at this index.
    pub id: &'a str,
    /// World-space direction using Aurora axes (+X right, +Y front, +Z up).
    pub world_direction: Vector3,
}

/// Metadata for the single candidate currently owned by the scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingStatus {
    /// Candidate filter generation.
    pub generation: u64,
    /// Exact logical media frame at which the candidate may be committed.
    pub target_frame: u64,
    /// Whether the renderer already accepted the borrowed candidate.
    pub committed: bool,
}

/// Fail-closed scheduling errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerError {
    /// Invalid configured object set or initial generation.
    Contract,
    /// Object count, identity or PCM channel order changed.
    ObjectIdentity,
    /// A candidate is already owned and must be committed/released or cancelled first.
    PendingCandidate,
    /// No candidate is currently owned.
    NoPendingCandidate,
    /// Candidate generation does not match the requested control operation.
    CandidateGeneration,
    /// Candidate was already committed and must be released on the control thread.
    CandidateAlreadyCommitted,
    /// Candidate has not yet been committed and therefore cannot be released as committed.
    CandidateNotCommitted,
    /// The block boundary did not equal the candidate's exact scheduled media frame.
    BoundaryFrame { expected: u64, actual: u64 },
    /// A new target frame would not advance past the last committed boundary.
    NonMonotonicBoundary { previous: u64, actual: u64 },
    /// No further strictly increasing filter generation can be represented.
    GenerationExhausted,
    /// Pose delivery was rejected by the bounded timeline.
    Pose(HeadPoseError),
    /// Pose/HRTF preparation failed before a candidate was published.
    Preparation(PreparationError),
    /// The prepared renderer rejected the candidate at the block boundary.
    Renderer(Error),
}

#[derive(Debug)]
struct PendingCandidate {
    status: PendingStatus,
    filters: Filters,
}

/// Bounded control-plane owner for continuously refreshed object HRTF snapshots.
///
/// Exactly one candidate may be outstanding. The configured object identifiers define
/// the immutable PCM channel order. Moving objects may change directions, but adding,
/// removing or reordering identities requires explicit scheduler/renderer reconfiguration.
#[derive(Debug)]
pub struct HeadPoseHrtfScheduler {
    hrtf: DirectionalHrtf,
    poses: HeadPoseState,
    object_ids: Vec<String>,
    generation: u64,
    last_committed_frame: Option<u64>,
    pending: Option<PendingCandidate>,
}

impl HeadPoseHrtfScheduler {
    /// Creates a scheduler around an already prepared HRTF bank.
    ///
    /// `active_generation` must match the renderer generation from which scheduling
    /// continues. Object identifiers must be unique, non-empty, and contain 1-16 entries.
    /// Allocation is permitted here because this is a control-thread constructor.
    pub fn new(
        hrtf: DirectionalHrtf,
        policy: HeadPosePolicy,
        object_ids: Vec<String>,
        active_generation: u64,
    ) -> Result<Self, SchedulerError> {
        if active_generation == 0 || !(1..=16).contains(&object_ids.len()) {
            return Err(SchedulerError::Contract);
        }
        for (index, id) in object_ids.iter().enumerate() {
            if id.is_empty() || object_ids[..index].iter().any(|other| other == id) {
                return Err(SchedulerError::Contract);
            }
        }
        Ok(Self {
            hrtf,
            poses: HeadPoseState::new(policy),
            object_ids,
            generation: active_generation,
            last_committed_frame: None,
            pending: None,
        })
    }

    /// Delivers one tracker sample already mapped onto Aurora's logical media timeline.
    /// This stores only the fixed two-sample pose window owned by `HeadPoseState`.
    pub fn commit_pose(&mut self, sample: HeadPoseSample) -> Result<(), SchedulerError> {
        self.poses.commit(sample).map_err(SchedulerError::Pose)
    }

    /// Clears tracker-pose history before an explicit mapper re-anchor/reconnect.
    ///
    /// Any prepared candidate must first be committed/released or cancelled on the control
    /// thread. Filter generation and Aurora media-boundary history intentionally continue.
    pub fn reset_pose_epoch(&mut self) -> Result<(), SchedulerError> {
        if self.pending.is_some() {
            return Err(SchedulerError::PendingCandidate);
        }
        self.poses.reset();
        Ok(())
    }

    /// Prepares one pose-selected filter snapshot for an exact future block boundary.
    ///
    /// This is a control-thread operation and may allocate. It is transactional: identity,
    /// pose, direction, angular coverage or filter failure leaves no candidate published
    /// and does not advance the owned generation.
    pub fn prepare_at(
        &mut self,
        target_frame: u64,
        objects: &[ObjectDirection<'_>],
    ) -> Result<u64, SchedulerError> {
        if self.pending.is_some() {
            return Err(SchedulerError::PendingCandidate);
        }
        if let Some(previous) = self.last_committed_frame {
            if target_frame <= previous {
                return Err(SchedulerError::NonMonotonicBoundary {
                    previous,
                    actual: target_frame,
                });
            }
        }
        if objects.len() != self.object_ids.len() {
            return Err(SchedulerError::ObjectIdentity);
        }

        let mut world_directions = Vec::with_capacity(objects.len());
        for (expected, object) in self.object_ids.iter().zip(objects) {
            if expected != object.id {
                return Err(SchedulerError::ObjectIdentity);
            }
            world_directions.push(object.world_direction);
        }

        let generation = self
            .generation
            .checked_add(1)
            .ok_or(SchedulerError::GenerationExhausted)?;
        let filters = self
            .hrtf
            .prepare_objects(&self.poses, target_frame, &world_directions, generation)
            .map_err(SchedulerError::Preparation)?;
        self.generation = generation;
        self.pending = Some(PendingCandidate {
            status: PendingStatus {
                generation,
                target_frame,
                committed: false,
            },
            filters,
        });
        Ok(generation)
    }

    /// Borrows the prepared candidate into the renderer at its exact block boundary.
    ///
    /// This operation neither allocates nor releases candidate storage. On success the
    /// candidate remains scheduler-owned until `release_committed` is called later from
    /// the control thread. Renderer rejection preserves the uncommitted candidate.
    pub fn commit_at_boundary(
        &mut self,
        renderer: &mut PreparedBinaural,
        boundary_frame: u64,
        transition_frames: usize,
    ) -> Result<u64, SchedulerError> {
        let pending = self
            .pending
            .as_mut()
            .ok_or(SchedulerError::NoPendingCandidate)?;
        if pending.status.committed {
            return Err(SchedulerError::CandidateAlreadyCommitted);
        }
        if boundary_frame != pending.status.target_frame {
            return Err(SchedulerError::BoundaryFrame {
                expected: pending.status.target_frame,
                actual: boundary_frame,
            });
        }
        renderer
            .commit(&pending.filters, transition_frames)
            .map_err(SchedulerError::Renderer)?;
        pending.status.committed = true;
        self.last_committed_frame = Some(boundary_frame);
        Ok(pending.status.generation)
    }

    /// Releases a renderer-accepted candidate on the control thread.
    ///
    /// This is deliberately separate from `commit_at_boundary`: dropping `Filters` may
    /// deallocate and therefore must never be folded into a realtime callback boundary.
    pub fn release_committed(&mut self, generation: u64) -> Result<(), SchedulerError> {
        let pending = self
            .pending
            .as_ref()
            .ok_or(SchedulerError::NoPendingCandidate)?;
        if pending.status.generation != generation {
            return Err(SchedulerError::CandidateGeneration);
        }
        if !pending.status.committed {
            return Err(SchedulerError::CandidateNotCommitted);
        }
        self.pending = None;
        Ok(())
    }

    /// Cancels an uncommitted prepared candidate on the control thread.
    pub fn cancel_pending(&mut self, generation: u64) -> Result<(), SchedulerError> {
        let pending = self
            .pending
            .as_ref()
            .ok_or(SchedulerError::NoPendingCandidate)?;
        if pending.status.generation != generation {
            return Err(SchedulerError::CandidateGeneration);
        }
        if pending.status.committed {
            return Err(SchedulerError::CandidateAlreadyCommitted);
        }
        self.pending = None;
        Ok(())
    }

    /// Returns metadata for the currently owned candidate without exposing its storage.
    pub fn pending_status(&self) -> Option<PendingStatus> {
        self.pending.as_ref().map(|pending| pending.status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_renderer_api::UnitQuaternion;

    fn bank() -> DirectionalHrtf {
        DirectionalHrtf::prepare(
            48_000,
            1,
            0.1,
            vec![
                super::super::SofaMeasurement {
                    // Canonical SOFA front -> Aurora front.
                    direction: Vector3::new(1.0, 0.0, 0.0),
                    coefficients: vec![1.0, 0.0],
                },
                super::super::SofaMeasurement {
                    // Canonical SOFA right -> Aurora right after axis conversion.
                    direction: Vector3::new(0.0, -1.0, 0.0),
                    coefficients: vec![0.0, 1.0],
                },
            ],
        )
        .unwrap()
    }

    fn scheduler(stale_frames: u64) -> HeadPoseHrtfScheduler {
        let mut scheduler = HeadPoseHrtfScheduler::new(
            bank(),
            HeadPosePolicy::new(100, stale_frames).unwrap(),
            vec!["dialogue".to_owned(), "effect".to_owned()],
            1,
        )
        .unwrap();
        scheduler
            .commit_pose(HeadPoseSample {
                sequence: 1,
                media_frame: 100,
                orientation: UnitQuaternion::IDENTITY,
            })
            .unwrap();
        scheduler
    }

    fn objects<'a>() -> [ObjectDirection<'a>; 2] {
        [
            ObjectDirection {
                id: "dialogue",
                world_direction: Vector3::new(0.0, 1.0, 0.0),
            },
            ObjectDirection {
                id: "effect",
                world_direction: Vector3::new(1.0, 0.0, 0.0),
            },
        ]
    }

    fn renderer() -> PreparedBinaural {
        let initial = Filters::prepare(
            crate::binaural::Input::Objects(2),
            48_000,
            1,
            1,
            vec![0.5, 0.5, 0.5, 0.5],
        )
        .unwrap();
        PreparedBinaural::new(initial, 8).unwrap()
    }

    #[test]
    fn rejects_invalid_or_duplicate_identity_contracts() {
        let policy = HeadPosePolicy::new(10, 10).unwrap();
        assert_eq!(
            HeadPoseHrtfScheduler::new(bank(), policy, vec![], 1).unwrap_err(),
            SchedulerError::Contract
        );
        assert_eq!(
            HeadPoseHrtfScheduler::new(
                bank(),
                policy,
                vec!["same".to_owned(), "same".to_owned()],
                1,
            )
            .unwrap_err(),
            SchedulerError::Contract
        );
        assert_eq!(
            HeadPoseHrtfScheduler::new(bank(), policy, vec!["one".to_owned()], 0).unwrap_err(),
            SchedulerError::Contract
        );
    }

    #[test]
    fn stable_identity_and_exact_boundary_gate_candidate_commit() {
        let mut scheduler = scheduler(10);
        let reordered = [
            ObjectDirection {
                id: "effect",
                world_direction: Vector3::new(1.0, 0.0, 0.0),
            },
            ObjectDirection {
                id: "dialogue",
                world_direction: Vector3::new(0.0, 1.0, 0.0),
            },
        ];
        assert_eq!(
            scheduler.prepare_at(100, &reordered),
            Err(SchedulerError::ObjectIdentity)
        );

        let generation = scheduler.prepare_at(100, &objects()).unwrap();
        assert_eq!(generation, 2);
        assert_eq!(
            scheduler.pending_status(),
            Some(PendingStatus {
                generation: 2,
                target_frame: 100,
                committed: false,
            })
        );

        let mut renderer = renderer();
        assert_eq!(
            scheduler.commit_at_boundary(&mut renderer, 99, 1),
            Err(SchedulerError::BoundaryFrame {
                expected: 100,
                actual: 99,
            })
        );
        assert_eq!(renderer.generation(), 1);
        scheduler.commit_at_boundary(&mut renderer, 100, 1).unwrap();
        assert_eq!(renderer.generation(), 2);
        assert_eq!(
            scheduler.commit_at_boundary(&mut renderer, 100, 1),
            Err(SchedulerError::CandidateAlreadyCommitted)
        );

        let mut output = [0.0; 2];
        renderer.process(&[1.0, 0.0], &mut output).unwrap();
        assert_eq!(output, [1.0, 0.0]);
        scheduler.release_committed(generation).unwrap();
        assert_eq!(scheduler.pending_status(), None);
    }

    #[test]
    fn pending_lifetime_and_generation_are_control_owned() {
        let mut scheduler = scheduler(10);
        let generation = scheduler.prepare_at(100, &objects()).unwrap();
        assert_eq!(
            scheduler.prepare_at(101, &objects()),
            Err(SchedulerError::PendingCandidate)
        );
        assert_eq!(
            scheduler.release_committed(generation),
            Err(SchedulerError::CandidateNotCommitted)
        );
        scheduler.cancel_pending(generation).unwrap();
        let next = scheduler.prepare_at(101, &objects()).unwrap();
        assert_eq!(next, generation + 1);
    }

    #[test]
    fn explicit_pose_epoch_reset_accepts_restarted_source_sequence() {
        let mut scheduler = scheduler(10);
        let generation = scheduler.prepare_at(100, &objects()).unwrap();
        assert_eq!(
            scheduler.reset_pose_epoch(),
            Err(SchedulerError::PendingCandidate)
        );
        scheduler.cancel_pending(generation).unwrap();

        scheduler.reset_pose_epoch().unwrap();
        scheduler
            .commit_pose(HeadPoseSample {
                sequence: 1,
                media_frame: 200,
                orientation: UnitQuaternion::IDENTITY,
            })
            .unwrap();
        let generation = scheduler.prepare_at(200, &objects()).unwrap();
        let mut renderer = renderer();
        scheduler.commit_at_boundary(&mut renderer, 200, 1).unwrap();
        scheduler.release_committed(generation).unwrap();
    }

    #[test]
    fn stale_pose_fails_before_candidate_publication() {
        let mut scheduler = scheduler(2);
        assert_eq!(
            scheduler.prepare_at(103, &objects()),
            Err(SchedulerError::Preparation(PreparationError::Pose(
                HeadPoseError::StalePose {
                    age_frames: 3,
                    maximum_frames: 2,
                }
            )))
        );
        assert_eq!(scheduler.pending_status(), None);
    }

    #[test]
    fn committed_boundaries_must_advance() {
        let mut scheduler = scheduler(10);
        let generation = scheduler.prepare_at(100, &objects()).unwrap();
        let mut renderer = renderer();
        scheduler.commit_at_boundary(&mut renderer, 100, 1).unwrap();
        scheduler.release_committed(generation).unwrap();
        assert_eq!(
            scheduler.prepare_at(100, &objects()),
            Err(SchedulerError::NonMonotonicBoundary {
                previous: 100,
                actual: 100,
            })
        );
    }
}
