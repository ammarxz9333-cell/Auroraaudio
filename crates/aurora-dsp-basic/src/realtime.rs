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
