//! Hardware-neutral tracker timestamp -> Aurora media-frame mapping.
//!
//! Device I/O and tracker-specific clock acquisition remain outside this module. Control
//! code supplies an explicit source-clock/media-frame anchor and the Aurora media frame
//! observed when each sample is delivered. Mapping is deterministic, allocation-free and
//! fail-closed; resets are explicit and never inferred from sequence/timestamp rollback.

use crate::{HeadPoseSample, UnitQuaternion};

const NANOS_PER_SECOND: u128 = 1_000_000_000;
const HALF_NANOSECOND_SCALE: u128 = NANOS_PER_SECOND / 2;

/// One tracker sample expressed only in its source clock domain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackerPoseSample {
    /// Strictly increasing tracker/source sequence within one mapping epoch.
    pub sequence: u64,
    /// Monotonic source timestamp in nanoseconds within one mapping epoch.
    pub source_timestamp_ns: u64,
    /// Validated normalized head orientation.
    pub orientation: UnitQuaternion,
}

/// Explicit correspondence between one tracker timestamp and one Aurora logical frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeadPoseClockAnchor {
    /// Source-clock timestamp used as the mapping origin.
    pub source_timestamp_ns: u64,
    /// Aurora logical media frame corresponding to the source timestamp.
    pub media_frame: u64,
}

/// Bounded policy for source-clock mapping and delivery freshness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeadPoseClockPolicy {
    sample_rate: u32,
    max_source_gap_ns: u64,
    max_delivery_lag_frames: u64,
    max_future_lead_frames: u64,
}

impl HeadPoseClockPolicy {
    /// Creates a bounded mapper policy.
    ///
    /// Aurora audio rates from 8-192 kHz are accepted. `max_source_gap_ns` must be
    /// nonzero. Lag/future budgets may be zero for an exact-only delivery contract.
    pub fn new(
        sample_rate: u32,
        max_source_gap_ns: u64,
        max_delivery_lag_frames: u64,
        max_future_lead_frames: u64,
    ) -> Result<Self, HeadPoseClockError> {
        if !(8_000..=192_000).contains(&sample_rate) || max_source_gap_ns == 0 {
            return Err(HeadPoseClockError::InvalidPolicy);
        }
        Ok(Self {
            sample_rate,
            max_source_gap_ns,
            max_delivery_lag_frames,
            max_future_lead_frames,
        })
    }

    /// Aurora logical sample rate used for timestamp conversion.
    pub const fn sample_rate(self) -> u32 {
        self.sample_rate
    }

    /// Maximum accepted source timestamp gap between consecutive samples.
    pub const fn max_source_gap_ns(self) -> u64 {
        self.max_source_gap_ns
    }

    /// Maximum age of a mapped pose at the control-side delivery observation point.
    pub const fn max_delivery_lag_frames(self) -> u64 {
        self.max_delivery_lag_frames
    }

    /// Maximum amount a mapped pose may lead the observed Aurora media frame.
    pub const fn max_future_lead_frames(self) -> u64 {
        self.max_future_lead_frames
    }
}

/// Fail-closed tracker-clock mapping errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadPoseClockError {
    /// Unsupported sample rate or zero source-gap budget.
    InvalidPolicy,
    /// Source sequence did not strictly advance within this epoch.
    NonMonotonicSequence { previous: u64, actual: u64 },
    /// Source timestamp did not strictly advance within this epoch.
    NonMonotonicTimestamp { previous: u64, actual: u64 },
    /// Source timestamp predates the explicit mapping anchor.
    BeforeAnchor {
        anchor_timestamp_ns: u64,
        actual_timestamp_ns: u64,
    },
    /// Consecutive source samples are separated by more than the configured gap budget.
    SourceGapTooLarge { gap_ns: u64, maximum_ns: u64 },
    /// Timestamp conversion or anchor addition exceeded the logical frame range.
    FrameOverflow,
    /// Two advancing source samples mapped to a non-advancing logical frame.
    NonMonotonicMappedFrame { previous: u64, actual: u64 },
    /// Sample arrived too late relative to the supplied Aurora observation frame.
    DeliveryTooLate {
        lag_frames: u64,
        maximum_frames: u64,
    },
    /// Sample maps too far ahead of the supplied Aurora observation frame.
    DeliveryTooEarly {
        lead_frames: u64,
        maximum_frames: u64,
    },
}

/// Allocation-free mapping state for one explicit tracker-clock epoch.
#[derive(Debug, Clone, Copy)]
pub struct HeadPoseClockMapper {
    policy: HeadPoseClockPolicy,
    anchor: HeadPoseClockAnchor,
    previous_sequence: Option<u64>,
    previous_source_timestamp_ns: Option<u64>,
    previous_media_frame: Option<u64>,
}

impl HeadPoseClockMapper {
    /// Creates a mapper for one explicit clock epoch.
    pub const fn new(policy: HeadPoseClockPolicy, anchor: HeadPoseClockAnchor) -> Self {
        Self {
            policy,
            anchor,
            previous_sequence: None,
            previous_source_timestamp_ns: None,
            previous_media_frame: None,
        }
    }

