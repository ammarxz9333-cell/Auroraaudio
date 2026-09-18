//! Deterministic head-tracker delivery traces for AuroraSim.
//!
//! This module models source timestamps, delivery timing and control-plane faults only.
//! It performs no device I/O and makes no claim about physical tracker latency, sensor
//! quality or operating-system timestamp accuracy.

use aurora_renderer_api::{
    HeadPoseClockAnchor, HeadPoseClockError, HeadPoseClockMapper, HeadPoseClockPolicy,
    HeadPoseSample, TrackerPoseSample, UnitQuaternion,
};

use crate::DeterministicRng;

const NANOS_PER_SECOND: u128 = 1_000_000_000;
const HALF_NANOSECOND_SCALE: u128 = NANOS_PER_SECOND / 2;

/// One deterministic tracker-delivery fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadTrackerFault {
    /// Omits a consecutive source-sample burst while source time keeps advancing.
    DropBurst {
        /// Zero-based sample index at which omission starts.
        at_sample: usize,
        /// Number of consecutive source samples omitted.
        count: usize,
    },
    /// Delivers the next pair in reverse order.
    ReorderPair {
        /// Zero-based index of the first source sample in the pair.
        at_sample: usize,
    },
    /// Delivers one source sample twice.
    Duplicate {
        /// Zero-based sample index to duplicate.
        at_sample: usize,
    },
    /// Applies a persistent source-clock timestamp step from this sample onward.
    TimestampJump {
        /// Zero-based sample index at which the jump starts.
        at_sample: usize,
        /// Signed source-clock step in nanoseconds.
        delta_ns: i64,
    },
    /// Delays one delivery beyond its nominal delivery point.
    StaleHold {
        /// Zero-based sample index to hold.
        at_sample: usize,
        /// Extra Aurora frames added to the observed delivery time.
        extra_frames: u64,
    },
    /// Disconnects and starts a new explicit tracker clock epoch.
    Reconnect {
        /// Zero-based sample index that becomes the first sample of the new epoch.
        at_sample: usize,
        /// Aurora-frame downtime inserted before the new epoch starts.
        downtime_frames: u64,
        /// First source timestamp in the new epoch.
        new_source_timestamp_ns: u64,
        /// First source sequence in the new epoch.
        new_sequence: u64,
    },
}

impl HeadTrackerFault {
    fn range(self) -> Option<std::ops::Range<usize>> {
        match self {
            Self::DropBurst { at_sample, count } if count > 0 => {
                at_sample.checked_add(count).map(|end| at_sample..end)
            }
            Self::ReorderPair { at_sample } => {
                at_sample.checked_add(2).map(|end| at_sample..end)
            }
            Self::Duplicate { at_sample }
            | Self::TimestampJump { at_sample, .. }
            | Self::StaleHold { at_sample, .. }
            | Self::Reconnect { at_sample, .. } => {
                at_sample.checked_add(1).map(|end| at_sample..end)
            }
            Self::DropBurst { .. } => None,
        }
    }

    fn at_sample(self) -> usize {
        match self {
            Self::DropBurst { at_sample, .. }
            | Self::ReorderPair { at_sample }
            | Self::Duplicate { at_sample }
            | Self::TimestampJump { at_sample, .. }
            | Self::StaleHold { at_sample, .. }
            | Self::Reconnect { at_sample, .. } => at_sample,
        }
    }
}

/// Deterministic source and delivery timing used to build one tracker trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadTrackerSimulationConfig {
    /// Seed for bounded delivery jitter.
    pub seed: u64,
    /// Aurora logical sample rate.
    pub sample_rate: u32,
    /// Source tracker cadence in nanoseconds.
    pub sample_period_ns: u64,
    /// Number of source sample slots to simulate.
    pub sample_count: usize,
    /// First source sequence in the initial epoch.
    pub start_sequence: u64,
    /// Explicit initial source-clock/media-frame correspondence.
    pub initial_anchor: HeadPoseClockAnchor,
    /// Nominal control-side delivery lag after the represented media frame.
    pub delivery_lag_frames: u64,
    /// Symmetric deterministic delivery-jitter bound.
    pub delivery_jitter_frames: u64,
    /// Scripted source/delivery faults.
    pub faults: Vec<HeadTrackerFault>,
}

