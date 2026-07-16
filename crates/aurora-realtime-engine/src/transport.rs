use std::sync::atomic::{AtomicUsize, Ordering};
use std::{cell::UnsafeCell, sync::Arc};

use crossbeam_queue::ArrayQueue;

/// Internal transport designs available to the duplex benchmark harness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    /// One bounded queue operation for every interleaved sample.
    SampleArrayQueue,
    /// Preallocated fixed-size blocks exchanged through SPSC slot indices.
    FixedBlockPool,
    /// Contiguous interleaved samples with one SPSC index update per block.
    ContiguousFrameRing,
}

impl TransportKind {
    /// Stable label used by benchmark output and architecture records.
    pub const fn label(self) -> &'static str {
        match self {
            Self::SampleArrayQueue => "array-queue-samples",
            Self::FixedBlockPool => "fixed-block-pool",
            Self::ContiguousFrameRing => "contiguous-frame-ring",
        }
    }
}

/// Bounded transport prototype used by benchmarks and contract tests.
///
/// The wrapper intentionally exposes no third-party queue type. It assumes one
/// producer and one consumer, matching the live duplex ownership contract.
#[derive(Clone)]
pub struct TransportPrototype {
    inner: Arc<TransportPrototypeInner>,
    block_samples: usize,
}

enum TransportPrototypeInner {
    Sample(Box<ArrayQueue<f32>>),
    Blocks(BlockPool),
    Ring(SpscSampleRing),
}

impl TransportPrototype {
    /// Allocates fixed transport storage before callback processing starts.
    pub fn new(kind: TransportKind, block_samples: usize, capacity_blocks: usize) -> Option<Self> {
        if block_samples == 0 || capacity_blocks < 2 {
            return None;
        }
        let sample_capacity = block_samples.checked_mul(capacity_blocks)?;
        let inner = match kind {
            TransportKind::SampleArrayQueue => {
                TransportPrototypeInner::Sample(Box::new(ArrayQueue::new(sample_capacity)))
            }
            TransportKind::FixedBlockPool => {
                TransportPrototypeInner::Blocks(BlockPool::new(block_samples, capacity_blocks)?)
            }
            TransportKind::ContiguousFrameRing => {
                TransportPrototypeInner::Ring(SpscSampleRing::new(sample_capacity)?)
            }
        };
        Some(Self {
            inner: Arc::new(inner),
            block_samples,
        })
    }

    /// Pushes exactly one benchmark block, returning false on bounded overflow.
    pub fn try_push_block(&self, input: &[f32]) -> bool {
        if input.len() != self.block_samples {
            return false;
        }
        match self.inner.as_ref() {
            TransportPrototypeInner::Sample(queue) => {
                if queue.capacity().saturating_sub(queue.len()) < input.len() {
                    return false;
                }
                input.iter().all(|sample| queue.push(*sample).is_ok())
            }
            TransportPrototypeInner::Blocks(pool) => pool.try_push(input),
            TransportPrototypeInner::Ring(ring) => ring.try_push_samples(input),
        }
    }

    /// Pops exactly one benchmark block, returning false on bounded underflow.
    pub fn try_pop_block(&self, output: &mut [f32]) -> bool {
        if output.len() != self.block_samples {
            return false;
        }
        match self.inner.as_ref() {
            TransportPrototypeInner::Sample(queue) => {
                if queue.len() < output.len() {
                    return false;
                }
                for sample in output {
                    *sample = queue.pop().expect("length checked before SPSC pop");
                }
                true
            }
            TransportPrototypeInner::Blocks(pool) => pool.try_pop(output),
            TransportPrototypeInner::Ring(ring) => ring.try_pop_samples(output),
        }
    }

    /// Returns current queued samples without changing bounded capacity.
    pub fn queued_samples(&self) -> usize {
        match self.inner.as_ref() {
            TransportPrototypeInner::Sample(queue) => queue.len(),
            TransportPrototypeInner::Blocks(pool) => pool.queued_blocks() * self.block_samples,
            TransportPrototypeInner::Ring(ring) => ring.len_samples(),
        }
    }

    /// Returns minimum uncontended atomic operations per complete push/pop pair.
    ///
    /// This is a source-level count, not a CPU instruction or cache-coherence
    /// measurement. Contention and compare-exchange retries add operations.
    pub fn visible_atomics_per_round_trip(&self) -> usize {
        match self.inner.as_ref() {
            TransportPrototypeInner::Sample(_) => self.block_samples.saturating_mul(8) + 4,
            TransportPrototypeInner::Blocks(_) => 8,
            TransportPrototypeInner::Ring(_) => 6,
        }
    }
}

pub(crate) struct SpscSampleRing {
    samples: Box<[UnsafeCell<f32>]>,
    head: AtomicUsize,
    tail: AtomicUsize,
}

// Safety: the transport contract permits exactly one producer and one consumer.
// The producer writes only unpublished slots; the consumer reads only published
// slots. Release/acquire index publication orders those disjoint accesses.
unsafe impl Sync for SpscSampleRing {}