    /// Explicitly starts a new mapping epoch and forgets prior monotonic state.
    ///
    /// Callers must use this after tracker reconnect, clock reset, timestamp jump, or any
    /// discontinuity that invalidates the old source-clock/media-frame relationship.
    pub fn reset(&mut self, anchor: HeadPoseClockAnchor) {
        self.anchor = anchor;
        self.previous_sequence = None;
        self.previous_source_timestamp_ns = None;
        self.previous_media_frame = None;
    }

    /// Maps one tracker sample into Aurora logical time transactionally.
    ///
    /// `observed_media_frame` is the Aurora logical frame observed by control code when
    /// this sample is delivered. It is used only for bounded lag/future validation; the
    /// mapped frame itself comes exclusively from the explicit anchor and source timestamp.
    /// No mapper state advances when validation fails.
    pub fn map(
        &mut self,
        sample: TrackerPoseSample,
        observed_media_frame: u64,
    ) -> Result<HeadPoseSample, HeadPoseClockError> {
        if let Some(previous) = self.previous_sequence {
            if sample.sequence <= previous {
                return Err(HeadPoseClockError::NonMonotonicSequence {
                    previous,
                    actual: sample.sequence,
                });
            }
        }
        if let Some(previous) = self.previous_source_timestamp_ns {
            if sample.source_timestamp_ns <= previous {
                return Err(HeadPoseClockError::NonMonotonicTimestamp {
                    previous,
                    actual: sample.source_timestamp_ns,
                });
            }
            let gap_ns = sample.source_timestamp_ns - previous;
            if gap_ns > self.policy.max_source_gap_ns {
                return Err(HeadPoseClockError::SourceGapTooLarge {
                    gap_ns,
                    maximum_ns: self.policy.max_source_gap_ns,
                });
            }
        }
        if sample.source_timestamp_ns < self.anchor.source_timestamp_ns {
            return Err(HeadPoseClockError::BeforeAnchor {
                anchor_timestamp_ns: self.anchor.source_timestamp_ns,
                actual_timestamp_ns: sample.source_timestamp_ns,
            });
        }

        let elapsed_ns = sample.source_timestamp_ns - self.anchor.source_timestamp_ns;
        let scaled = u128::from(elapsed_ns) * u128::from(self.policy.sample_rate);
        // Nearest logical frame, half-way values rounded forward. The integer conversion
        // makes the result deterministic and avoids accumulating floating-point drift.
        let offset_frames = (scaled + HALF_NANOSECOND_SCALE) / NANOS_PER_SECOND;
        let offset_frames =
            u64::try_from(offset_frames).map_err(|_| HeadPoseClockError::FrameOverflow)?;
        let media_frame = self
            .anchor
            .media_frame
            .checked_add(offset_frames)
            .ok_or(HeadPoseClockError::FrameOverflow)?;

        if let Some(previous) = self.previous_media_frame {
            if media_frame <= previous {
                return Err(HeadPoseClockError::NonMonotonicMappedFrame {
                    previous,
                    actual: media_frame,
                });
            }
        }

        if media_frame <= observed_media_frame {
            let lag_frames = observed_media_frame - media_frame;
            if lag_frames > self.policy.max_delivery_lag_frames {
                return Err(HeadPoseClockError::DeliveryTooLate {
                    lag_frames,
                    maximum_frames: self.policy.max_delivery_lag_frames,
                });
            }
        } else {
            let lead_frames = media_frame - observed_media_frame;
            if lead_frames > self.policy.max_future_lead_frames {
                return Err(HeadPoseClockError::DeliveryTooEarly {
                    lead_frames,
                    maximum_frames: self.policy.max_future_lead_frames,
                });
            }
        }

        self.previous_sequence = Some(sample.sequence);
        self.previous_source_timestamp_ns = Some(sample.source_timestamp_ns);
        self.previous_media_frame = Some(media_frame);
        Ok(HeadPoseSample {
            sequence: sample.sequence,
            media_frame,
            orientation: sample.orientation,
        })
    }

