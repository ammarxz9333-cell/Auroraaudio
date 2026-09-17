//! Prepared stereo FIR rendering. SOFA parsing, spatial lookup and filter preparation
//! belong on the control thread. The callback only convolves fixed storage.
//! This does not imply a personalized HRTF or perceptual discrimination guarantee.

/// Explicit supported input interpretation. HOA uses ACN ordering and SN3D only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    /// Independent mono objects in stable channel order.
    Objects(usize),
    /// Full-sphere Ambisonics, orders one through three, ACN/SN3D.
    AmbisonicsAcnSn3d(u8),
}

impl Input {
    /// Validates the input and returns the exact channel count.
    pub fn channels(self) -> Result<usize, Error> {
        match self {
            Self::Objects(n) if (1..=16).contains(&n) => Ok(n),
            Self::AmbisonicsAcnSn3d(n) if (1..=3).contains(&n) => Ok((usize::from(n) + 1).pow(2)),
            _ => Err(Error::Contract),
        }
    }
}

/// Numeric, allocation-free rejection categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Unsupported input, rate, dimensions or resource bound.
    Contract,
    /// Filter/input has nonfinite or out-of-bound values.
    Numeric,
    /// Filter generation is stale or incompatible with the active configuration.
    Generation,
    /// Finish the active transition before committing another filter.
    TransitionBusy,
    /// Input or output shape differs from the configured block contract.
    Buffer,
}

/// Immutable filter bank, validated before it can reach the callback.
#[derive(Debug, Clone)]
pub struct Filters {
    input: Input,
    sample_rate: u32,
    generation: u64,
    taps: usize,
    // input-major, ear-minor, tap-minor.
    coefficients: Vec<f32>,
}

impl Filters {
    /// Validates a bank of `[input][left/right][tap]` impulse responses.
    /// Each response has L1 norm at most 16, bounding output for normalized PCM.
    /// Calls allocate only on the control thread; rejected banks cannot mutate a renderer.
    pub fn prepare(
        input: Input,
        sample_rate: u32,
        generation: u64,
        taps: usize,
        coefficients: Vec<f32>,
    ) -> Result<Self, Error> {
        let channels = input.channels()?;
        if !(8_000..=192_000).contains(&sample_rate)
            || generation == 0
            || !(1..=2048).contains(&taps)
            || coefficients.len() != channels * 2 * taps
        {
            return Err(Error::Contract);
        }
        for response in coefficients.chunks_exact(taps) {
            let norm: f64 = response.iter().map(|x| f64::from(x.abs())).sum();
            if response.iter().any(|x| !x.is_finite()) || norm > 16.0 {
                return Err(Error::Numeric);
            }
        }
        Ok(Self {
            input,
            sample_rate,
            generation,
            taps,
            coefficients,
        })
    }
}

/// Fixed-storage streaming convolution with sample-wise linear filter crossfades.
/// Input history is shared by both banks, so transitions retain convolution tails.
/// No allocation, deallocation, locks, file/process access or formatting in processing.
#[derive(Debug)]
pub struct PreparedBinaural {
    input: Input,
    sample_rate: u32,
    generation: u64,
    channels: usize,
    taps: usize,
    max_frames: usize,
    active: Vec<f32>,
    next: Vec<f32>,
    history: Vec<f32>,
    cursor: usize,
    transition_frames: usize,
    transition_position: usize,
}

impl PreparedBinaural {
    /// Allocates all storage during preparation.
    pub fn new(filters: Filters, max_frames: usize) -> Result<Self, Error> {
        if !(1..=1024).contains(&max_frames) {
            return Err(Error::Contract);
        }
        let channels = filters.input.channels()?;
        Ok(Self {
            input: filters.input,
            sample_rate: filters.sample_rate,
            generation: filters.generation,
            channels,
            taps: filters.taps,
            max_frames,
            next: vec![0.0; filters.coefficients.len()],
            active: filters.coefficients,
            history: vec![0.0; channels * filters.taps],
            cursor: 0,
            transition_frames: 0,
            transition_position: 0,
        })
    }

