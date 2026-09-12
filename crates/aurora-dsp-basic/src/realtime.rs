use aurora_dsp_api::{RealtimeDelayProcessor, RealtimeDspFault};

use super::{BasicDspError, DelayProcessor};

impl RealtimeDelayProcessor for DelayProcessor {
    fn channel_count(&self) -> usize {
        self.channel_count
    }

    fn max_delay_samples(&self) -> f32 {
        self.max_delay_samples
    }

    fn set_delays(&mut self, delays_samples: &[f32]) -> Result<(), RealtimeDspFault> {
        self.set_delays_slice(delays_samples)
            .map_err(map_realtime_fault)
    }

    fn process_planar(
        &mut self,
        input: &[Vec<f32>],
        output: &mut [Vec<f32>],
        frame_count: usize,
    ) -> Result<(), RealtimeDspFault> {
        self.process_block_into(input, output, frame_count)
            .map_err(map_realtime_fault)
    }

    fn reset(&mut self) {
        DelayProcessor::reset(self);
    }

    fn latency_frames(&self) -> usize {
        DelayProcessor::latency_frames(self)
    }
}

fn map_realtime_fault(error: BasicDspError) -> RealtimeDspFault {
    match error {
        BasicDspError::DelayChannelCount { .. } => RealtimeDspFault::DelayShape,
        BasicDspError::DelayExceedsMaximum { .. }
        | BasicDspError::DelayNotFinite { .. }
        | BasicDspError::DelayNegative { .. } => RealtimeDspFault::DelayValue,
        BasicDspError::ChannelCount { .. } | BasicDspError::BufferFrames { .. } => {
            RealtimeDspFault::BufferShape
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurora_test_alloc::{count_allocations, CountingAllocator};

    #[global_allocator]
    static ALLOCATOR: CountingAllocator = CountingAllocator;

    #[test]
    fn trait_path_matches_delay_processor_behavior() {
        let mut delay = DelayProcessor::new(1, 8.0);
        RealtimeDelayProcessor::set_delays(&mut delay, &[1.5]).unwrap();

        let input = vec![vec![1.0, 0.0, 0.0, 0.0]];
        let mut output = vec![vec![0.0; 4]];
        RealtimeDelayProcessor::process_planar(&mut delay, &input, &mut output, 4).unwrap();

        assert_eq!(output[0], vec![0.0, 0.5, 0.5, 0.0]);
        assert_eq!(RealtimeDelayProcessor::channel_count(&delay), 1);
        assert_eq!(RealtimeDelayProcessor::max_delay_samples(&delay), 8.0);
        assert_eq!(RealtimeDelayProcessor::latency_frames(&delay), 2);
    }

    #[test]
    fn trait_path_is_allocation_free_and_capacity_invariant() {
        let mut delay = DelayProcessor::new(2, 16.0);
        let input = vec![vec![0.1; 64], vec![-0.2; 64]];
        let mut output = vec![vec![0.0; 64], vec![0.0; 64]];
        let first_delays = [1.25, 3.5];
        let second_delays = [2.0, 4.25];

        RealtimeDelayProcessor::set_delays(&mut delay, &first_delays).unwrap();
        RealtimeDelayProcessor::process_planar(&mut delay, &input, &mut output, 64).unwrap();
        let _ = count_allocations(|| {});

        let input_capacities = input.iter().map(Vec::capacity).collect::<Vec<_>>();
        let output_capacities = output.iter().map(Vec::capacity).collect::<Vec<_>>();
        let delay_capacity = delay.delays_samples.capacity();
        let history_outer_capacity = delay.history.capacity();
        let history_capacities = delay.history.iter().map(Vec::capacity).collect::<Vec<_>>();
        let write_positions_capacity = delay.write_positions.capacity();

        let mut failed = false;
        let allocations = count_allocations(|| {
            for iteration in 0..1_000 {
                let delays = if iteration % 2 == 0 {
                    &first_delays[..]
                } else {
                    &second_delays[..]
                };
                if RealtimeDelayProcessor::set_delays(&mut delay, delays).is_err()
                    || RealtimeDelayProcessor::process_planar(&mut delay, &input, &mut output, 64)
                        .is_err()
                {
                    failed = true;
                    break;
                }
            }
        });

        assert!(!failed);
        assert_eq!(allocations, 0, "trait-path processing allocated");
        assert_eq!(
            input.iter().map(Vec::capacity).collect::<Vec<_>>(),
            input_capacities
        );
        assert_eq!(
            output.iter().map(Vec::capacity).collect::<Vec<_>>(),
            output_capacities
        );
        assert_eq!(delay.delays_samples.capacity(), delay_capacity);
        assert_eq!(delay.history.capacity(), history_outer_capacity);
        assert_eq!(
            delay.history.iter().map(Vec::capacity).collect::<Vec<_>>(),
            history_capacities
        );
        assert_eq!(delay.write_positions.capacity(), write_positions_capacity);
    }

    #[test]
    fn basic_errors_collapse_to_fixed_realtime_faults() {
        let mut delay = DelayProcessor::new(2, 4.0);
        assert_eq!(
            RealtimeDelayProcessor::set_delays(&mut delay, &[1.0]),
            Err(RealtimeDspFault::DelayShape)
        );
        assert_eq!(
            RealtimeDelayProcessor::set_delays(&mut delay, &[1.0, 9.0]),
            Err(RealtimeDspFault::DelayValue)
        );

        let input = vec![vec![0.0; 4]];
        let mut output = vec![vec![0.0; 4], vec![0.0; 4]];
        assert_eq!(
            RealtimeDelayProcessor::process_planar(&mut delay, &input, &mut output, 4),
            Err(RealtimeDspFault::BufferShape)
        );
    }
}
