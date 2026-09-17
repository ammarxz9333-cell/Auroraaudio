use aurora_renderer_basic::binaural::{Error, Filters, Input, PreparedBinaural};
use aurora_test_alloc::{count_allocations, CountingAllocator};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn filters(generation: u64, left: f32, right: f32) -> Filters {
    Filters::prepare(Input::Objects(1), 48_000, generation, 1, vec![left, right]).unwrap()
}

#[test]
fn prepared_binaural_callback_transition_fault_and_recovery_allocate_zero_times() {
    let mut renderer = PreparedBinaural::new(filters(1, 0.5, 0.25), 8).unwrap();
    let candidate = filters(2, 0.25, 0.5);
    let input = [0.5; 8];
    let invalid = [f32::NAN; 8];
    let mut output = [0.0; 16];
    let mut okay = true;

    let allocations = count_allocations(|| {
        okay &= renderer.commit(&candidate, 80).is_ok();
        for _ in 0..10_000 {
            okay &= renderer.process(&input, &mut output).is_ok();
        }
        okay &= renderer.process(&invalid, &mut output) == Err(Error::Numeric);
        renderer.discontinuity();
        okay &= renderer.process(&input, &mut output).is_ok();
    });

    assert!(okay);
    assert_eq!(allocations, 0);
    assert!(output.iter().all(|sample| sample.is_finite()));
}

#[test]
fn rejected_filter_commit_does_not_allocate_or_mutate_generation() {
    let mut renderer = PreparedBinaural::new(filters(4, 0.5, 0.25), 8).unwrap();
    let stale = filters(4, 0.25, 0.5);
    let mut result = Ok(());

    let allocations = count_allocations(|| {
        result = renderer.commit(&stale, 80);
    });

    assert_eq!(result, Err(Error::Generation));
    assert_eq!(renderer.generation(), 4);
    assert_eq!(allocations, 0);
}
