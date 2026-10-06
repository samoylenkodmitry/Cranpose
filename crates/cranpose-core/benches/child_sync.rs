//! What a parent's unchanged children cost when the parent recomposes: a
//! parent node with `CHILDREN` child nodes recomposes each frame while every
//! child call skips, so each child is attached to the parent again and the
//! parent's child list is synced. Run it under `/usr/bin/time -l` with
//! `CHILDREN=0` and `CHILDREN=1000` and divide the difference in retired
//! instructions by `FRAMES * 1000`.

use std::hint::black_box;

use cranpose_core::{
    Composition, MemoryApplier, MutableState, location_key, pop_parent, push_parent,
    with_current_composer,
};
use cranpose_macros::composable;

#[path = "../tests/support/ordered_parent.rs"]
mod ordered_parent;

use ordered_parent::{FixtureLeaf, OrderedParent, ParentTracked};

/// A child node about the size of a layout node, so children spread over
/// memory as a screen's nodes do.
struct Leaf {
    _payload: [u64; 64],
}

impl FixtureLeaf for Leaf {}

#[composable]
fn Parent(tick: MutableState<usize>, children: usize) {
    black_box(tick.get());
    let parent = with_current_composer(|composer| composer.emit_node(OrderedParent::default));
    push_parent(parent);
    for index in 0..children {
        Child(index);
    }
    pop_parent();
}

#[composable]
fn Child(index: usize) {
    black_box(index);
    with_current_composer(|composer| {
        composer.emit_node(|| ParentTracked::new(Leaf { _payload: [0; 64] }));
    });
}

fn main() {
    let children: usize = std::env::var("CHILDREN")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1000);
    let frames: usize = std::env::var("FRAMES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(2000);
    let mut composition = Composition::new(MemoryApplier::new());
    let tick = MutableState::with_runtime(0usize, composition.runtime_handle());
    composition
        .render(location_key(file!(), line!(), column!()), || {
            Parent(tick, children);
        })
        .expect("initial composition");
    for frame in 1..=frames {
        tick.set(frame);
        composition
            .process_invalid_scopes()
            .expect("scope recomposition");
    }
    println!("children {children} frames {frames}");
}
