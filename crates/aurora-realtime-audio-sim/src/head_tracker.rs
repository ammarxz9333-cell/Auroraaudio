//! Deterministic hardware-neutral head-tracker delivery simulation.
//!
//! This module models source timestamps, delivery jitter and control-plane faults only.
//! It deliberately does not know about Aurora renderer types, device APIs or physical
//! tracker behavior. Higher-level assurance code maps these events into Aurora time.

use crate::DeterministicRng;

const NANOS_PER_SECOND: u64 = 1_000_000_000;

/// Fault profile applied to one deterministic tracker-delivery run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadTrackerFaultProfile {
    /// Monotonic source with bounded delivery jitter only.
    Healthy,
    /// Uses the configured bounded delivery jitter on every sample.
    Jitter,
    /// Drops one source sample while source time continues.
    Dropout,
    /// Delivers one sample twice.
    Duplicate,
    /// Swaps one adjacent sample pair.
    Reorder,
    /// Holds delivery long enough for the downstream pose stale budget to expire, then re-anchors.
    StaleHold,
    /// Injects one forward source-timestamp discontinuity.
    TimestampJump,
    /// Explicitly starts a fresh source clock/sequence epoch at the fault boundary.
    Reconnect,
}

impl HeadTrackerFaultProfile {
    /// Stable profile identifier used by evidence reports.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Jitter => "jitter",
            Self::Dropout => "dropout",
            Self::Duplicate => "duplicate",
            Self::Reorder => "reorder",
            Self::StaleHold => "stale-hold",
            Self::TimestampJump => "timestamp-jump",
            Self::Reconnect => "reconnect",
        }
    }

    /// Profiles exercised by the standard assurance lane.
    pub const ALL: [Self; 8] = [
        Self::Healthy,
        Self::Jitter,
        Self::Dropout,
        Self::Duplicate,
        Self::Reorder,
        Self::StaleHold,
        Self::TimestampJump,
        Self::Reconnect,
    ];
}

/// Explicit source-clock/Aurora-frame correspondence used after reconnect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimulatedHeadTrackerAnchor {
    /// Source timestamp at the new epoch origin.
    pub source_timestamp_ns: u64,
    /// Aurora logical frame corresponding to the source timestamp.
    pub media_frame: u64,
}

/// One raw tracker sample before Aurora clock mapping.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimulatedHeadTrackerSample {
    /// Source-local sequence. It may restart only after a `Reconnect` event.
    pub sequence: u64,
    /// Source-local monotonic timestamp in nanoseconds.
    pub source_timestamp_ns: u64,
    /// Quaternion components in `(w, x, y, z)` order.
    pub orientation_wxyz: [f32; 4],
    /// Aurora frame observed by the control plane when this sample is delivered.
    pub observed_media_frame: u64,
}

/// Deterministic control-plane event emitted by the tracker simulator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HeadTrackerDeliveryEvent {
    /// A source sample is delivered to Aurora control code.
    Sample(SimulatedHeadTrackerSample),
    /// A source sample existed but was not delivered.
    Drop {
        /// Source-local sequence that was lost.
        sequence: u64,
        /// Source timestamp of the lost sample.
        source_timestamp_ns: u64,
        /// Nominal Aurora frame corresponding to this simulation instant.
        nominal_media_frame: u64,
    },
    /// The consumer should prove that an already accepted pose is now stale.
    StaleProbe {
        /// Aurora logical frame at which stale resolution must be checked.
        media_frame: u64,
    },
    /// A new tracker source-clock/sequence epoch begins and requires explicit re-anchoring.
    Reconnect {
        /// New explicit source-clock/Aurora-frame anchor.
        anchor: SimulatedHeadTrackerAnchor,
    },
}

/// Configuration for deterministic tracker delivery generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeadTrackerSimulationConfig {
    sample_rate: u32,
    tracker_rate_hz: u32,
    sample_count: u32,
    seed: u64,
    delivery_jitter_frames: u64,
}

impl HeadTrackerSimulationConfig {
    /// Creates a bounded simulation configuration.
    pub fn new(
        sample_rate: u32,
        tracker_rate_hz: u32,
        sample_count: u32,
        seed: u64,
        delivery_jitter_frames: u64,
    ) -> Result<Self, HeadTrackerSimulationError> {
        if !(8_000..=192_000).contains(&sample_rate)
            || tracker_rate_hz == 0
            || tracker_rate_hz > sample_rate
            || sample_rate % tracker_rate_hz != 0
            || sample_count < 16
            || delivery_jitter_frames > (i64::MAX as u64 / 2)
        {
            return Err(HeadTrackerSimulationError::InvalidConfiguration);
        }
        Ok(Self {
            sample_rate,
            tracker_rate_hz,
            sample_count,
            seed,
            delivery_jitter_frames,
        })
    }

    /// Aurora sample rate.
    pub const fn sample_rate(self) -> u32 {
        self.sample_rate
    }

