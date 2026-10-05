//! What marking a deep tree dirty costs: a chain of `DEPTH` nested parent
//! nodes whose innermost parent gains or loses a child each frame, so the
//! structural change marks every ancestor up to the root. Run it under
//! `/usr/bin/time -l` with `DEPTH=0` and `DEPTH=200` and divide the
//! difference in retired instructions by `FRAMES * 200`.

use std::hint::black_box;

use cranpose_core::{
    Composition, MemoryApplier, MutableState, Node, NodeId, location_key, pop_parent, push_parent,
    with_current_composer,
};
use cranpose_macros::composable;

#[path = "../tests/support/ordered_parent.rs"]
mod ordered_parent;

use ordered_parent::{FixtureLeaf, OrderedParent, ParentTracked};

/// A node about the size of a layout node, so the chain spreads over memory
/// as a screen's nodes do.
struct Leaf {
    _payload: [u64; 64],
}

impl FixtureLeaf for Leaf {}

/// An [`OrderedParent`] that tracks its own parent, so a structural change
/// below it marks the whole chain.
#[derive(Default)]
struct TrackedParent {
    inner: OrderedParent,
    parent: Option<NodeId>,
}

impl Node for TrackedParent {
    fn insert_child(&mut self, child: NodeId) -> bool {
        self.inner.insert_child(child)
    }

    fn remove_child(&mut self, child: NodeId) -> bool {
        self.inner.remove_child(child)
    }

    fn move_child(&mut self, from: usize, to: usize) {
        self.inner.move_child(from, to);
    }

    fn collect_children_into(&self, out: &mut smallvec::SmallVec<[NodeId; 8]>) {
        self.inner.collect_children_into(out);
    }

    fn collect_owned_children_into(&self, out: &mut smallvec::SmallVec<[NodeId; 8]>) {
        self.inner.collect_owned_children_into(out);
    }

    fn owned_child_index(&self, child: NodeId) -> Option<usize> {
        self.inner.owned_child_index(child)
    }

    fn on_attached_to_parent(&mut self, parent: NodeId) {
        self.parent = Some(parent);
    }

    fn on_removed_from_parent(&mut self) {
        self.parent = None;
    }

    fn parent(&self) -> Option<NodeId> {
        self.parent
    }
}

#[composable]
fn Chain(tick: MutableState<usize>, depth: usize) {
    with_current_composer(|composer| {
        let parent = composer.emit_node(|| ParentTracked::new(Leaf { _payload: [0; 64] }));
        black_box(parent);
    });
    if depth == 0 {
        Toggle(tick);
        return;
    }
    let parent = with_current_composer(|composer| composer.emit_node(TrackedParent::default));
    push_parent(parent);
    Chain(tick, depth - 1);
    pop_parent();
}

#[composable]
fn Toggle(tick: MutableState<usize>) {
    if tick.get().is_multiple_of(2) {
        with_current_composer(|composer| {
            composer.emit_node(|| ParentTracked::new(Leaf { _payload: [0; 64] }));
        });
    }
}

fn main() {
    let depth: usize = std::env::var("DEPTH")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(200);
    let frames: usize = std::env::var("FRAMES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(2000);
    let mut composition = Composition::new(MemoryApplier::new());
    let tick = MutableState::with_runtime(0usize, composition.runtime_handle());
    composition
        .render(location_key(file!(), line!(), column!()), || {
            Chain(tick, depth);
        })
        .expect("initial composition");
    for frame in 1..=frames {
        tick.set(frame);
        composition
            .process_invalid_scopes()
            .expect("scope recomposition");
    }
    println!("depth {depth} frames {frames}");
}
