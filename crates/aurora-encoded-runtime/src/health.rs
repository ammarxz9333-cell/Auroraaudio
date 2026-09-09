//! Observational telemetry only: no transport, decoder or output policy changes.
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, Thread};
use std::time::{Duration, Instant};

use crossbeam_queue::ArrayQueue;

use crate::PlaybackBatch;
use aurora_decoder_engine::JocDecoderStatus;
use aurora_direct_earc_decoder::DirectEarcTransportTelemetry;

pub const DEFAULT_HEALTH_INTERVAL: Duration = Duration::from_secs(5);

/// Cumulative successful ingest/decode counters for one process session.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeCounters {
    pub carrier_bursts: u64,
    pub format_changes: u64,
    pub decoded_frames: u64,
    pub decoded_pcm_frames: u64,
    pub transport_discontinuities: u64,
    pub capture_xruns: u64,
    pub capture_recoveries: u64,
    pub capture_discontinuities: u64,
}

impl RuntimeCounters {
    /// Count decoded output, independently of whether the sink later accepts it.
    /// Native capture discontinuities are recorded separately by the caller.
    pub fn record_batch(&mut self, batch: &PlaybackBatch) {
        self.carrier_bursts = self.carrier_bursts.saturating_add(batch.bursts as u64);
        self.format_changes = self
            .format_changes
            .saturating_add(batch.format_changes as u64);
        self.transport_discontinuities = self
            .transport_discontinuities
            .saturating_add(u64::from(batch.discontinuity));
        self.decoded_frames = self
            .decoded_frames
            .saturating_add(batch.frames.len() as u64);
        for frame in &batch.frames {
            self.decoded_pcm_frames = self
                .decoded_pcm_frames
                .saturating_add(frame.frame_count as u64);
        }
    }

    /// Builds a transport-only snapshot. This is retained for callers that do
    /// not own a decoder status handle; it never guesses JOC from IEC61937.
    pub fn snapshot(
        self,
        parser: DirectEarcTransportTelemetry,
        output: Option<OutputHealth>,
    ) -> RuntimeHealthSnapshot {
        RuntimeHealthSnapshot {
            counters: self,
            parser,
            joc: JocHealth::default(),
            output,
        }
    }

    /// Builds a coherent transport + decoder snapshot. JOC status comes only
    /// from the decoder engine and is never derived from IEC61937 data type 0x15.
    pub fn snapshot_with_joc(
        self,
        parser: DirectEarcTransportTelemetry,
        output: Option<OutputHealth>,
        joc: &JocDecoderStatus,
    ) -> RuntimeHealthSnapshot {
        RuntimeHealthSnapshot {
            counters: self,
            parser,
            joc: JocHealth::from(joc),
            output,
        }
    }
}

/// Native output totals, including all handles retired on transport resets.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct OutputHealth {
    pub xruns: u64,
    pub recoveries: u64,
}

impl OutputHealth {
    pub fn plus(self, current: Self) -> Self {
        Self {
            xruns: self.xruns.saturating_add(current.xruns),
            recoveries: self.recoveries.saturating_add(current.recoveries),
        }
    }
}

/// Allocation-free subset of JOC decoder status suitable for the one-slot
/// periodic health mailbox. Detailed layout/fallback strings remain available
/// through `AuroraDecoderEngine::joc_status()` outside the realtime snapshot.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct JocHealth {
    pub codec_classified_joc: bool,
    pub speaker_render_active: bool,
    pub channel_count: Option<usize>,
    pub latency_samples: Option<usize>,
    pub object_count: Option<u16>,
    pub complexity_index: Option<u8>,
    pub fallback_present: bool,
}

impl From<&JocDecoderStatus> for JocHealth {
    fn from(status: &JocDecoderStatus) -> Self {
        Self {
            codec_classified_joc: status.codec_classified_joc,
            speaker_render_active: status.speaker_render_active,
            channel_count: status.channel_count,
            latency_samples: status.latency_samples,
            object_count: status.object_count,
            complexity_index: status.complexity_index,
            fallback_present: status.fallback_reason.is_some(),
        }
    }
}

/// Fixed-size, coherent observation at an ingest boundary. `None` means native
/// output telemetry is unavailable (for example stdout), not zero XRUNs.
/// Parser discarded bytes include carrier padding and NEVER imply eARC unlock.
/// Parser pending bytes are a gauge; JOC is a separate decoder observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeHealthSnapshot {
    pub counters: RuntimeCounters,
    pub parser: DirectEarcTransportTelemetry,
    pub joc: JocHealth,
    pub output: Option<OutputHealth>,
}

