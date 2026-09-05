mod common;
use aurora_config::ValidatedConfiguration;
use aurora_test_alloc::{count_allocations, CountingAllocator};
use common::stereo;
use std::hint::black_box;
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn immutable_field_reads_allocate_zero_times() {
    let validated = ValidatedConfiguration::new(stereo()).unwrap();
    let allocations = count_allocations(|| {
        for _ in 0..10_000 {
            black_box(validated.config().audio_format.sample_rate);
            black_box(validated.config().speaker_layout.speakers.as_slice());
            black_box(&validated.config().renderer);
        }
    });
    assert_eq!(allocations, 0);
}
