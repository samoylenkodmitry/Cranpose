//! What one composable call costs when its parent recomposes: `LEAVES`
//! leaves under a parent that reads a state each frame. In the `skip` mode
//! the leaves get the same arguments every frame and skip; in the `run` mode
//! one argument changes, so every leaf runs. Run it under `/usr/bin/time -l`
//! with `LEAVES=0` and `LEAVES=1000` and divide the difference in retired
//! instructions by `FRAMES * 1000`.

use std::hint::black_box;

use cranpose_core::{Composition, MemoryApplier, MutableState, location_key};
use cranpose_macros::composable;

#[composable]
fn Scene(tick: MutableState<usize>, leaves: usize, run: bool) {
    let value = tick.get() * usize::from(run);
    for index in 0..leaves {
        Leaf(index, value);
    }
}

#[composable]
fn Leaf(index: usize, value: usize) {
    black_box((index, value));
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
    let mut composition = Composition::new(MemoryApplier::new());
    let tick = MutableState::with_runtime(0usize, composition.runtime_handle());
    composition
        .render(location_key(file!(), line!(), column!()), || {
            Scene(tick, leaves, run);
        })
        .expect("initial composition");
    for frame in 1..=frames {
        tick.set(frame);
        composition
            .process_invalid_scopes()
            .expect("scope recomposition");
    }
    println!("leaves {leaves} frames {frames} run {run}");
}
