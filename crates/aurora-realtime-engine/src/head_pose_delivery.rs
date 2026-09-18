//! Bounded adapter-thread -> control-thread head-pose delivery bridge.
//!
//! Device I/O belongs to an external adapter thread. That thread may only publish fixed-size
//! tracker events into this preallocated queue. Control code polls at most one event per call,
//! maps samples onto Aurora logical media time, and then decides whether to feed a scheduler.
//! Nothing in this bridge requires or performs work from the audio callback.

use std::sync::Arc;

use aurora_renderer_api::{
    HeadPoseClockAnchor, HeadPoseClockError, HeadPoseClockMapper, HeadPoseClockPolicy,
    HeadPoseSample, TrackerPoseSample,
};
use crossbeam_queue::ArrayQueue;
use thiserror::Error;

const MAX_HEAD_POSE_DELIVERY_EVENTS: usize = 1024;

/// Fixed-size event accepted from a tracker/device adapter thread.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HeadPoseIngressEvent {
    /// One raw tracker pose still expressed in the tracker source-clock domain.
    Sample(TrackerPoseSample),
    /// The current tracker transport/source epoch ended.
    Disconnected,
    /// An explicit new source-clock/Aurora-media-frame correspondence.
    Reanchor(HeadPoseClockAnchor),
}

/// Setup-time bridge creation failures.
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum HeadPoseDeliveryCreateError {
    /// Queue capacity must allow at least a disconnect/reanchor pair and remain bounded.
    #[error("head-pose delivery queue capacity is outside the supported bound")]
    InvalidCapacity,
}

/// Nonblocking adapter-thread publication failures.
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum HeadPoseIngressPushError {
    /// The bounded queue is full. No event was published.
    #[error("head-pose delivery queue is full")]
    Overflow,
}

/// One control-side result from polling a single ingress event.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HeadPoseControlEvent {
    /// One tracker sample was mapped successfully onto Aurora logical media time.
    Mapped(HeadPoseSample),
    /// Mapping rejected a sample transactionally.
    Rejected {
        /// Source sequence of the rejected tracker sample.
        sequence: u64,
        /// Fail-closed mapper reason.
        error: HeadPoseClockError,
    },
    /// A sample arrived while no tracker epoch was active.
    RejectedWhileDisconnected {
        /// Source sequence rejected without touching mapper state.
        sequence: u64,
    },
    /// The active tracker epoch ended.
    Disconnected,
    /// A new explicit mapping epoch was installed.
    ///
    /// A scheduler consuming mapped poses must reset its pose epoch before accepting
    /// restarted source sequence numbers.
    Reanchored(HeadPoseClockAnchor),
}

/// Adapter-thread half of the bounded head-pose delivery bridge.
///
/// Construction allocates the queue once. Publication is nonblocking and allocation-free.
/// This type intentionally is not Clone, keeping one logical producer in the public API.
pub struct HeadPoseIngressProducer {
    queue: Arc<ArrayQueue<HeadPoseIngressEvent>>,
}

/// Control-thread half of the bounded head-pose delivery bridge.
pub struct HeadPoseDeliveryControl {
    queue: Arc<ArrayQueue<HeadPoseIngressEvent>>,
    mapper: HeadPoseClockMapper,
    connected: bool,
}

/// Creates one bounded tracker ingress/control bridge.
///
/// Capacity is limited to 2..=1024 events. Two slots are the minimum so a transport can
/// publish an ordered Disconnected + Reanchor pair without requiring an unbounded queue.
pub fn create_head_pose_delivery_bridge(
    policy: HeadPoseClockPolicy,
    initial_anchor: HeadPoseClockAnchor,
    capacity_events: usize,
) -> Result<(HeadPoseIngressProducer, HeadPoseDeliveryControl), HeadPoseDeliveryCreateError> {
    if !(2..=MAX_HEAD_POSE_DELIVERY_EVENTS).contains(&capacity_events) {
        return Err(HeadPoseDeliveryCreateError::InvalidCapacity);
    }
    let queue = Arc::new(ArrayQueue::new(capacity_events));
    Ok((
        HeadPoseIngressProducer {
            queue: Arc::clone(&queue),
        },
        HeadPoseDeliveryControl {
            queue,
            mapper: HeadPoseClockMapper::new(policy, initial_anchor),
            connected: true,
        },
    ))
}