    /// Current mapping anchor.
    pub const fn anchor(self) -> HeadPoseClockAnchor {
        self.anchor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> HeadPoseClockPolicy {
        HeadPoseClockPolicy::new(48_000, 20_000_000, 240, 48).unwrap()
    }

    fn sample(sequence: u64, source_timestamp_ns: u64) -> TrackerPoseSample {
        TrackerPoseSample {
            sequence,
            source_timestamp_ns,
            orientation: UnitQuaternion::IDENTITY,
        }
    }

    #[test]
    fn timestamp_mapping_is_deterministic_and_anchor_relative() {
        let anchor = HeadPoseClockAnchor {
            source_timestamp_ns: 1_000_000_000,
            media_frame: 96_000,
        };
        let mut mapper = HeadPoseClockMapper::new(policy(), anchor);
        assert_eq!(
            mapper
                .map(sample(10, 1_000_000_000), 96_000)
                .unwrap()
                .media_frame,
            96_000
        );
        assert_eq!(
            mapper
                .map(sample(11, 1_010_000_000), 96_480)
                .unwrap()
                .media_frame,
            96_480
        );
        assert_eq!(
            mapper
                .map(sample(12, 1_011_000_000), 96_528)
                .unwrap()
                .media_frame,
            96_528
        );
    }

    #[test]
    fn invalid_delivery_does_not_advance_transactional_state() {
        let anchor = HeadPoseClockAnchor {
            source_timestamp_ns: 0,
            media_frame: 0,
        };
        let mut mapper = HeadPoseClockMapper::new(policy(), anchor);
        assert_eq!(
            mapper.map(sample(1, 10_000_000), 1_000),
            Err(HeadPoseClockError::DeliveryTooLate {
                lag_frames: 520,
                maximum_frames: 240,
            })
        );
        // Same source sample remains admissible when delivered under the stated budget.
        assert_eq!(
            mapper.map(sample(1, 10_000_000), 480).unwrap().media_frame,
            480
        );
    }

    #[test]
    fn duplicate_reorder_and_timestamp_rollback_fail_closed() {
        let anchor = HeadPoseClockAnchor {
            source_timestamp_ns: 1_000,
            media_frame: 10,
        };
        let mut mapper = HeadPoseClockMapper::new(policy(), anchor);
        mapper.map(sample(5, 1_001_000), 58).unwrap();
        assert_eq!(
            mapper.map(sample(5, 2_001_000), 106),
            Err(HeadPoseClockError::NonMonotonicSequence {
                previous: 5,
                actual: 5,
            })
        );
        assert_eq!(
            mapper.map(sample(6, 1_000_000), 58),
            Err(HeadPoseClockError::NonMonotonicTimestamp {
                previous: 1_001_000,
                actual: 1_000_000,
            })
        );
    }

    #[test]
    fn source_gap_and_future_lead_are_bounded() {
        let anchor = HeadPoseClockAnchor {
            source_timestamp_ns: 0,
            media_frame: 0,
        };
        let mut mapper = HeadPoseClockMapper::new(policy(), anchor);
        mapper.map(sample(1, 1_000_000), 48).unwrap();
        assert_eq!(
            mapper.map(sample(2, 22_000_000), 1_056),
            Err(HeadPoseClockError::SourceGapTooLarge {
                gap_ns: 21_000_000,
                maximum_ns: 20_000_000,
            })
        );
        assert_eq!(
            mapper.map(sample(2, 2_000_000), 0),
            Err(HeadPoseClockError::DeliveryTooEarly {
                lead_frames: 96,
                maximum_frames: 48,
            })
        );
        assert_eq!(
            mapper.map(sample(2, 2_000_000), 48).unwrap().media_frame,
            96
        );
    }

    #[test]
    fn mapped_frames_must_advance_even_when_source_clock_does() {
        let anchor = HeadPoseClockAnchor {
            source_timestamp_ns: 0,
            media_frame: 0,
        };
        let mut mapper = HeadPoseClockMapper::new(policy(), anchor);
        mapper.map(sample(1, 1_000), 0).unwrap();
        assert_eq!(
            mapper.map(sample(2, 2_000), 0),
            Err(HeadPoseClockError::NonMonotonicMappedFrame {
                previous: 0,
                actual: 0,
            })
        );
    }

    #[test]
    fn reset_is_required_to_accept_a_new_clock_epoch() {
        let mut mapper = HeadPoseClockMapper::new(
            policy(),
            HeadPoseClockAnchor {
                source_timestamp_ns: 1_000_000,
                media_frame: 100,
            },
        );
        mapper.map(sample(100, 2_000_000), 148).unwrap();
        assert!(matches!(
            mapper.map(sample(1, 10_000), 200),
            Err(HeadPoseClockError::NonMonotonicSequence { .. })
        ));
        mapper.reset(HeadPoseClockAnchor {
            source_timestamp_ns: 10_000,
            media_frame: 200,
        });
        assert_eq!(mapper.map(sample(1, 10_000), 200).unwrap().media_frame, 200);
    }

    #[test]
    fn timestamp_before_anchor_and_frame_overflow_fail_closed() {
        let mut mapper = HeadPoseClockMapper::new(
            policy(),
            HeadPoseClockAnchor {
                source_timestamp_ns: 1_000,
                media_frame: 0,
            },
        );
        assert_eq!(
            mapper.map(sample(1, 999), 0),
            Err(HeadPoseClockError::BeforeAnchor {
                anchor_timestamp_ns: 1_000,
                actual_timestamp_ns: 999,
            })
        );

        let mut overflow = HeadPoseClockMapper::new(
            policy(),
            HeadPoseClockAnchor {
                source_timestamp_ns: 0,
                media_frame: u64::MAX,
            },
        );
        assert_eq!(
            overflow.map(sample(1, 1_000_000), u64::MAX),
            Err(HeadPoseClockError::FrameOverflow)
        );
    }
}