impl HeadTrackerSimulationConfig {
    /// Validates the bounded trace contract before generation.
    pub fn validate(&self) -> Result<(), HeadTrackerSimulationError> {
        if !(8_000..=192_000).contains(&self.sample_rate)
            || self.sample_period_ns == 0
            || self.sample_count == 0
            || self.delivery_jitter_frames > i64::MAX as u64 / 2
        {
            return Err(HeadTrackerSimulationError::InvalidConfig);
        }

        for (index, fault) in self.faults.iter().copied().enumerate() {
            let range = fault
                .range()
                .ok_or(HeadTrackerSimulationError::InvalidFault)?;
            if range.start >= self.sample_count || range.end > self.sample_count {
                return Err(HeadTrackerSimulationError::FaultOutOfRange);
            }
            match fault {
                HeadTrackerFault::TimestampJump { delta_ns: 0, .. }
                | HeadTrackerFault::StaleHold {
                    extra_frames: 0, ..
                } => return Err(HeadTrackerSimulationError::InvalidFault),
                _ => {}
            }

            for previous in self.faults[..index].iter().copied() {
                let previous_range = previous
                    .range()
                    .ok_or(HeadTrackerSimulationError::InvalidFault)?;
                if range.start < previous_range.end && previous_range.start < range.end {
                    return Err(HeadTrackerSimulationError::OverlappingFaults);
                }
            }
        }
        Ok(())
    }
}

/// One raw AuroraSim tracker-delivery event.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HeadTrackerDeliveryEvent {
    /// A source sample reached control code at the stated Aurora frame.
    Sample {
        /// Tracker/source sample.
        sample: TrackerPoseSample,
        /// Aurora logical frame observed when the sample was delivered.
        observed_media_frame: u64,
    },
    /// One source sample was intentionally omitted.
    Dropped {
        /// Sequence that would have been delivered.
        sequence: u64,
    },
    /// The simulated tracker disconnected.
    Disconnected {
        /// Aurora frame at which the old epoch stopped.
        media_frame: u64,
    },
    /// Control code received an explicit new source-clock/media-frame anchor.
    Reanchor {
        /// New mapping epoch origin.
        anchor: HeadPoseClockAnchor,
    },
}

/// Generated deterministic trace plus the initial mapping anchor.
#[derive(Debug, Clone, PartialEq)]
pub struct HeadTrackerTrace {
    /// Initial mapping epoch origin.
    pub initial_anchor: HeadPoseClockAnchor,
    /// Ordered delivery/fault events.
    pub events: Vec<HeadTrackerDeliveryEvent>,
}

/// Result of applying a generated trace to HeadPoseClockMapper.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HeadTrackerSimulationRecord {
    /// Sample mapped successfully into Aurora logical time.
    Accepted(HeadPoseSample),
    /// Sample was rejected transactionally by the mapper.
    Rejected {
        /// Rejected source sequence.
        sequence: u64,
        /// Fail-closed mapping reason.
        error: HeadPoseClockError,
    },
    /// Source sample was omitted by the simulator.
    Dropped {
        /// Omitted source sequence.
        sequence: u64,
    },
    /// Old tracker epoch ended.
    Disconnected {
        /// Aurora frame at disconnect.
        media_frame: u64,
    },
    /// Mapper was explicitly reset onto a new epoch.
    Reanchored {
        /// New explicit mapping anchor.
        anchor: HeadPoseClockAnchor,
    },
}

/// Deterministic mapper report for one generated trace.
#[derive(Debug, Clone, PartialEq)]
pub struct HeadTrackerSimulationReport {
    /// Ordered mapping/fault records.
    pub records: Vec<HeadTrackerSimulationRecord>,
}

impl HeadTrackerSimulationReport {
    /// Number of successfully mapped samples.
    pub fn accepted(&self) -> usize {
        self.records
            .iter()
            .filter(|record| matches!(record, HeadTrackerSimulationRecord::Accepted(_)))
            .count()
    }

    /// Number of fail-closed mapping rejections.
    pub fn rejected(&self) -> usize {
        self.records
            .iter()
            .filter(|record| matches!(record, HeadTrackerSimulationRecord::Rejected { .. }))
            .count()
    }

    /// Number of intentionally dropped source samples.
    pub fn dropped(&self) -> usize {
        self.records
            .iter()
            .filter(|record| matches!(record, HeadTrackerSimulationRecord::Dropped { .. }))
            .count()
    }
}

/// Trace construction failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadTrackerSimulationError {
    /// Sample rate, cadence, count or jitter bound is invalid.
    InvalidConfig,
    /// Fault has an empty/meaningless range.
    InvalidFault,
    /// Fault references sample slots outside the configured trace.
    FaultOutOfRange,
    /// Two scripted faults consume overlapping source-sample slots.
    OverlappingFaults,
    /// Sequence, timestamp or media-frame arithmetic overflowed.
    ArithmeticOverflow,
}