impl HeadPoseIngressProducer {
    fn try_push(&self, event: HeadPoseIngressEvent) -> Result<(), HeadPoseIngressPushError> {
        self.queue
            .push(event)
            .map_err(|_| HeadPoseIngressPushError::Overflow)
    }

    /// Publishes one raw tracker sample without blocking or allocation.
    pub fn try_push_sample(
        &self,
        sample: TrackerPoseSample,
    ) -> Result<(), HeadPoseIngressPushError> {
        self.try_push(HeadPoseIngressEvent::Sample(sample))
    }

    /// Publishes an explicit tracker disconnect marker.
    pub fn try_push_disconnected(&self) -> Result<(), HeadPoseIngressPushError> {
        self.try_push(HeadPoseIngressEvent::Disconnected)
    }

    /// Publishes an explicit new source-clock/Aurora-media-frame anchor.
    pub fn try_push_reanchor(
        &self,
        anchor: HeadPoseClockAnchor,
    ) -> Result<(), HeadPoseIngressPushError> {
        self.try_push(HeadPoseIngressEvent::Reanchor(anchor))
    }

    /// Number of ingress events currently waiting for control-thread processing.
    pub fn queued_events(&self) -> usize {
        self.queue.len()
    }

    /// Fixed prepared event capacity.
    pub fn capacity(&self) -> usize {
        self.queue.capacity()
    }
}

impl HeadPoseDeliveryControl {
    /// Polls and processes at most one queued tracker event.
    ///
    /// observed_media_frame is sampled by control code at dequeue time. It is used only
    /// when mapping a Sample; disconnect and re-anchor markers ignore it. A caller that
    /// wants to process multiple events must apply its own explicit per-tick bound.
    pub fn try_poll(&mut self, observed_media_frame: u64) -> Option<HeadPoseControlEvent> {
        let event = self.queue.pop()?;
        Some(match event {
            HeadPoseIngressEvent::Sample(sample) => {
                if !self.connected {
                    HeadPoseControlEvent::RejectedWhileDisconnected {
                        sequence: sample.sequence,
                    }
                } else {
                    match self.mapper.map(sample, observed_media_frame) {
                        Ok(mapped) => HeadPoseControlEvent::Mapped(mapped),
                        Err(error) => HeadPoseControlEvent::Rejected {
                            sequence: sample.sequence,
                            error,
                        },
                    }
                }
            }
            HeadPoseIngressEvent::Disconnected => {
                self.connected = false;
                HeadPoseControlEvent::Disconnected
            }
            HeadPoseIngressEvent::Reanchor(anchor) => {
                self.mapper.reset(anchor);
                self.connected = true;
                HeadPoseControlEvent::Reanchored(anchor)
            }
        })
    }

    /// Whether a tracker mapping epoch is currently active.
    pub const fn connected(&self) -> bool {
        self.connected
    }

    /// Number of ingress events still queued.
    pub fn queued_events(&self) -> usize {
        self.queue.len()
    }

