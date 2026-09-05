use aurora_test_alloc::{count_allocations, CountingAllocator};
use std::hint::black_box;
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn counts_real_allocation_and_reallocation() {
    assert_eq!(count_allocations(|| {}), 0);
    assert!(
        count_allocations(|| {
            let mut bytes = Vec::with_capacity(8);
            bytes.extend_from_slice(&[1_u8; 8]);
            bytes.reserve(4096);
            black_box(bytes);
        }) >= 2
    );
}

#[test]
fn panic_restores_measurement_state() {
    let result = std::panic::catch_unwind(|| count_allocations(|| panic!("audit recovery")));
    assert!(result.is_err());
    assert_eq!(count_allocations(|| {}), 0);
}

#[test]
fn concurrent_measurements_remain_independent() {
    let threads: Vec<_> = (0..4)
        .map(|_| {
            std::thread::spawn(|| {
                for _ in 0..50 {
                    assert_eq!(
                        count_allocations(|| {
                            black_box(Box::new(42));
                        }),
                        1
                    );
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
}
