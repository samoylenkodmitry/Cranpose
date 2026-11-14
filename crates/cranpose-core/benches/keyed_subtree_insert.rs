//! A column of keyed cards, about 100,000 nodes in all, takes a new card of
//! 3,195 nodes at its front and lets its last card go, as the patchwork
//! screen does every 64 frames. Every record of the new card enters the slot
//! table before the records of all the cards after it. The benchmark prints
//! the time of each entry, from the first one on.

#![allow(clippy::print_stdout, reason = "a benchmark prints its numbers")]

#[path = "../tests/support/ordered_parent.rs"]
mod ordered_parent;
#[path = "../tests/support/tracked_parent.rs"]
mod tracked_parent;

use std::time::Instant;

use cranpose_core::{
    Composition, MemoryApplier, MutableState, NodeId, key, location_key, pop_parent, push_parent,
    with_current_composer,
};
use cranpose_macros::composable;
use ordered_parent::{FixtureLeaf, ParentTracked};
use tracked_parent::TrackedParent;

const CARDS: usize = 31;
const ROWS: usize = 12;
const TILES: usize = 53;
const TILE_LEAVES: usize = 4;
const HEADER_LEAVES: usize = 2;
const CARD_NODES: usize = 1 + HEADER_LEAVES + ROWS * (1 + TILES * (1 + TILE_LEAVES));
const ENTRIES: usize = 6;

struct Leaf;

impl FixtureLeaf for Leaf {}

fn emit_parent() -> NodeId {
    with_current_composer(|composer| composer.emit_node(TrackedParent::default))
}

#[composable]
fn Column(first: MutableState<usize>) {
    let first = first.get();
    push_parent(emit_parent());
    for id in first..first + CARDS {
        key(id, || Card(id));
    }
    pop_parent();
}

#[composable]
fn Card(id: usize) {
    push_parent(emit_parent());
    for _ in 0..HEADER_LEAVES {
        Label(id);
    }
    for row in 0..ROWS {
        Row(id, row);
    }
    pop_parent();
}

#[composable]
fn Row(card: usize, row: usize) {
    push_parent(emit_parent());
    for tile in 0..TILES {
        Tile(card, row * TILES + tile);
    }
    pop_parent();
}

#[composable]
fn Tile(card: usize, tile: usize) {
    push_parent(emit_parent());
    for _ in 0..TILE_LEAVES {
        Label(card ^ tile);
    }
    pop_parent();
}

#[composable]
fn Label(value: usize) {
    std::hint::black_box(value);
    with_current_composer(|composer| {
        composer.emit_node(|| ParentTracked::new(Leaf));
    });
}

fn main() {
    let mut composition = Composition::new(MemoryApplier::new());
    let first = MutableState::with_runtime(ENTRIES, composition.runtime_handle());
    let start = Instant::now();
    composition
        .render(location_key(file!(), line!(), column!()), || {
            Column(first);
        })
        .expect("initial composition");
    let compose_us = start.elapsed().as_micros();
    let mut entries_us = Vec::with_capacity(ENTRIES);
    for entry in 1..=ENTRIES {
        first.set(ENTRIES - entry);
        let start = Instant::now();
        composition
            .process_invalid_scopes()
            .expect("card entry recomposition");
        entries_us.push(start.elapsed().as_micros());
    }
    let entries: Vec<String> = entries_us.iter().map(u128::to_string).collect();
    println!(
        "{{\"benchmark\":\"keyed_subtree_insert\",\"cards\":{CARDS},\"card_nodes\":{CARD_NODES},\"compose_us\":{compose_us},\"entries_us\":[{}]}}",
        entries.join(",")
    );
}
