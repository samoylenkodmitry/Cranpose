use std::{cell::Cell, rc::Rc};

use cranpose_core::{
    Composition, Key, MemoryApplier, MutableState, RecomposeOptions, RetentionBudget,
    RetentionEvictionPolicy, RetentionPolicy, SlotTable, SlotsHost, location_key, remember,
    rememberMovableContentOf, with_current_composer,
};

const RETAINED_KEY_A: Key = Key(0x2404_0001);
const RETAINED_KEY_B: Key = Key(0x2404_0002);

fn retained_counter(key: Key, value_out: &Rc<Cell<u32>>) {
    let value_out = Rc::clone(value_out);
    with_current_composer(|composer| {
        composer.cranpose_with_reuse(key, RecomposeOptions::default(), |_| {
            let slot = remember(|| Cell::new(0u32));
            slot.with(|cell| {
                let next = cell.get() + 1;
                cell.set(next);
                value_out.set(next);
            });
        });
    });
}

#[test]
fn retention_eviction_empties_retained_count_while_keeping_eviction_stats() {
    let mut composition = Composition::new(MemoryApplier::new());
    composition.set_retention_policy(RetentionPolicy {
        budget: RetentionBudget {
            max_retained_subtrees: None,
            max_retained_bytes: None,
            max_age_passes: Some(1),
        },
        eviction: RetentionEvictionPolicy::LeastRecentlyDetached,
    });
    let runtime = composition.runtime_handle();
    let show = MutableState::with_runtime(true, runtime.clone());
    let tick = MutableState::with_runtime(0u32, runtime);
    let seen = Rc::new(Cell::new(0u32));
    let root_key = location_key(file!(), line!(), column!());

    let render = |composition: &mut Composition<MemoryApplier>| {
        let seen = Rc::clone(&seen);
        composition
            .render(root_key, move || {
                let _ = tick.get();
                if show.get() {
                    retained_counter(RETAINED_KEY_A, &seen);
                }
            })
            .expect("render composition");
    };

    render(&mut composition);
    assert_eq!(seen.get(), 1);

    // Hide the retained group so it enters retention.
    show.set(false);
    render(&mut composition);
    let stats = composition.debug_slot_table_stats();
    assert_eq!(stats.retained_subtree_count, 1);
    assert_eq!(stats.retained_evictions_total, 0);

    // Advance one pass so the retained group ages out and retention empties.
    tick.set(1);
    render(&mut composition);
    let stats = composition.debug_slot_table_stats();
    assert_eq!(stats.retained_subtree_count, 0);
    assert_eq!(stats.retained_evictions_total, 1);

    // Later passes keep eviction stats while composing fresh groups.
    show.set(true);
    tick.set(2);
    render(&mut composition);
    assert_eq!(seen.get(), 1, "an evicted group starts with fresh state");
    let stats = composition.debug_slot_table_stats();
    assert_eq!(stats.retained_subtree_count, 0);
    assert_eq!(stats.retained_evictions_total, 1);
}