#[derive(Debug, Clone, Copy)]
struct SimulatedSample {
    sample: TrackerPoseSample,
    capture_media_frame: u64,
    observed_media_frame: u64,
}

fn frame_offset(sample_rate: u32, elapsed_ns: u128) -> Result<u64, HeadTrackerSimulationError> {
    let scaled = elapsed_ns
        .checked_mul(u128::from(sample_rate))
        .ok_or(HeadTrackerSimulationError::ArithmeticOverflow)?;
    let frames = (scaled + HALF_NANOSECOND_SCALE) / NANOS_PER_SECOND;
    u64::try_from(frames).map_err(|_| HeadTrackerSimulationError::ArithmeticOverflow)
}

fn add_signed(value: u64, delta: i128) -> Result<u64, HeadTrackerSimulationError> {
    let value = i128::from(value)
        .checked_add(delta)
        .ok_or(HeadTrackerSimulationError::ArithmeticOverflow)?;
    if !(0..=i128::from(u64::MAX)).contains(&value) {
        return Err(HeadTrackerSimulationError::ArithmeticOverflow);
    }
    Ok(value as u64)
}

fn build_sample(
    config: &HeadTrackerSimulationConfig,
    rng: &mut DeterministicRng,
    epoch_anchor: HeadPoseClockAnchor,
    epoch_index: usize,
    epoch_sequence: u64,
    timestamp_offset_ns: i128,
    extra_delivery_frames: u64,
) -> Result<SimulatedSample, HeadTrackerSimulationError> {
    let epoch_index =
        u64::try_from(epoch_index).map_err(|_| HeadTrackerSimulationError::ArithmeticOverflow)?;
    let elapsed_ns = u128::from(config.sample_period_ns)
        .checked_mul(u128::from(epoch_index))
        .ok_or(HeadTrackerSimulationError::ArithmeticOverflow)?;
    let elapsed_ns_u64 =
        u64::try_from(elapsed_ns).map_err(|_| HeadTrackerSimulationError::ArithmeticOverflow)?;

    let base_timestamp = epoch_anchor
        .source_timestamp_ns
        .checked_add(elapsed_ns_u64)
        .ok_or(HeadTrackerSimulationError::ArithmeticOverflow)?;
    let source_timestamp_ns = add_signed(base_timestamp, timestamp_offset_ns)?;
    let capture_media_frame = epoch_anchor
        .media_frame
        .checked_add(frame_offset(config.sample_rate, elapsed_ns)?)
        .ok_or(HeadTrackerSimulationError::ArithmeticOverflow)?;
    let sequence = epoch_sequence
        .checked_add(epoch_index)
        .ok_or(HeadTrackerSimulationError::ArithmeticOverflow)?;

    let jitter_frames = if config.delivery_jitter_frames == 0 {
        0_i128
    } else {
        let width = config
            .delivery_jitter_frames
            .checked_mul(2)
            .and_then(|value| value.checked_add(1))
            .ok_or(HeadTrackerSimulationError::ArithmeticOverflow)?;
        i128::from(rng.next_u64() % width) - i128::from(config.delivery_jitter_frames)
    };
    let nominal_delivery = capture_media_frame
        .checked_add(config.delivery_lag_frames)
        .and_then(|value| value.checked_add(extra_delivery_frames))
        .ok_or(HeadTrackerSimulationError::ArithmeticOverflow)?;
    let observed_media_frame = add_signed(nominal_delivery, jitter_frames)?;

    Ok(SimulatedSample {
        sample: TrackerPoseSample {
            sequence,
            source_timestamp_ns,
            orientation: UnitQuaternion::IDENTITY,
        },
        capture_media_frame,
        observed_media_frame,
    })
}

