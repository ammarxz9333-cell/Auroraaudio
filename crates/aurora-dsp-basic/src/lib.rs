//! Basic offline DSP processors.

use aurora_core::AudioBlock;
use thiserror::Error;

/// Errors returned by the basic DSP processors.
#[derive(Debug, Error, PartialEq)]
pub enum BasicDspError {
    /// Delay vector length must match the configured channel count.
    #[error("expected {expected} channel delays, got {actual}")]
    DelayChannelCount { expected: usize, actual: usize },
    /// Requested delay exceeds the configured maximum delay.
    #[error("delay {delay_samples} exceeds maximum {max_delay_samples}")]
    DelayExceedsMaximum {
        delay_samples: f32,
        max_delay_samples: f32,
    },
    /// Audio block channel count does not match the configured channel count.
    #[error("expected {expected} channels, got {actual}")]
    ChannelCount { expected: usize, actual: usize },
    /// A channel buffer is shorter than the requested frame count.
    #[error("channel {channel} has {actual} frames, needs {required}")]
    BufferFrames {
        channel: usize,
        required: usize,
        actual: usize,
    },
}

/// Per-channel delay processor using linear interpolation for fractional delay.
#[derive(Debug, Clone, PartialEq)]
pub struct DelayProcessor {
    channel_count: usize,
    max_delay_samples: f32,
    delays_samples: Vec<f32>,
    history: Vec<Vec<f32>>,
    history_len: usize,
    write_positions: Vec<usize>,
}

impl DelayProcessor {
    /// Creates a processor with a maximum delay in samples.
    pub fn new(channel_count: usize, max_delay_samples: f32) -> Self {
        let history_len = max_delay_samples.ceil() as usize + 2;
        Self {
            channel_count,
            max_delay_samples,
            delays_samples: vec![0.0; channel_count],
            history: vec![vec![0.0; history_len]; channel_count],
            history_len,
            write_positions: vec![0; channel_count],
        }
    }

    /// Sets independent per-channel delays in samples.
    pub fn set_delays(&mut self, delays_samples: Vec<f32>) -> Result<(), BasicDspError> {
        if delays_samples.len() != self.channel_count {
            return Err(BasicDspError::DelayChannelCount {
                expected: self.channel_count,
                actual: delays_samples.len(),
            });
        }
        for delay_samples in &delays_samples {
            if *delay_samples > self.max_delay_samples {
                return Err(BasicDspError::DelayExceedsMaximum {
                    delay_samples: *delay_samples,
                    max_delay_samples: self.max_delay_samples,
                });
            }
        }

        self.delays_samples = delays_samples;
        Ok(())
    }

    /// Sets independent per-channel delays in samples from a slice without allocation.
    pub fn set_delays_slice(&mut self, delays_samples: &[f32]) -> Result<(), BasicDspError> {
        if delays_samples.len() != self.channel_count {
            return Err(BasicDspError::DelayChannelCount {
                expected: self.channel_count,
                actual: delays_samples.len(),
            });
        }
        for &delay_samples in delays_samples {
            if delay_samples > self.max_delay_samples {
                return Err(BasicDspError::DelayExceedsMaximum {
                    delay_samples,
                    max_delay_samples: self.max_delay_samples,
                });
            }
        }

        self.delays_samples.copy_from_slice(delays_samples);
        Ok(())
    }

    /// Processes a planar audio block and returns delayed output.
    pub fn process_block(&mut self, input: &[Vec<f32>]) -> Result<Vec<Vec<f32>>, BasicDspError> {
        let frame_count = input.first().map_or(0, Vec::len);
        let mut output = vec![vec![0.0; frame_count]; self.channel_count];
        self.process_block_into(input, &mut output, frame_count)?;
        Ok(output)
    }

    /// Processes planar input into caller-owned output without allocation.
    pub fn process_block_into(
        &mut self,
        input: &[Vec<f32>],
        output: &mut [Vec<f32>],
        frame_count: usize,
    ) -> Result<(), BasicDspError> {
        if input.len() != self.channel_count {
            return Err(BasicDspError::ChannelCount {
                expected: self.channel_count,
                actual: input.len(),
            });
        }
        if output.len() != self.channel_count {
            return Err(BasicDspError::ChannelCount {
                expected: self.channel_count,
                actual: output.len(),
            });
        }
        for (channel, samples) in input.iter().enumerate() {
            if samples.len() < frame_count {
                return Err(BasicDspError::BufferFrames {
                    channel,
                    required: frame_count,
                    actual: samples.len(),
                });
            }
        }
        for (channel, samples) in output.iter().enumerate() {
            if samples.len() < frame_count {
                return Err(BasicDspError::BufferFrames {
                    channel,
                    required: frame_count,
                    actual: samples.len(),
                });
            }
        }

        for ((((input_channel, output_channel), delay), history), write_position) in input
            .iter()
            .zip(output.iter_mut())
            .zip(self.delays_samples.iter().copied())
            .zip(self.history.iter_mut())
            .zip(self.write_positions.iter_mut())
        {
            for (input_sample, output_sample) in input_channel
                .iter()
                .zip(output_channel.iter_mut())
                .take(frame_count)
            {
                let Some(history_slot) = history.get_mut(*write_position) else {
                    return Err(BasicDspError::BufferFrames {
                        channel: 0,
                        required: self.history_len,
                        actual: history.len(),
                    });
                };
                *history_slot = *input_sample;
                *output_sample = delayed_sample(history, *write_position, delay);
                *write_position += 1;
                if *write_position == self.history_len {
                    *write_position = 0;
                }
            }
        }
        Ok(())
    }