#[test]
fn restoring_after_budget_eviction_empties_retention_and_keeps_eviction_stats() {
    let mut composition = Composition::new(MemoryApplier::new());
    composition.set_retention_policy(RetentionPolicy {
        budget: RetentionBudget {
            max_retained_subtrees: Some(1),
            max_retained_bytes: None,
            max_age_passes: None,
        },
        eviction: RetentionEvictionPolicy::LeastRecentlyDetached,
    });
    let runtime = composition.runtime_handle();
    let show_a = MutableState::with_runtime(true, runtime.clone());
    let show_b = MutableState::with_runtime(true, runtime);
    let seen_a = Rc::new(Cell::new(0u32));
    let seen_b = Rc::new(Cell::new(0u32));
    let root_key = location_key(file!(), line!(), column!());

    let render = |composition: &mut Composition<MemoryApplier>| {
        let seen_a = Rc::clone(&seen_a);
        let seen_b = Rc::clone(&seen_b);
        composition
            .render(root_key, move || {
                if show_a.get() {
                    retained_counter(RETAINED_KEY_A, &seen_a);
                }
                if show_b.get() {
                    retained_counter(RETAINED_KEY_B, &seen_b);
                }
            })
            .expect("render composition");
    };

    render(&mut composition);
    assert_eq!((seen_a.get(), seen_b.get()), (1, 1));

    // Hide both so key A evicts under the 1-subtree budget while key B stays.
    show_a.set(false);
    show_b.set(false);
    render(&mut composition);
    let stats = composition.debug_slot_table_stats();
    assert_eq!(stats.retained_subtree_count, 1);
    assert_eq!(stats.retained_evictions_total, 1);

    // Restore key B so retention empties while eviction count stays 1.
    show_b.set(true);
    render(&mut composition);
    assert_eq!(seen_b.get(), 2, "key B restores its remembered counter");
    let stats = composition.debug_slot_table_stats();
    assert_eq!(stats.retained_subtree_count, 0);
    assert_eq!(stats.retained_evictions_total, 1);
}

#[test]
fn cross_host_movable_restore_empties_secondary_host_retention() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let show_in_host = MutableState::with_runtime(true, runtime.clone());
    let show_in_root = MutableState::with_runtime(false, runtime);
    let host = Rc::new(SlotsHost::new(SlotTable::new()));
    let root_key = location_key(file!(), line!(), column!());

    let render = |composition: &mut Composition<MemoryApplier>| {
        let host = Rc::clone(&host);
        composition
            .render(root_key, move || {
                let movable = rememberMovableContentOf(|| {
                    let _slot = remember(|| 42u32);
                });
                with_current_composer(|composer| {
                    composer
                        .subcompose_in(&host, None, |_| {
                            if show_in_host.get() {
                                movable.show();
                            }
                        })
                        .expect("subcompose_in");
                });
                if show_in_root.get() {
                    movable.show();
                }
            })
            .expect("render composition");
    };

    render(&mut composition);

    // Detach in the secondary host so the movable waits in host retention.
    show_in_host.set(false);
    render(&mut composition);

    // Move into root so the secondary host empties via take_retained_movable_elsewhere.
    show_in_root.set(true);
    render(&mut composition);

    let stats = composition.debug_slot_table_stats();
    assert_eq!(stats.retained_subtree_count, 0);
}

#[test]
fn zero_subtree_budget_evicts_on_insert_and_keeps_eviction_stats() {
    let mut composition = Composition::new(MemoryApplier::new());
    composition.set_retention_policy(RetentionPolicy {
        budget: RetentionBudget {
            max_retained_subtrees: Some(0),
            max_retained_bytes: None,
            max_age_passes: None,
        },
        eviction: RetentionEvictionPolicy::LeastRecentlyDetached,
    });
    let runtime = composition.runtime_handle();
    let show = MutableState::with_runtime(true, runtime);
    let seen = Rc::new(Cell::new(0u32));
    let root_key = location_key(file!(), line!(), column!());

    let render = |composition: &mut Composition<MemoryApplier>| {
        let seen = Rc::clone(&seen);
        composition
            .render(root_key, move || {
                if show.get() {
                    retained_counter(RETAINED_KEY_A, &seen);
                }
            })
            .expect("render composition");
    };

    render(&mut composition);
    assert_eq!(seen.get(), 1);

    show.set(false);
    render(&mut composition);
    let stats = composition.debug_slot_table_stats();
    assert_eq!(stats.retained_subtree_count, 0);
    assert_eq!(stats.retained_evictions_total, 1);

    show.set(true);
    render(&mut composition);
    assert_eq!(seen.get(), 1);
    let stats = composition.debug_slot_table_stats();
    assert_eq!(stats.retained_subtree_count, 0);
    assert_eq!(stats.retained_evictions_total, 1);
}