/// Generates one deterministic tracker delivery trace.
pub fn generate_head_tracker_trace(
    config: &HeadTrackerSimulationConfig,
) -> Result<HeadTrackerTrace, HeadTrackerSimulationError> {
    config.validate()?;

    let mut rng = DeterministicRng::new(config.seed);
    let mut events = Vec::with_capacity(config.sample_count + config.faults.len() * 2);
    let mut sample_index = 0_usize;
    let mut epoch_index = 0_usize;
    let mut epoch_anchor = config.initial_anchor;
    let mut epoch_sequence = config.start_sequence;
    let mut timestamp_offset_ns = 0_i128;

    while sample_index < config.sample_count {
        let fault = config
            .faults
            .iter()
            .copied()
            .find(|fault| fault.at_sample() == sample_index);

        match fault {
            Some(HeadTrackerFault::DropBurst { count, .. }) => {
                for _ in 0..count {
                    let sample = build_sample(
                        config,
                        &mut rng,
                        epoch_anchor,
                        epoch_index,
                        epoch_sequence,
                        timestamp_offset_ns,
                        0,
                    )?;
                    events.push(HeadTrackerDeliveryEvent::Dropped {
                        sequence: sample.sample.sequence,
                    });
                    sample_index += 1;
                    epoch_index += 1;
                }
            }
            Some(HeadTrackerFault::ReorderPair { .. }) => {
                let first = build_sample(
                    config,
                    &mut rng,
                    epoch_anchor,
                    epoch_index,
                    epoch_sequence,
                    timestamp_offset_ns,
                    0,
                )?;
                let second = build_sample(
                    config,
                    &mut rng,
                    epoch_anchor,
                    epoch_index + 1,
                    epoch_sequence,
                    timestamp_offset_ns,
                    0,
                )?;
                let delivery_floor = first.observed_media_frame.max(second.observed_media_frame);
                events.push(HeadTrackerDeliveryEvent::Sample {
                    sample: second.sample,
                    observed_media_frame: delivery_floor,
                });
                events.push(HeadTrackerDeliveryEvent::Sample {
                    sample: first.sample,
                    observed_media_frame: delivery_floor
                        .checked_add(1)
                        .ok_or(HeadTrackerSimulationError::ArithmeticOverflow)?,
                });
                sample_index += 2;
                epoch_index += 2;
            }
            Some(HeadTrackerFault::Duplicate { .. }) => {
                let sample = build_sample(
                    config,
                    &mut rng,
                    epoch_anchor,
                    epoch_index,
                    epoch_sequence,
                    timestamp_offset_ns,
                    0,
                )?;
                events.push(HeadTrackerDeliveryEvent::Sample {
                    sample: sample.sample,
                    observed_media_frame: sample.observed_media_frame,
                });
                events.push(HeadTrackerDeliveryEvent::Sample {
                    sample: sample.sample,
                    observed_media_frame: sample
                        .observed_media_frame
                        .checked_add(1)
                        .ok_or(HeadTrackerSimulationError::ArithmeticOverflow)?,
                });
                sample_index += 1;
                epoch_index += 1;
            }
            Some(HeadTrackerFault::TimestampJump { delta_ns, .. }) => {
                timestamp_offset_ns = timestamp_offset_ns
                    .checked_add(i128::from(delta_ns))
                    .ok_or(HeadTrackerSimulationError::ArithmeticOverflow)?;
                let sample = build_sample(
                    config,
                    &mut rng,
                    epoch_anchor,
                    epoch_index,
                    epoch_sequence,
                    timestamp_offset_ns,
                    0,
                )?;
                events.push(HeadTrackerDeliveryEvent::Sample {
                    sample: sample.sample,
                    observed_media_frame: sample.observed_media_frame,
                });
                sample_index += 1;
                epoch_index += 1;
            }
            Some(HeadTrackerFault::StaleHold { extra_frames, .. }) => {
                let sample = build_sample(
                    config,
                    &mut rng,
                    epoch_anchor,
                    epoch_index,
                    epoch_sequence,
                    timestamp_offset_ns,
                    extra_frames,
                )?;
                events.push(HeadTrackerDeliveryEvent::Sample {
                    sample: sample.sample,
                    observed_media_frame: sample.observed_media_frame,
                });
                sample_index += 1;
                epoch_index += 1;
            }
            Some(HeadTrackerFault::Reconnect {
                downtime_frames,
                new_source_timestamp_ns,
                new_sequence,
                ..
            }) => {
                let old_boundary = build_sample(
                    config,
                    &mut rng,
                    epoch_anchor,
                    epoch_index,
                    epoch_sequence,
                    timestamp_offset_ns,
                    0,
                )?
                .capture_media_frame;
                let new_media_frame = old_boundary
                    .checked_add(downtime_frames)
                    .ok_or(HeadTrackerSimulationError::ArithmeticOverflow)?;
                events.push(HeadTrackerDeliveryEvent::Disconnected {
                    media_frame: old_boundary,
                });
                epoch_anchor = HeadPoseClockAnchor {
                    source_timestamp_ns: new_source_timestamp_ns,
                    media_frame: new_media_frame,
                };
                events.push(HeadTrackerDeliveryEvent::Reanchor {
                    anchor: epoch_anchor,
                });
                epoch_sequence = new_sequence;
                epoch_index = 0;
                timestamp_offset_ns = 0;

                let sample = build_sample(
                    config,
                    &mut rng,
                    epoch_anchor,
                    epoch_index,
                    epoch_sequence,
                    timestamp_offset_ns,
                    0,
                )?;
                events.push(HeadTrackerDeliveryEvent::Sample {
                    sample: sample.sample,
                    observed_media_frame: sample.observed_media_frame,
                });
                sample_index += 1;
                epoch_index += 1;
            }
            None => {
                let sample = build_sample(
                    config,
                    &mut rng,
                    epoch_anchor,
                    epoch_index,
                    epoch_sequence,
                    timestamp_offset_ns,
                    0,
                )?;
                events.push(HeadTrackerDeliveryEvent::Sample {
                    sample: sample.sample,
                    observed_media_frame: sample.observed_media_frame,
                });
                sample_index += 1;
                epoch_index += 1;
            }
        }
    }

    Ok(HeadTrackerTrace {
        initial_anchor: config.initial_anchor,
        events,
    })
}