    /// Simulated tracker sample rate.
    pub const fn tracker_rate_hz(self) -> u32 {
        self.tracker_rate_hz
    }

    /// Nominal samples generated before fault expansion such as duplicates.
    pub const fn sample_count(self) -> u32 {
        self.sample_count
    }

    /// Maximum absolute control-side delivery jitter.
    pub const fn delivery_jitter_frames(self) -> u64 {
        self.delivery_jitter_frames
    }

    /// Exact Aurora frame advance per nominal tracker sample.
    pub const fn frames_per_sample(self) -> u64 {
        (self.sample_rate / self.tracker_rate_hz) as u64
    }

    /// Integer source-clock period used by the simulator.
    pub const fn source_period_ns(self) -> u64 {
        NANOS_PER_SECOND / self.tracker_rate_hz as u64
    }
}

/// Configuration errors detected before event generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadTrackerSimulationError {
    /// Rates/count/jitter cannot produce the bounded deterministic contract.
    InvalidConfiguration,
}

fn orientation(global_index: u64) -> [f32; 4] {
    let phase = (global_index % 360) as f32 * std::f32::consts::PI / 180.0;
    let half = phase * 0.5;
    [half.cos(), 0.0, 0.0, half.sin()]
}

fn observed_media_frame(
    nominal_media_frame: u64,
    max_jitter_frames: u64,
    rng: &mut DeterministicRng,
) -> u64 {
    if max_jitter_frames == 0 {
        return nominal_media_frame;
    }
    let width = max_jitter_frames * 2 + 1;
    let offset = (rng.next_u64() % width) as i64 - max_jitter_frames as i64;
    nominal_media_frame.saturating_add_signed(offset)
}

fn sample_event(
    sequence: u64,
    source_timestamp_ns: u64,
    global_index: u64,
    config: HeadTrackerSimulationConfig,
    rng: &mut DeterministicRng,
    jitter_enabled: bool,
) -> HeadTrackerDeliveryEvent {
    let nominal_media_frame = global_index * config.frames_per_sample();
    let observed_media_frame = if jitter_enabled {
        observed_media_frame(nominal_media_frame, config.delivery_jitter_frames, rng)
    } else {
        nominal_media_frame
    };
    HeadTrackerDeliveryEvent::Sample(SimulatedHeadTrackerSample {
        sequence,
        source_timestamp_ns,
        orientation_wxyz: orientation(global_index),
        observed_media_frame,
    })
}