/// Monotonic interval gate: zero disables; delayed wakes never cause catch-up bursts.
pub struct HealthInterval {
    interval: Duration,
    last: Duration,
}

impl HealthInterval {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            last: Duration::ZERO,
        }
    }

    pub fn due(&mut self, elapsed: Duration) -> bool {
        if self.interval.is_zero() || elapsed.saturating_sub(self.last) < self.interval {
            return false;
        }
        self.last = elapsed;
        true
    }

    fn remaining(&self, elapsed: Duration) -> Duration {
        self.interval
            .saturating_sub(elapsed.saturating_sub(self.last))
    }
}

struct Shared {
    latest: ArrayQueue<(RuntimeHealthSnapshot, Instant)>,
    stopped: AtomicBool,
}

/// One-slot, preallocated lock-free mailbox. Publication never formats, logs,
/// allocates or waits for the reporter. Slow reporters coalesce observations.
/// The worker emits the last snapshot even when capture/read/write is stalled;
/// the supplied age reveals stale observations, not physical link status.
pub struct HealthReporter {
    worker: Option<(Arc<Shared>, Thread)>,
}

impl HealthReporter {
    pub fn start(
        interval: Duration,
        initial: RuntimeHealthSnapshot,
        mut emit: impl FnMut(RuntimeHealthSnapshot, Duration) + Send + 'static,
    ) -> io::Result<Self> {
        if interval.is_zero() {
            return Ok(Self { worker: None });
        }
        let shared = Arc::new(Shared {
            latest: ArrayQueue::new(1),
            stopped: AtomicBool::new(false),
        });
        let reader = Arc::clone(&shared);
        let handle = thread::Builder::new()
            .name("aurora-health".into())
            .spawn(move || {
                let started = Instant::now();
                let mut latest = (initial, started);
                let mut gate = HealthInterval::new(interval);
                loop {
                    thread::park_timeout(gate.remaining(started.elapsed()));
                    if reader.stopped.load(Ordering::Acquire) {
                        break;
                    }
                    if gate.due(started.elapsed()) {
                        if let Some(snapshot) = reader.latest.pop() {
                            latest = snapshot;
                        }
                        emit(latest.0, latest.1.elapsed());
                    }
                }
            })?;
        // Do not join a worker potentially blocked in an external diagnostic sink.
        Ok(Self {
            worker: Some((shared, handle.thread().clone())),
        })
    }

    pub fn publish(&self, snapshot: RuntimeHealthSnapshot) {
        if let Some((shared, _)) = &self.worker {
            shared.latest.force_push((snapshot, Instant::now()));
        }
    }
}