    /// Processes an [`AudioBlock`] and returns a delayed block with matching metadata.
    pub fn process_audio_block(&mut self, input: &AudioBlock) -> Result<AudioBlock, BasicDspError> {
        let channels = self.process_block(&input.channels)?;
        Ok(AudioBlock {
            channels,
            frame_count: input.frame_count,
            presentation_time_seconds: input.presentation_time_seconds,
            discontinuity: input.discontinuity,
        })
    }

    /// Clears all delay history.
    pub fn reset(&mut self) {
        for channel in &mut self.history {
            channel.fill(0.0);
        }
        self.write_positions.fill(0);
    }

    /// Returns maximum configured delay latency in frames.
    pub fn latency_frames(&self) -> usize {
        self.delays_samples
            .iter()
            .copied()
            .fold(0.0_f32, f32::max)
            .ceil() as usize
    }
}

fn delayed_sample(history: &[f32], write_position: usize, delay: f32) -> f32 {
    let lower_offset = delay.floor() as usize;
    let upper_offset = lower_offset + 1;
    let lower_index =
        (write_position + history.len() - lower_offset % history.len()) % history.len();
    let upper_index =
        (write_position + history.len() - upper_offset % history.len()) % history.len();
    let lower = history.get(lower_index).copied().unwrap_or(0.0);
    let upper = history.get(upper_index).copied().unwrap_or(0.0);
    let fraction = delay - lower_offset as f32;
    lower * (1.0 - fraction) + upper * fraction
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_delay_impulse_response() {
        let mut delay = DelayProcessor::new(1, 8.0);
        delay.set_delays(vec![2.0]).unwrap();

        let output = delay.process_block(&[vec![1.0, 0.0, 0.0, 0.0]]).unwrap();

        assert_eq!(output[0], vec![0.0, 0.0, 1.0, 0.0]);
    }

    #[test]
    fn fractional_delay_impulse_response_uses_linear_interpolation() {
        let mut delay = DelayProcessor::new(1, 8.0);
        delay.set_delays(vec![1.5]).unwrap();

        let output = delay.process_block(&[vec![1.0, 0.0, 0.0, 0.0]]).unwrap();

        assert_eq!(output[0], vec![0.0, 0.5, 0.5, 0.0]);
    }

    #[test]
    fn delay_reset_clears_history() {
        let mut delay = DelayProcessor::new(1, 8.0);
        delay.set_delays(vec![2.0]).unwrap();
        delay.process_block(&[vec![1.0, 0.0]]).unwrap();
        delay.reset();

        let output = delay.process_block(&[vec![0.0, 0.0, 0.0]]).unwrap();

        assert_eq!(output[0], vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn silence_remains_silence() {
        let mut delay = DelayProcessor::new(2, 8.0);
        delay.set_delays(vec![1.25, 3.5]).unwrap();

        let output = delay.process_block(&[vec![0.0; 8], vec![0.0; 8]]).unwrap();

        assert!(output.iter().flatten().all(|sample| *sample == 0.0));
    }

    #[test]
    fn no_nan_or_infinity() {
        let mut delay = DelayProcessor::new(1, 8.0);
        delay.set_delays(vec![2.25]).unwrap();

        let output = delay.process_block(&[vec![0.1; 16]]).unwrap();

        assert!(output.iter().flatten().all(|sample| sample.is_finite()));
    }

    #[test]
    fn deterministic_output() {
        let mut first = DelayProcessor::new(1, 8.0);
        let mut second = DelayProcessor::new(1, 8.0);
        first.set_delays(vec![2.5]).unwrap();
        second.set_delays(vec![2.5]).unwrap();

        let first_output = first.process_block(&[vec![0.0, 1.0, 0.5, -0.5]]).unwrap();
        let second_output = second.process_block(&[vec![0.0, 1.0, 0.5, -0.5]]).unwrap();

        assert_eq!(first_output, second_output);
    }

    #[test]
    fn caller_owned_processing_keeps_capacities_unchanged() {
        let mut delay = DelayProcessor::new(2, 16.0);
        delay.set_delays(vec![1.25, 3.5]).unwrap();
        let input = vec![vec![0.1; 64]; 2];
        let mut output = vec![vec![0.0; 64]; 2];
        let capacities = output.iter().map(Vec::capacity).collect::<Vec<_>>();
        for _ in 0..1_000 {
            delay.process_block_into(&input, &mut output, 64).unwrap();
        }
        assert_eq!(
            output.iter().map(Vec::capacity).collect::<Vec<_>>(),
            capacities
        );
    }
}