/// Generates one complete deterministic head-tracker delivery timeline.
///
/// The function may allocate because it is a simulation/control-plane utility. The
/// resulting events contain no device handles and make no claim about physical timing.
pub fn simulate_head_tracker_delivery(
    config: HeadTrackerSimulationConfig,
    profile: HeadTrackerFaultProfile,
) -> Vec<HeadTrackerDeliveryEvent> {
    let mut events = Vec::with_capacity(config.sample_count as usize + 8);
    let mut rng = DeterministicRng::new(config.seed);
    let mut global_index = 0_u64;
    let mut epoch_index = 0_u64;
    let fault_at = u64::from(config.sample_count / 2);
    let jitter_enabled = matches!(profile, HeadTrackerFaultProfile::Jitter);

    while global_index < u64::from(config.sample_count) {
        if global_index == fault_at {
            match profile {
                HeadTrackerFaultProfile::Reconnect => {
                    let media_frame = global_index * config.frames_per_sample();
                    events.push(HeadTrackerDeliveryEvent::Reconnect {
                        anchor: SimulatedHeadTrackerAnchor {
                            source_timestamp_ns: 0,
                            media_frame,
                        },
                    });
                    epoch_index = 0;
                }
                HeadTrackerFaultProfile::StaleHold => {
                    let dropped = 4_u64;
                    for offset in 0..dropped {
                        let sequence = epoch_index + offset + 1;
                        events.push(HeadTrackerDeliveryEvent::Drop {
                            sequence,
                            source_timestamp_ns: (epoch_index + offset)
                                * config.source_period_ns(),
                            nominal_media_frame: (global_index + offset)
                                * config.frames_per_sample(),
                        });
                    }
                    global_index += dropped;
                    epoch_index += dropped;
                    let media_frame = global_index * config.frames_per_sample();
                    events.push(HeadTrackerDeliveryEvent::StaleProbe { media_frame });
                    events.push(HeadTrackerDeliveryEvent::Reconnect {
                        anchor: SimulatedHeadTrackerAnchor {
                            source_timestamp_ns: 0,
                            media_frame,
                        },
                    });
                    epoch_index = 0;
                    continue;
                }
                _ => {}
            }
        }

        let sequence = epoch_index + 1;
        let source_timestamp_ns = epoch_index * config.source_period_ns();

        match profile {
            HeadTrackerFaultProfile::Dropout if global_index == fault_at => {
                events.push(HeadTrackerDeliveryEvent::Drop {
                    sequence,
                    source_timestamp_ns,
                    nominal_media_frame: global_index * config.frames_per_sample(),
                });
            }
            HeadTrackerFaultProfile::Duplicate if global_index == fault_at => {
                let event = sample_event(
                    sequence,
                    source_timestamp_ns,
                    global_index,
                    config,
                    &mut rng,
                    false,
                );
                events.push(event);
                events.push(event);
            }
            HeadTrackerFaultProfile::Reorder if global_index == fault_at => {
                let next_sequence = sequence + 1;
                let next_timestamp_ns = source_timestamp_ns + config.source_period_ns();
                events.push(sample_event(
                    next_sequence,
                    next_timestamp_ns,
                    global_index + 1,
                    config,
                    &mut rng,
                    false,
                ));
                events.push(sample_event(
                    sequence,
                    source_timestamp_ns,
                    global_index,
                    config,
                    &mut rng,
                    false,
                ));
                global_index += 2;
                epoch_index += 2;
                continue;
            }
            HeadTrackerFaultProfile::TimestampJump if global_index == fault_at => {
                events.push(sample_event(
                    sequence,
                    source_timestamp_ns + config.source_period_ns() * 10,
                    global_index,
                    config,
                    &mut rng,
                    false,
                ));
            }
            _ => events.push(sample_event(
                sequence,
                source_timestamp_ns,
                global_index,
                config,
                &mut rng,
                jitter_enabled,
            )),
        }

        global_index += 1;
        epoch_index += 1;
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> HeadTrackerSimulationConfig {
        HeadTrackerSimulationConfig::new(48_000, 100, 40, 7, 48).unwrap()
    }

    #[test]
    fn same_seed_and_profile_are_exactly_reproducible() {
        let first = simulate_head_tracker_delivery(config(), HeadTrackerFaultProfile::Jitter);
        let second = simulate_head_tracker_delivery(config(), HeadTrackerFaultProfile::Jitter);
        assert_eq!(first, second);
    }

    #[test]
    fn jitter_never_exceeds_configured_delivery_budget() {
        let config = config();
        let events = simulate_head_tracker_delivery(config, HeadTrackerFaultProfile::Jitter);
        for (index, event) in events.iter().enumerate() {
            let HeadTrackerDeliveryEvent::Sample(sample) = event else {
                panic!("jitter profile must contain only samples");
            };
            let nominal = index as u64 * config.frames_per_sample();
            assert!(sample.observed_media_frame.abs_diff(nominal) <= 48);
        }
    }

    #[test]
    fn duplicate_reorder_and_dropout_have_explicit_events() {
        let duplicate = simulate_head_tracker_delivery(config(), HeadTrackerFaultProfile::Duplicate);
        assert_eq!(duplicate.len(), config().sample_count() as usize + 1);
        assert!(duplicate.windows(2).any(|pair| pair[0] == pair[1]));

        let reorder = simulate_head_tracker_delivery(config(), HeadTrackerFaultProfile::Reorder);
        let has_reordered_pair = reorder.windows(2).any(|pair| {
            let (
                HeadTrackerDeliveryEvent::Sample(first),
                HeadTrackerDeliveryEvent::Sample(second),
            ) = (&pair[0], &pair[1])
            else {
                return false;
            };
            first.sequence == second.sequence + 1
        });
        assert!(has_reordered_pair);

        let dropout = simulate_head_tracker_delivery(config(), HeadTrackerFaultProfile::Dropout);
        assert_eq!(
            dropout
                .iter()
                .filter(|event| matches!(event, HeadTrackerDeliveryEvent::Drop { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn stale_hold_and_reconnect_are_explicit_epoch_boundaries() {
        let stale = simulate_head_tracker_delivery(config(), HeadTrackerFaultProfile::StaleHold);
        assert_eq!(
            stale
                .iter()
                .filter(|event| matches!(event, HeadTrackerDeliveryEvent::Drop { .. }))
                .count(),
            4
        );
        assert!(stale
            .iter()
            .any(|event| matches!(event, HeadTrackerDeliveryEvent::StaleProbe { .. })));
        assert!(stale
            .iter()
            .any(|event| matches!(event, HeadTrackerDeliveryEvent::Reconnect { .. })));

        let reconnect = simulate_head_tracker_delivery(config(), HeadTrackerFaultProfile::Reconnect);
        let reconnect_index = reconnect
            .iter()
            .position(|event| matches!(event, HeadTrackerDeliveryEvent::Reconnect { .. }))
            .unwrap();
        let HeadTrackerDeliveryEvent::Sample(sample) = reconnect[reconnect_index + 1] else {
            panic!("reconnect must be followed by a sample");
        };
        assert_eq!(sample.sequence, 1);
        assert_eq!(sample.source_timestamp_ns, 0);
    }
}