impl Drop for HealthReporter {
    fn drop(&mut self) {
        if let Some((shared, worker)) = &self.worker {
            shared.stopped.store(true, Ordering::Release);
            worker.unpark();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SpeakerOutputFrame;

    fn parser_snapshot() -> DirectEarcTransportTelemetry {
        DirectEarcTransportTelemetry {
            pending_carrier_bytes: 7,
            discarded_bytes: 800,
            malformed_headers: 2,
            iec61937_locked: false,
            observation_epoch: 3,
            total_bursts: 11,
            bursts_since_lock: 0,
            total_format_changes: 1,
            relocks: 1,
            last_valid_burst_age_ms: None,
        }
    }

    fn snapshot() -> RuntimeHealthSnapshot {
        RuntimeCounters::default().snapshot(parser_snapshot(), None)
    }

    #[test]
    fn aggregation_preserves_parser_and_separates_padding_from_discontinuity() {
        let mut counters = RuntimeCounters {
            capture_xruns: 3,
            capture_recoveries: 4,
            capture_discontinuities: 1,
            ..RuntimeCounters::default()
        };
        counters.record_batch(&PlaybackBatch {
            bursts: 2,
            format_changes: 1,
            frames: vec![SpeakerOutputFrame {
                interleaved_f32: vec![],
                frame_count: 40,
                presentation_time_seconds: 0.0,
                discontinuity: false,
            }],
            ..PlaybackBatch::default()
        });
        let health = counters.snapshot(
            parser_snapshot(),
            Some(OutputHealth {
                xruns: 5,
                recoveries: 6,
            }),
        );
        assert_eq!(health.counters.carrier_bursts, 2);
        assert_eq!(health.counters.format_changes, 1);
        assert_eq!(health.counters.decoded_frames, 1);
        assert_eq!(health.counters.decoded_pcm_frames, 40);
        assert_eq!(health.counters.transport_discontinuities, 0);
        assert_eq!(health.counters.capture_xruns, 3);
        assert_eq!(health.counters.capture_recoveries, 4);
        assert_eq!(health.counters.capture_discontinuities, 1);
        assert_eq!(health.parser, parser_snapshot());
        assert_eq!(health.joc, JocHealth::default());
        assert_eq!(health.output.unwrap().xruns, 5);
        counters.record_batch(&PlaybackBatch {
            discontinuity: true,
            ..PlaybackBatch::default()
        });
        assert_eq!(counters.transport_discontinuities, 1);
    }

    #[test]
    fn transport_only_snapshot_never_infers_joc() {
        let health = RuntimeCounters::default().snapshot(parser_snapshot(), None);
        assert_eq!(health.joc, JocHealth::default());
    }

    #[test]
    fn joc_status_is_reduced_to_allocation_free_health_fields() {
        let detailed = JocDecoderStatus {
            codec_classified_joc: true,
            speaker_render_active: true,
            layout_name: Some("7.1.4".to_owned()),
            channel_count: Some(12),
            latency_samples: Some(256),
            object_count: Some(15),
            complexity_index: Some(8),
            fallback_reason: None,
        };
        let health = RuntimeCounters::default().snapshot_with_joc(
            parser_snapshot(),
            None,
            &detailed,
        );
        assert!(health.joc.codec_classified_joc);
        assert!(health.joc.speaker_render_active);
        assert_eq!(health.joc.channel_count, Some(12));
        assert_eq!(health.joc.object_count, Some(15));
        assert!(!health.joc.fallback_present);
    }

    #[test]
    fn counters_saturate_and_output_totals_survive_reopen() {
        let mut counters = RuntimeCounters {
            carrier_bursts: u64::MAX,
            ..RuntimeCounters::default()
        };
        counters.record_batch(&PlaybackBatch {
            bursts: 1,
            ..PlaybackBatch::default()
        });
        assert_eq!(counters.carrier_bursts, u64::MAX);
        let retired = OutputHealth {
            xruns: 3,
            recoveries: 4,
        };
        assert_eq!(retired.plus(OutputHealth::default()), retired);
        assert_eq!(
            retired.plus(OutputHealth {
                xruns: u64::MAX,
                recoveries: 2
            }),
            OutputHealth {
                xruns: u64::MAX,
                recoveries: 6
            }
        );
    }

    #[test]
    fn interval_boundaries_disable_and_no_catchup() {
        let mut gate = HealthInterval::new(DEFAULT_HEALTH_INTERVAL);
        assert!(!gate.due(Duration::ZERO));
        assert!(!gate.due(Duration::from_millis(4999)));
        assert!(gate.due(Duration::from_secs(5)));
        assert!(!gate.due(Duration::from_secs(5)));
        assert!(gate.due(Duration::from_secs(23)));
        assert!(!gate.due(Duration::from_secs(23)));
        assert!(!gate.due(Duration::from_secs(27)));
        assert!(gate.due(Duration::from_secs(28)));
        assert!(!HealthInterval::new(Duration::ZERO).due(Duration::MAX));
        assert!(HealthInterval::new(Duration::from_millis(10)).due(Duration::from_millis(10)));
    }

    #[test]
    fn disabled_reporter_has_no_worker() {
        let reporter =
            HealthReporter::start(Duration::ZERO, snapshot(), |_, _| panic!("disabled")).unwrap();
        assert!(reporter.worker.is_none());
        reporter.publish(snapshot());
    }

    #[test]
    fn reporter_emits_latest_snapshot_and_marks_idle_observations_with_age() {
        let (tx, rx) = std::sync::mpsc::channel();
        let reporter =
            HealthReporter::start(Duration::from_millis(10), snapshot(), move |s, age| {
                let _ = tx.send((s, age));
            })
            .unwrap();
        let mut updated = snapshot();
        updated.counters.carrier_bursts = 42;
        reporter.publish(updated);
        let deadline = Instant::now() + Duration::from_secs(5);
        let first = loop {
            let event = rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap();
            if event.0 == updated {
                break event;
            }
        };
        let next = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(next.0, updated);
        assert!(next.1 >= first.1);
        drop(reporter);
    }

    #[test]
    fn blocked_emitter_cannot_block_publication_or_drop() {
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let reporter = HealthReporter::start(Duration::from_millis(1), snapshot(), move |_, _| {
            let _ = entered_tx.send(());
            let _ = release_rx.recv();
        })
        .unwrap();
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        for value in 0..1000 {
            let mut health = snapshot();
            health.counters.carrier_bursts = value;
            reporter.publish(health);
        }
        let shared = &reporter.worker.as_ref().unwrap().0;
        assert_eq!(shared.latest.len(), 1);
        assert_eq!(shared.latest.pop().unwrap().0.counters.carrier_bursts, 999);
        drop(reporter);
        release_tx.send(()).unwrap();
    }
}