/// Runs a generated trace through the hardware-neutral clock mapper.
///
/// Reanchor events explicitly reset the mapper; no discontinuity is inferred.
pub fn run_head_tracker_simulation(
    policy: HeadPoseClockPolicy,
    config: &HeadTrackerSimulationConfig,
) -> Result<HeadTrackerSimulationReport, HeadTrackerSimulationError> {
    let trace = generate_head_tracker_trace(config)?;
    let mut mapper = HeadPoseClockMapper::new(policy, trace.initial_anchor);
    let mut records = Vec::with_capacity(trace.events.len());

    for event in trace.events {
        match event {
            HeadTrackerDeliveryEvent::Sample {
                sample,
                observed_media_frame,
            } => match mapper.map(sample, observed_media_frame) {
                Ok(mapped) => records.push(HeadTrackerSimulationRecord::Accepted(mapped)),
                Err(error) => records.push(HeadTrackerSimulationRecord::Rejected {
                    sequence: sample.sequence,
                    error,
                }),
            },
            HeadTrackerDeliveryEvent::Dropped { sequence } => {
                records.push(HeadTrackerSimulationRecord::Dropped { sequence });
            }
            HeadTrackerDeliveryEvent::Disconnected { media_frame } => {
                records.push(HeadTrackerSimulationRecord::Disconnected { media_frame });
            }
            HeadTrackerDeliveryEvent::Reanchor { anchor } => {
                mapper.reset(anchor);
                records.push(HeadTrackerSimulationRecord::Reanchored { anchor });
            }
        }
    }

    Ok(HeadTrackerSimulationReport { records })
}

