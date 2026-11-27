//! What one composable call costs when its parent recomposes: `LEAVES`
//! leaves under a parent that reads a state each frame. In the `skip` mode
//! the leaves get the same arguments every frame and skip; in the `run` mode
//! one argument changes, so every leaf runs. Set `AFTER_EVICTION=1` to evict
//! a retained group before the measured frames so the run measures
//! `start_group` after retention eviction. Run it under `/usr/bin/time -l`
//! or callgrind with `LEAVES=0` and `LEAVES=1000` and divide the difference
//! in retired instructions by `FRAMES * 1000`.

#![allow(clippy::print_stdout, reason = "a benchmark prints its numbers")]

use std::hint::black_box;

use cranpose_core::{
    Composition, Key, MemoryApplier, MutableState, RecomposeOptions, RetentionBudget,
    RetentionEvictionPolicy, RetentionPolicy, location_key, remember, with_current_composer,
};
use cranpose_macros::composable;

const EVICTED_GROUP_KEY: Key = Key(0x2404_cafe);

#[composable]
fn Scene(tick: MutableState<usize>, show_retained: MutableState<bool>, leaves: usize, run: bool) {
    let value = tick.get() * usize::from(run);
    if show_retained.get() {
        with_current_composer(|composer| {
            composer.cranpose_with_reuse(EVICTED_GROUP_KEY, RecomposeOptions::default(), |_| {
                let _kept = remember(|| 1usize);
            });
        });
    }
    for index in 0..leaves {
        Leaf(index, value);
    }
}

#[composable]
fn Leaf(index: usize, value: usize) {
    black_box((index, value));
}

#[inline(never)]
fn run_frames(
    composition: &mut Composition<MemoryApplier>,
    tick: MutableState<usize>,
    frames: usize,
) {
    for frame in 1..=frames {
        tick.set(frame);
        composition
            .process_invalid_scopes()
            .expect("scope recomposition");
    }
}

fn main() {
    let leaves: usize = std::env::var("LEAVES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1000);
    let frames: usize = std::env::var("FRAMES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(2000);
    let run = std::env::var("MODE").as_deref() == Ok("run");
    let after_eviction = std::env::var("AFTER_EVICTION").as_deref() == Ok("1");
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let tick = MutableState::with_runtime(0usize, runtime.clone());
    let show_retained = MutableState::with_runtime(after_eviction, runtime);
    let root_key = location_key(file!(), line!(), column!());
    if after_eviction {
        composition.set_retention_policy(RetentionPolicy {
            budget: RetentionBudget {
                max_retained_subtrees: None,
                max_retained_bytes: None,
                max_age_passes: Some(1),
            },
            eviction: RetentionEvictionPolicy::LeastRecentlyDetached,
        });
    }
    composition
        .render(root_key, || {
            Scene(tick, show_retained, leaves, run);
        })
        .expect("initial composition");
    if after_eviction {
        show_retained.set(false);
        composition
            .process_invalid_scopes()
            .expect("hide retained group");
        tick.set(usize::MAX);
        composition
            .process_invalid_scopes()
            .expect("evict retained group");
        let stats = composition.debug_slot_table_stats();
        assert_eq!(stats.retained_subtree_count, 0);
        assert_eq!(stats.retained_evictions_total, 1);
        tick.set(0);
        composition
            .process_invalid_scopes()
            .expect("reset tick before measured frames");
    }
    run_frames(&mut composition, tick, frames);
    println!("leaves {leaves} frames {frames} run {run} after_eviction {after_eviction}");
}
