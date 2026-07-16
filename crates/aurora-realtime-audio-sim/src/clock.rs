use aurora_realtime_audio_api::AudioDeviceDirection;

use crate::CallbackSizePolicy;

const TICKS_PER_SECOND: u64 = 1_000_000_000_000;

/// Small deterministic generator used only for simulation scheduling and noise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    /// Creates a generator. Zero is mapped to a fixed nonzero state.
    pub fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 {
                0x9E37_79B9_7F4A_7C15
            } else {
                seed
            },
        }
    }

    /// Returns the next deterministic integer.
    pub fn next_u64(&mut self) -> u64 {
        let mut value = self.state;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.state = value;
        value
    }

    /// Returns a deterministic value in `[-1, 1]`.
    pub fn next_bipolar(&mut self) -> f32 {
        let unit = (self.next_u64() >> 40) as f32 / ((1_u64 << 24) - 1) as f32;
        unit * 2.0 - 1.0
    }
}

/// Monotonic simulated time in picosecond ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct VirtualClock {
    ticks: u64,
}

impl VirtualClock {
    /// Creates a clock at zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns simulated seconds.
    pub fn seconds(&self) -> f64 {
        self.ticks as f64 / TICKS_PER_SECOND as f64
    }

    /// Returns raw deterministic ticks.
    pub fn ticks(&self) -> u64 {
        self.ticks
    }
}

/// One scheduled virtual callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallbackEvent {
    /// Clock domain that fired.
    pub direction: AudioDeviceDirection,
    /// Callback frame count.
    pub frames: usize,
    /// Scheduled monotonic timestamp.
    pub ticks: u64,
}

#[derive(Debug, Clone)]
struct Domain {
    direction: AudioDeviceDirection,
    sample_rate: u32,
    ppm: i32,
    jitter_frames: usize,
    policy: CallbackSizePolicy,
    nominal_ticks: u64,
    next_ticks: u64,
    callbacks: u64,
    burst_callbacks: usize,
}

impl Domain {
    fn period_ticks(&self, frames: usize) -> u64 {
        let scaled_rate = i128::from(self.sample_rate) * (1_000_000_i128 + i128::from(self.ppm));
        ((frames as i128 * i128::from(TICKS_PER_SECOND) * 1_000_000_i128) / scaled_rate.max(1))
            .clamp(1, i128::from(u64::MAX)) as u64
    }
}

/// Two-domain deterministic callback scheduler.
#[derive(Debug, Clone)]
pub struct VirtualScheduler {
    clock: VirtualClock,
    input: Domain,
    output: Domain,
    rng: DeterministicRng,
}

