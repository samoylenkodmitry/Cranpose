use std::{alloc::System, task::Poll};

use coroflow::{FlowExt, MutableStateFlow, Turbine, flow};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const EMISSIONS: u64 = 1_000;

fn operator_chain_allocations() {
    let state = MutableStateFlow::new(0_u64);
    let chain = state
        .as_state_flow()
        .map(|value| value * 2)
        .filter(|value| value % 4 == 0)
        .distinct_until_changed()
        .on_each(|_| {});
    let mut run = Turbine::of(&chain);
    assert_eq!(run.next_now(), Poll::Ready(Some(0)));

    let region = Region::new(GLOBAL);
    let mut received = 0;
    for value in 1..=EMISSIONS {
        state.set(value * 2);
        if let Poll::Ready(Some(_)) = run.next_now() {
            received += 1;
        }
    }
    let change = region.change();
    assert_eq!(received, EMISSIONS);
    assert_eq!(change.allocations, 0, "{change:?}");
}

fn allocations_to_collect(emissions: u64) -> (u64, usize) {
    let numbers = flow(move |emitter| async move {
        for value in 0..emissions {
            emitter.emit(value).await;
        }
    });
    let region = Region::new(GLOBAL);
    let mut run = Turbine::of(&numbers.map(|value| value + 1));
    let mut count = 0;
    while let Poll::Ready(Some(_)) = run.next_now() {
        count += 1;
    }
    (count, region.change().allocations)
}

fn flow_block_allocations() {
    let (few, few_allocations) = allocations_to_collect(10);
    let (many, many_allocations) = allocations_to_collect(EMISSIONS);
    assert_eq!((few, many), (10, EMISSIONS));
    assert_eq!(few_allocations, many_allocations);
    assert!(
        few_allocations <= 3,
        "a collection allocated {few_allocations} times"
    );
}

#[test]
fn flows_allocate_per_collection_and_never_per_emitted_value() {
    operator_chain_allocations();
    flow_block_allocations();
}