impl SpscSampleRing {
    pub(crate) fn new(capacity_samples: usize) -> Option<Self> {
        if capacity_samples == 0 {
            return None;
        }
        let samples = (0..capacity_samples)
            .map(|_| UnsafeCell::new(0.0))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Some(Self {
            samples,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        })
    }

    pub(crate) fn capacity(&self) -> usize {
        self.samples.len()
    }

    pub(crate) fn len_samples(&self) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        head.wrapping_sub(tail).min(self.capacity())
    }

    pub(crate) fn try_push_samples(&self, input: &[f32]) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        if input.len() > self.capacity().saturating_sub(head.wrapping_sub(tail)) {
            return false;
        }
        for (offset, sample) in input.iter().enumerate() {
            let index = head.wrapping_add(offset) % self.capacity();
            // Safety: this producer owns every slot from head to the unpublished
            // new head, and the capacity check prevents overlap with the reader.
            unsafe { *self.samples[index].get() = *sample };
        }
        self.head
            .store(head.wrapping_add(input.len()), Ordering::Release);
        true
    }

    pub(crate) fn try_pop_samples(&self, output: &mut [f32]) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        if output.len() > head.wrapping_sub(tail) {
            return false;
        }
        for (offset, sample) in output.iter_mut().enumerate() {
            let index = tail.wrapping_add(offset) % self.capacity();
            // Safety: the acquire load published these initialized slots, and
            // only this consumer advances the tail that releases them.
            *sample = unsafe { *self.samples[index].get() };
        }
        self.tail
            .store(tail.wrapping_add(output.len()), Ordering::Release);
        true
    }

    pub(crate) fn discard_samples(&self, sample_count: usize) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        if sample_count > head.wrapping_sub(tail) {
            return false;
        }
        self.tail
            .store(tail.wrapping_add(sample_count), Ordering::Release);
        true
    }

    pub(crate) fn peek_samples(&self, output: &mut [f32]) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        if output.len() > head.wrapping_sub(tail) {
            return false;
        }
        for (offset, sample) in output.iter_mut().enumerate() {
            let index = tail.wrapping_add(offset) % self.capacity();
            *sample = unsafe { *self.samples[index].get() };
        }
        true
    }
}

struct BlockPool {
    samples: Box<[UnsafeCell<f32>]>,
    lengths: Box<[AtomicUsize]>,
    block_samples: usize,
    head: AtomicUsize,
    tail: AtomicUsize,
}

unsafe impl Sync for BlockPool {}

impl BlockPool {
    fn new(block_samples: usize, slots: usize) -> Option<Self> {
        let total = block_samples.checked_mul(slots)?;
        Some(Self {
            samples: (0..total)
                .map(|_| UnsafeCell::new(0.0))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            lengths: (0..slots)
                .map(|_| AtomicUsize::new(0))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            block_samples,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        })
    }

    fn try_push(&self, input: &[f32]) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        if head.wrapping_sub(tail) >= self.lengths.len() {
            return false;
        }
        let slot = head % self.lengths.len();
        let base = slot * self.block_samples;
        for (offset, sample) in input.iter().enumerate() {
            unsafe { *self.samples[base + offset].get() = *sample };
        }
        self.lengths[slot].store(input.len(), Ordering::Relaxed);
        self.head.store(head.wrapping_add(1), Ordering::Release);
        true
    }

    fn try_pop(&self, output: &mut [f32]) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        if tail == head {
            return false;
        }
        let slot = tail % self.lengths.len();
        if self.lengths[slot].load(Ordering::Relaxed) != output.len() {
            return false;
        }
        let base = slot * self.block_samples;
        for (offset, sample) in output.iter_mut().enumerate() {
            *sample = unsafe { *self.samples[base + offset].get() };
        }
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        true
    }

    fn queued_blocks(&self) -> usize {
        self.head
            .load(Ordering::Acquire)
            .wrapping_sub(self.tail.load(Ordering::Acquire))
            .min(self.lengths.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_transport_is_bounded_and_preserves_interleaved_order() {
        for kind in [
            TransportKind::SampleArrayQueue,
            TransportKind::FixedBlockPool,
            TransportKind::ContiguousFrameRing,
        ] {
            let transport = TransportPrototype::new(kind, 12, 2).unwrap();
            let first = (0..12).map(|value| value as f32).collect::<Vec<_>>();
            let second = (12..24).map(|value| value as f32).collect::<Vec<_>>();
            assert!(transport.try_push_block(&first));
            assert!(transport.try_push_block(&second));
            assert!(!transport.try_push_block(&first));
            let mut output = [0.0; 12];
            assert!(transport.try_pop_block(&mut output));
            assert_eq!(output.as_slice(), first);
            assert!(transport.try_pop_block(&mut output));
            assert_eq!(output.as_slice(), second);
            assert!(!transport.try_pop_block(&mut output));
            assert_eq!(transport.queued_samples(), 0);
        }
    }
}
