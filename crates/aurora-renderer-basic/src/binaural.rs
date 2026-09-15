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