    /// Copies a fully validated candidate into fixed inactive storage.
    /// Call at an exclusive block boundary. Failure preserves the active state.
    /// Transitions must last between one sample and one second; no silent snapping.
    pub fn commit(&mut self, filters: &Filters, transition_frames: usize) -> Result<(), Error> {
        if filters.input != self.input
            || filters.sample_rate != self.sample_rate
            || filters.taps != self.taps
            || filters.generation <= self.generation
        {
            return Err(Error::Generation);
        }
        if transition_frames == 0 || transition_frames > self.sample_rate as usize {
            return Err(Error::Contract);
        }
        if self.transition_frames != 0 {
            return Err(Error::TransitionBusy);
        }
        self.next.copy_from_slice(&filters.coefficients);
        self.generation = filters.generation;
        self.transition_frames = transition_frames;
        self.transition_position = 0;
        Ok(())
    }

    /// Processes interleaved normalized PCM into interleaved stereo.
    /// Rejects the entire invalid block before history changes and silences output.
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) -> Result<(), Error> {
        if input.is_empty()
            || input.len() % self.channels != 0
            || input.len() / self.channels > self.max_frames
            || output.len() != input.len() / self.channels * 2
        {
            output.fill(0.0);
            return Err(Error::Buffer);
        }
        if input.iter().any(|x| !x.is_finite() || x.abs() > 1.0) {
            output.fill(0.0);
            return Err(Error::Numeric);
        }
        for (frame, ears) in input
            .chunks_exact(self.channels)
            .zip(output.chunks_exact_mut(2))
        {
            for (channel, sample) in frame.iter().enumerate() {
                self.history[channel * self.taps + self.cursor] = *sample;
            }
            let alpha = if self.transition_frames != 0 {
                (self.transition_position + 1) as f64 / self.transition_frames as f64
            } else {
                0.0
            };
            for (ear, sample) in ears.iter_mut().enumerate() {
                let mut sum = 0.0_f64;
                for channel in 0..self.channels {
                    let offset = (channel * 2 + ear) * self.taps;
                    for tap in 0..self.taps {
                        let index = (self.cursor + self.taps - tap) % self.taps;
                        let h = f64::from(self.active[offset + tap]);
                        let coefficient = if self.transition_frames != 0 {
                            h + (f64::from(self.next[offset + tap]) - h) * alpha
                        } else {
                            h
                        };
                        sum += f64::from(self.history[channel * self.taps + index]) * coefficient;
                    }
                }
                *sample = sum as f32;
            }
            self.cursor = (self.cursor + 1) % self.taps;
            if self.transition_frames != 0 {
                self.transition_position += 1;
                if self.transition_position == self.transition_frames {
                    std::mem::swap(&mut self.active, &mut self.next);
                    self.transition_frames = 0;
                    self.transition_position = 0;
                }
            }
        }
        Ok(())
    }

    /// Clears history at a discontinuity and completes any already accepted transition.
    /// No storage is released or resized.
    pub fn discontinuity(&mut self) {
        self.history.fill(0.0);
        self.cursor = 0;
        if self.transition_frames != 0 {
            std::mem::swap(&mut self.active, &mut self.next);
        }
        self.transition_frames = 0;
        self.transition_position = 0;
    }

    /// Active or accepted target generation.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Causal direct FIR adds no buffering latency. Filter onset/group delay is separate.
    pub fn buffering_latency_frames(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filters(generation: u64, taps: Vec<f32>) -> Filters {
        Filters::prepare(Input::Objects(1), 48_000, generation, taps.len() / 2, taps).unwrap()
    }

    #[test]
    fn impulse_recovers_both_responses_and_tail_across_blocks() {
        let bank = filters(1, vec![0.5, 0.25, -0.125, 0.0, 0.0, 0.0, 0.75, 0.125]);
        let mut renderer = PreparedBinaural::new(bank, 2).unwrap();
        let mut first = [0.0; 4];
        let mut second = [0.0; 4];
        renderer.process(&[1.0, 0.0], &mut first).unwrap();
        renderer.process(&[0.0, 0.0], &mut second).unwrap();
        assert_eq!(first, [0.5, 0.0, 0.25, 0.0]);
        assert_eq!(second, [-0.125, 0.75, 0.0, 0.125]);
        renderer.process(&[0.0, 0.0], &mut second).unwrap();
        assert_eq!(second, [0.0; 4]);
    }

    #[test]
    fn hoa_channels_remain_independent_and_order_is_explicit() {
        for order in 1..=3 {
            let input = Input::AmbisonicsAcnSn3d(order);
            let n = input.channels().unwrap();
            let coefficients = (0..n)
                .flat_map(|i| [i as f32 / n as f32, -(i as f32) / n as f32])
                .collect();
            let bank = Filters::prepare(input, 48_000, 1, 1, coefficients).unwrap();
            let mut renderer = PreparedBinaural::new(bank, 1).unwrap();
            for i in 0..n {
                let mut frame = vec![0.0; n];
                frame[i] = 1.0;
                let mut output = [0.0; 2];
                renderer.process(&frame, &mut output).unwrap();
                assert_eq!(output, [i as f32 / n as f32, -(i as f32) / n as f32]);
            }
        }
        assert_eq!(Input::AmbisonicsAcnSn3d(4).channels(), Err(Error::Contract));
    }

    #[test]
    fn crossfade_is_sample_continuous_and_independent_of_block_partition() {
        let make = || {
            let mut renderer = PreparedBinaural::new(filters(1, vec![1.0, 0.0]), 8).unwrap();
            renderer.commit(&filters(2, vec![0.0, 1.0]), 8).unwrap();
            renderer
        };
        let mut whole = make();
        let mut split = make();
        let mut expected = [0.0; 16];
        let mut actual = [0.0; 16];
        whole.process(&[1.0; 8], &mut expected).unwrap();
        split.process(&[1.0; 3], &mut actual[..6]).unwrap();
        split.process(&[1.0; 5], &mut actual[6..]).unwrap();
        assert_eq!(actual, expected);
        for (i, frame) in actual.chunks_exact(2).enumerate() {
            assert_eq!(frame, &[1.0 - (i + 1) as f32 / 8.0, (i + 1) as f32 / 8.0]);
        }
    }

    #[test]
    fn invalid_candidates_and_blocks_preserve_active_state() {
        let mut renderer = PreparedBinaural::new(filters(1, vec![0.5, 0.25]), 4).unwrap();
        assert_eq!(
            renderer.commit(&filters(1, vec![0.0, 1.0]), 4),
            Err(Error::Generation)
        );
        assert_eq!(
            renderer.commit(&filters(2, vec![0.0, 1.0]), 0),
            Err(Error::Contract)
        );
        let mut out = [9.0; 2];
        assert_eq!(renderer.process(&[f32::NAN], &mut out), Err(Error::Numeric));
        assert_eq!(out, [0.0; 2]);
        assert_eq!(renderer.generation(), 1);
        renderer.process(&[1.0], &mut out).unwrap();
        assert_eq!(out, [0.5, 0.25]);
        renderer.commit(&filters(2, vec![0.0, 1.0]), 4).unwrap();
        assert_eq!(
            renderer.commit(&filters(3, vec![1.0, 0.0]), 4),
            Err(Error::TransitionBusy)
        );
        renderer.discontinuity();
        renderer.process(&[1.0], &mut out).unwrap();
        assert_eq!(out, [0.0, 1.0]);
    }

    #[test]
    fn rejects_unknown_shapes_rates_and_unbounded_filters() {
        for input in [
            Input::Objects(0),
            Input::Objects(17),
            Input::AmbisonicsAcnSn3d(0),
        ] {
            assert!(Filters::prepare(input, 48_000, 1, 1, vec![0.0; 2]).is_err());
        }
        for data in [
            vec![f32::NAN, 0.0],
            vec![17.0, 0.0],
            vec![f32::INFINITY, 0.0],
        ] {
            assert!(Filters::prepare(Input::Objects(1), 48_000, 1, 1, data).is_err());
        }
        assert!(Filters::prepare(Input::Objects(1), 0, 1, 1, vec![0.0; 2]).is_err());
    }
}