    /// Current explicit mapper anchor.
    pub const fn anchor(&self) -> HeadPoseClockAnchor {
        self.mapper.anchor()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_renderer_api::UnitQuaternion;
    use aurora_test_alloc::count_allocations;

    fn policy() -> HeadPoseClockPolicy {
        HeadPoseClockPolicy::new(48_000, 20_000_000, 240, 48).unwrap()
    }

    fn anchor() -> HeadPoseClockAnchor {
        HeadPoseClockAnchor {
            source_timestamp_ns: 1_000_000_000,
            media_frame: 48_000,
        }
    }

    fn sample(sequence: u64, source_timestamp_ns: u64) -> TrackerPoseSample {
        TrackerPoseSample {
            sequence,
            source_timestamp_ns,
            orientation: UnitQuaternion::IDENTITY,
        }
    }

    #[test]
    fn capacity_is_explicitly_bounded() {
        assert!(matches!(
            create_head_pose_delivery_bridge(policy(), anchor(), 1),
            Err(HeadPoseDeliveryCreateError::InvalidCapacity)
        ));
        assert!(matches!(
            create_head_pose_delivery_bridge(policy(), anchor(), 1025),
            Err(HeadPoseDeliveryCreateError::InvalidCapacity)
        ));
        let (producer, control) = create_head_pose_delivery_bridge(policy(), anchor(), 2).unwrap();
        assert_eq!(producer.capacity(), 2);
        assert_eq!(control.queued_events(), 0);
    }

    #[test]
    fn queue_overflow_rejects_without_discarding_accepted_events() {
        let (producer, mut control) =
            create_head_pose_delivery_bridge(policy(), anchor(), 2).unwrap();
        producer.try_push_sample(sample(1, 1_000_000_000)).unwrap();
        producer.try_push_sample(sample(2, 1_010_000_000)).unwrap();
        assert_eq!(
            producer.try_push_sample(sample(3, 1_020_000_000)),
            Err(HeadPoseIngressPushError::Overflow)
        );
        assert!(matches!(
            control.try_poll(48_000),
            Some(HeadPoseControlEvent::Mapped(HeadPoseSample {
                sequence: 1,
                ..
            }))
        ));
        assert!(matches!(
            control.try_poll(48_480),
            Some(HeadPoseControlEvent::Mapped(HeadPoseSample {
                sequence: 2,
                ..
            }))
        ));
        assert_eq!(control.try_poll(48_960), None);
    }

    #[test]
    fn mapper_rejection_is_transactional_for_following_sample() {
        let (producer, mut control) =
            create_head_pose_delivery_bridge(policy(), anchor(), 4).unwrap();
        producer.try_push_sample(sample(10, 1_000_000_000)).unwrap();
        producer.try_push_sample(sample(10, 1_010_000_000)).unwrap();
        producer.try_push_sample(sample(11, 1_010_000_000)).unwrap();

        assert!(matches!(
            control.try_poll(48_000),
            Some(HeadPoseControlEvent::Mapped(HeadPoseSample {
                sequence: 10,
                ..
            }))
        ));
        assert!(matches!(
            control.try_poll(48_480),
            Some(HeadPoseControlEvent::Rejected {
                sequence: 10,
                error: HeadPoseClockError::NonMonotonicSequence { .. },
            })
        ));
        assert!(matches!(
            control.try_poll(48_480),
            Some(HeadPoseControlEvent::Mapped(HeadPoseSample {
                sequence: 11,
                ..
            }))
        ));
    }

    #[test]
    fn disconnect_blocks_samples_until_explicit_reanchor() {
        let (producer, mut control) =
            create_head_pose_delivery_bridge(policy(), anchor(), 8).unwrap();
        producer
            .try_push_sample(sample(100, 1_000_000_000))
            .unwrap();
        producer.try_push_disconnected().unwrap();
        producer
            .try_push_sample(sample(101, 1_010_000_000))
            .unwrap();
        let next_anchor = HeadPoseClockAnchor {
            source_timestamp_ns: 10_000,
            media_frame: 50_000,
        };
        producer.try_push_reanchor(next_anchor).unwrap();
        producer.try_push_sample(sample(1, 10_000)).unwrap();

        assert!(matches!(
            control.try_poll(48_000),
            Some(HeadPoseControlEvent::Mapped(HeadPoseSample {
                sequence: 100,
                ..
            }))
        ));
        assert_eq!(
            control.try_poll(48_480),
            Some(HeadPoseControlEvent::Disconnected)
        );
        assert_eq!(
            control.try_poll(48_480),
            Some(HeadPoseControlEvent::RejectedWhileDisconnected { sequence: 101 })
        );
        assert_eq!(
            control.try_poll(50_000),
            Some(HeadPoseControlEvent::Reanchored(next_anchor))
        );
        assert_eq!(control.anchor(), next_anchor);
        assert!(matches!(
            control.try_poll(50_000),
            Some(HeadPoseControlEvent::Mapped(HeadPoseSample {
                sequence: 1,
                ..
            }))
        ));
    }

    #[test]
    fn prepared_push_and_single_event_poll_are_allocation_free() {
        let (producer, mut control) =
            create_head_pose_delivery_bridge(policy(), anchor(), 2).unwrap();
        let allocations = count_allocations(|| {
            for index in 0_u64..10_000 {
                let sequence = index + 1;
                let source_timestamp_ns = 1_000_000_000 + index * 1_000_000;
                let observed_media_frame = 48_000 + index * 48;
                producer
                    .try_push_sample(sample(sequence, source_timestamp_ns))
                    .unwrap();
                assert!(matches!(
                    control.try_poll(observed_media_frame),
                    Some(HeadPoseControlEvent::Mapped(_))
                ));
            }
        });
        assert_eq!(allocations, 0);
    }
}