impl VirtualScheduler {
    /// Creates independent input and output callback domains.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        seed: u64,
        input_rate: u32,
        output_rate: u32,
        input_ppm: i32,
        output_ppm: i32,
        input_jitter_frames: usize,
        output_jitter_frames: usize,
        input_policy: CallbackSizePolicy,
        output_policy: CallbackSizePolicy,
    ) -> Self {
        let domain = |direction, sample_rate, ppm, jitter_frames, policy| Domain {
            direction,
            sample_rate,
            ppm,
            jitter_frames,
            policy,
            nominal_ticks: 0,
            next_ticks: 0,
            callbacks: 0,
            burst_callbacks: 0,
        };
        Self {
            clock: VirtualClock::new(),
            input: domain(
                AudioDeviceDirection::Input,
                input_rate,
                input_ppm,
                input_jitter_frames,
                input_policy,
            ),
            output: domain(
                AudioDeviceDirection::Output,
                output_rate,
                output_ppm,
                output_jitter_frames,
                output_policy,
            ),
            rng: DeterministicRng::new(seed),
        }
    }

    /// Returns the next callback and advances only its clock domain.
    pub fn next_callback(&mut self) -> CallbackEvent {
        let domain = if self.input.next_ticks <= self.output.next_ticks {
            &mut self.input
        } else {
            &mut self.output
        };
        let random = self.rng.next_u64();
        let frames = domain.policy.frames(domain.callbacks, random);
        let event_ticks = domain.next_ticks;
        let jitter = if domain.jitter_frames == 0 {
            0_i64
        } else {
            let width = domain.jitter_frames.saturating_mul(2).saturating_add(1);
            (random as usize % width) as i64
        } - domain.jitter_frames as i64;
        let base = domain.period_ticks(frames);
        let jitter_ticks =
            domain.period_ticks(jitter.unsigned_abs() as usize) as i64 * jitter.signum();
        domain.nominal_ticks = domain.nominal_ticks.saturating_add(base);
        if domain.burst_callbacks > 0 {
            domain.burst_callbacks -= 1;
            domain.next_ticks = event_ticks.saturating_add(1);
        } else {
            domain.next_ticks = domain
                .nominal_ticks
                .saturating_add_signed(jitter_ticks)
                .max(event_ticks.saturating_add(1));
        }
        domain.callbacks += 1;
        self.clock.ticks = event_ticks;
        CallbackEvent {
            direction: domain.direction,
            frames,
            ticks: event_ticks,
        }
    }

    /// Returns the current virtual clock.
    pub fn clock(&self) -> VirtualClock {
        self.clock
    }

    /// Changes one domain clock offset for fault injection.
    pub fn set_ppm(&mut self, direction: AudioDeviceDirection, ppm: i32) {
        match direction {
            AudioDeviceDirection::Input => self.input.ppm = ppm,
            AudioDeviceDirection::Output => self.output.ppm = ppm,
        }
    }

    /// Changes both callback-size policies for fault injection.
    pub fn set_callback_size(&mut self, frames: usize) {
        self.input.policy = CallbackSizePolicy::Fixed {
            frames: frames.max(1),
        };
        self.output.policy = CallbackSizePolicy::Fixed {
            frames: frames.max(1),
        };
    }

    /// Schedules bounded back-to-back callbacks on both clock domains.
    pub fn inject_burst(&mut self, callbacks: usize) {
        self.input.burst_callbacks = callbacks;
        self.output.burst_callbacks = callbacks;
    }

    /// Delays one clock domain without advancing the other.
    pub fn stall_direction(&mut self, direction: AudioDeviceDirection, milliseconds: u64) {
        let ticks = milliseconds.saturating_mul(TICKS_PER_SECOND / 1_000);
        let domain = match direction {
            AudioDeviceDirection::Input => &mut self.input,
            AudioDeviceDirection::Output => &mut self.output,
        };
        domain.nominal_ticks = domain.nominal_ticks.saturating_add(ticks);
        domain.next_ticks = domain.next_ticks.saturating_add(ticks);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scheduler(seed: u64) -> VirtualScheduler {
        VirtualScheduler::new(
            seed,
            48_000,
            48_000,
            100,
            -100,
            3,
            3,
            CallbackSizePolicy::RandomBounded {
                minimum: 64,
                maximum: 512,
            },
            CallbackSizePolicy::Alternating {
                first: 128,
                second: 256,
            },
        )
    }

    #[test]
    fn same_seed_produces_identical_independent_clock_events() {
        let mut first = scheduler(42);
        let mut second = scheduler(42);
        for _ in 0..10_000 {
            assert_eq!(first.next_callback(), second.next_callback());
        }
        assert!(first.clock().ticks() > 0);
    }

    #[test]
    fn different_seed_changes_bounded_jitter() {
        let mut first = scheduler(1);
        let mut second = scheduler(2);
        assert!((0..100).any(|_| first.next_callback() != second.next_callback()));
    }

    #[test]
    fn steady_state_scheduler_callbacks_allocate_zero_times() {
        let mut value = scheduler(7);
        value.next_callback();
        let allocations = crate::allocation_audit::count_allocations(|| {
            for _ in 0..100_000 {
                std::hint::black_box(value.next_callback());
            }
        });
        assert_eq!(allocations, 0);
    }
}
