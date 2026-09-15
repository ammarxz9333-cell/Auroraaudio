use aurora_renderer_basic::binaural::{Error, Filters, Input, PreparedBinaural};
use aurora_test_alloc::{count_allocations, CountingAllocator};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

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

#[test]
fn processing_transition_fault_and_recovery_allocate_zero_times() {
    let mut renderer = PreparedBinaural::new(filters(1, vec![0.5, 0.25]), 8).unwrap();
    let candidate = filters(2, vec![0.25, 0.5]);
    let mut output = [0.0; 16];
    let mut okay = true;
    let allocations = count_allocations(|| {
        okay &= renderer.commit(&candidate, 80).is_ok();
        for _ in 0..10_000 {
            okay &= renderer.process(&[0.5; 8], &mut output).is_ok();
        }
        okay &= renderer.process(&[f32::NAN; 8], &mut output) == Err(Error::Numeric);
        renderer.discontinuity();
        okay &= renderer.process(&[0.5; 8], &mut output).is_ok();
    });
    assert!(okay);
    assert_eq!(allocations, 0);
    assert!(output.iter().all(|x| x.is_finite()));
}
