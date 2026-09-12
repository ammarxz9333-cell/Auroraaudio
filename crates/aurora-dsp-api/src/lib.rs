//! DSP processing boundaries for Aurora.

use aurora_core::AudioBlock;
use thiserror::Error;

/// Errors returned by control/offline DSP engines.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DspError {
    /// The DSP engine has not received a valid configuration.
    #[error("DSP engine is not configured")]
    NotConfigured,
}

/// DSP processing abstraction used after rendering outside the specialized
/// allocation-free realtime delay path.
pub trait DspEngine {
    /// Configures the DSP engine for channel count, sample rate, and block size.
    fn configure(
        &mut self,
        channel_count: usize,
        sample_rate: u32,
        block_size: usize,
    ) -> Result<(), DspError>;

    /// Processes one mutable audio block in place.
    fn process(&mut self, audio_block: &mut AudioBlock) -> Result<(), DspError>;

    /// Clears internal processing state.
    fn reset(&mut self);

    /// Returns DSP latency in frames.
    fn latency_frames(&self) -> usize;
}

/// Allocation-free fault categories for callback-safe delay processing.
///
/// The error is intentionally fixed-size and carries no heap-owned diagnostic
/// payload. Detailed setup diagnostics belong to control-thread validation.
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum RealtimeDspFault {
    /// Per-channel delay count does not match the prepared component.
    #[error("realtime DSP delay shape is invalid")]
    DelayShape,
    /// One or more delay values are non-finite, negative, or out of range.
    #[error("realtime DSP delay value is invalid")]
    DelayValue,
    /// Input/output planar buffers do not match the prepared component.
    #[error("realtime DSP buffer shape is invalid")]
    BufferShape,
}

/// Prepared dynamic-delay component allowed to execute inside Aurora's audio
/// callback.
///
/// Implementations must be fully configured and allocate all storage before
/// crossing this boundary. Every callback-reachable method (`set_delays`,
/// `process_planar`, `reset`, and latency/capability accessors) must be bounded,
/// deterministic, and callback-safe: no heap allocation or deallocation, no
/// buffer growth, no blocking or contended locks, no sleeping/yielding/waiting,
/// no filesystem/network/device I/O, no process spawn or IPC/RPC, no logging or
/// string formatting, and no other operation that can block or have unbounded
/// execution time. Setup, diagnostics, telemetry formatting, and external I/O
/// belong on the control thread before activation.
pub trait RealtimeDelayProcessor: std::fmt::Debug + Send {
    /// Number of channels fixed at component preparation time.
    fn channel_count(&self) -> usize;

    /// Maximum supported dynamic delay in samples.
    fn max_delay_samples(&self) -> f32;

    /// Updates finite, non-negative per-channel delays using only bounded,
    /// callback-safe work and without allocation or blocking.
    fn set_delays(&mut self, delays_samples: &[f32]) -> Result<(), RealtimeDspFault>;

    /// Processes caller-owned planar buffers using only bounded, callback-safe
    /// work and without allocation, blocking, logging, or external I/O.
    fn process_planar(
        &mut self,
        input: &[Vec<f32>],
        output: &mut [Vec<f32>],
        frame_count: usize,
    ) -> Result<(), RealtimeDspFault>;

    /// Clears processing history using bounded callback-safe work only.
    fn reset(&mut self);

    /// Current delay latency in frames; this accessor must be callback-safe.
    fn latency_frames(&self) -> usize;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct FixtureDelay {
        delay: f32,
    }

    impl RealtimeDelayProcessor for FixtureDelay {
        fn channel_count(&self) -> usize {
            1
        }

        fn max_delay_samples(&self) -> f32 {
            8.0
        }

        fn set_delays(&mut self, delays_samples: &[f32]) -> Result<(), RealtimeDspFault> {
            if delays_samples.len() != 1 {
                return Err(RealtimeDspFault::DelayShape);
            }
            let value = delays_samples[0];
            if !value.is_finite() || !(0.0..=8.0).contains(&value) {
                return Err(RealtimeDspFault::DelayValue);
            }
            self.delay = value;
            Ok(())
        }

        fn process_planar(
            &mut self,
            input: &[Vec<f32>],
            output: &mut [Vec<f32>],
            frame_count: usize,
        ) -> Result<(), RealtimeDspFault> {
            if input.len() != 1
                || output.len() != 1
                || input[0].len() < frame_count
                || output[0].len() < frame_count
            {
                return Err(RealtimeDspFault::BufferShape);
            }
            output[0][..frame_count].copy_from_slice(&input[0][..frame_count]);
            Ok(())
        }

        fn reset(&mut self) {
            self.delay = 0.0;
        }

        fn latency_frames(&self) -> usize {
            self.delay.ceil() as usize
        }
    }

    #[test]
    fn realtime_contract_uses_fixed_fault_categories() {
        let mut delay = FixtureDelay { delay: 0.0 };
        assert_eq!(delay.set_delays(&[]), Err(RealtimeDspFault::DelayShape));
        assert_eq!(
            delay.set_delays(&[f32::NAN]),
            Err(RealtimeDspFault::DelayValue)
        );
        assert_eq!(delay.set_delays(&[2.5]), Ok(()));
        assert_eq!(delay.latency_frames(), 3);
    }

    #[test]
    fn realtime_contract_processes_caller_owned_buffers() {
        let mut delay = FixtureDelay { delay: 0.0 };
        let input = vec![vec![0.1, 0.2, 0.3, 0.4]];
        let mut output = vec![vec![0.0; 4]];
        delay.process_planar(&input, &mut output, 4).unwrap();
        assert_eq!(output, input);
    }
}