/// Returns an executable built-in head-tracker profile by coverage name.
pub fn builtin_head_tracker_profile(name: &str) -> Option<HeadTrackerSimulationConfig> {
    let mut config = HeadTrackerSimulationConfig {
        seed: 0xA5A5_5A5A_1234_5678,
        sample_rate: 48_000,
        sample_period_ns: 10_000_000,
        sample_count: 32,
        start_sequence: 100,
        initial_anchor: HeadPoseClockAnchor {
            source_timestamp_ns: 1_000_000_000,
            media_frame: 48_000,
        },
        delivery_lag_frames: 24,
        delivery_jitter_frames: 0,
        faults: Vec::new(),
    };

    match name {
        "head-tracker-healthy" => {}
        "head-tracker-jitter" => config.delivery_jitter_frames = 12,
        "head-tracker-burst-loss" => config.faults.push(HeadTrackerFault::DropBurst {
            at_sample: 8,
            count: 3,
        }),
        "head-tracker-reorder" => config
            .faults
            .push(HeadTrackerFault::ReorderPair { at_sample: 8 }),
        "head-tracker-duplicate" => config
            .faults
            .push(HeadTrackerFault::Duplicate { at_sample: 8 }),
        "head-tracker-timestamp-jump" => {
            config.faults.push(HeadTrackerFault::TimestampJump {
                at_sample: 8,
                delta_ns: 100_000_000,
            });
        }
        "head-tracker-stale-hold" => config.faults.push(HeadTrackerFault::StaleHold {
            at_sample: 8,
            extra_frames: 400,
        }),
        "head-tracker-reconnect" => config.faults.push(HeadTrackerFault::Reconnect {
            at_sample: 8,
            downtime_frames: 960,
            new_source_timestamp_ns: 10_000,
            new_sequence: 1,
        }),
        _ => return None,
    }
    Some(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> HeadPoseClockPolicy {
        HeadPoseClockPolicy::new(48_000, 25_000_000, 240, 48).unwrap()
    }

    fn report(name: &str) -> HeadTrackerSimulationReport {
        run_head_tracker_simulation(policy(), &builtin_head_tracker_profile(name).unwrap()).unwrap()
    }

    #[test]
    fn same_seed_produces_identical_trace() {
        let config = builtin_head_tracker_profile("head-tracker-jitter").unwrap();
        assert_eq!(
            generate_head_tracker_trace(&config).unwrap(),
            generate_head_tracker_trace(&config).unwrap()
        );
    }

    #[test]
    fn healthy_and_bounded_jitter_streams_map_continuously() {
        for name in ["head-tracker-healthy", "head-tracker-jitter"] {
            let report = report(name);
            assert_eq!(report.accepted(), 32, "{name}");
            assert_eq!(report.rejected(), 0, "{name}");
            assert_eq!(report.dropped(), 0, "{name}");
        }
    }

    #[test]
    fn burst_loss_exposes_gap_and_remains_fail_closed_without_reanchor() {
        let report = report("head-tracker-burst-loss");
        assert_eq!(report.dropped(), 3);
        assert!(report.records.iter().any(|record| matches!(
            record,
            HeadTrackerSimulationRecord::Rejected {
                error: HeadPoseClockError::SourceGapTooLarge { .. },
                ..
            }
        )));
    }

    #[test]
    fn reorder_and_duplicate_reject_only_invalid_delivery_order() {
        for name in ["head-tracker-reorder", "head-tracker-duplicate"] {
            let report = report(name);
            assert!(report.records.iter().any(|record| matches!(
                record,
                HeadTrackerSimulationRecord::Rejected {
                    error: HeadPoseClockError::NonMonotonicSequence { .. },
                    ..
                }
            )));
            assert!(matches!(
                report.records.last(),
                Some(HeadTrackerSimulationRecord::Accepted(_))
            ));
        }
    }

    #[test]
    fn timestamp_jump_is_not_inferred_as_a_new_epoch() {
        let report = report("head-tracker-timestamp-jump");
        assert!(report.records.iter().any(|record| matches!(
            record,
            HeadTrackerSimulationRecord::Rejected {
                error: HeadPoseClockError::SourceGapTooLarge { .. },
                ..
            }
        )));
    }

    #[test]
    fn stale_hold_is_transactional_and_following_sample_recovers() {
        let report = report("head-tracker-stale-hold");
        let rejected = report
            .records
            .iter()
            .position(|record| {
                matches!(
                    record,
                    HeadTrackerSimulationRecord::Rejected {
                        error: HeadPoseClockError::DeliveryTooLate { .. },
                        ..
                    }
                )
            })
            .unwrap();
        assert!(report.records[rejected + 1..]
            .iter()
            .any(|record| matches!(record, HeadTrackerSimulationRecord::Accepted(_))));
    }

    #[test]
    fn reconnect_requires_and_applies_explicit_reanchor() {
        let config = builtin_head_tracker_profile("head-tracker-reconnect").unwrap();
        let trace = generate_head_tracker_trace(&config).unwrap();
        let mut mapper = HeadPoseClockMapper::new(policy(), trace.initial_anchor);
        let mut ignored_reanchor = false;
        let mut restart_rejected = false;

        for event in trace.events.iter().copied() {
            match event {
                HeadTrackerDeliveryEvent::Sample {
                    sample,
                    observed_media_frame,
                } => {
                    let result = mapper.map(sample, observed_media_frame);
                    if ignored_reanchor
                        && matches!(
                            result,
                            Err(HeadPoseClockError::NonMonotonicSequence { .. })
                        )
                    {
                        restart_rejected = true;
                        break;
                    }
                }
                HeadTrackerDeliveryEvent::Reanchor { .. } => ignored_reanchor = true,
                _ => {}
            }
        }
        assert!(restart_rejected);

        let report = run_head_tracker_simulation(policy(), &config).unwrap();
        assert!(report.records.iter().any(|record| matches!(
            record,
            HeadTrackerSimulationRecord::Reanchored { .. }
        )));
        assert!(matches!(
            report.records.last(),
            Some(HeadTrackerSimulationRecord::Accepted(_))
        ));
    }
}
