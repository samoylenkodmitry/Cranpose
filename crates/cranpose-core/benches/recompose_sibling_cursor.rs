use std::{cell::Cell, hint::black_box, rc::Rc, time::Instant};

use cranpose_core::{
    Composition, MemoryApplier, MutableState, location_key, pop_parent, push_parent,
    with_current_composer,
};
use cranpose_macros::composable;
#[path = "../tests/support/ordered_parent.rs"]
mod ordered_parent;

use ordered_parent::{OrderedParent, ParentTracked};

const FRAMES: usize = 512;
const INVALIDATED_ROWS: usize = 4;

struct Leaf;

impl ordered_parent::FixtureLeaf for Leaf {}

#[composable]
fn Scene(
    values: Rc<Vec<MutableState<usize>>>,
    insert: MutableState<bool>,
    updates: Rc<Cell<usize>>,
) {
    let parent = with_current_composer(|composer| composer.emit_node(OrderedParent::default));
    push_parent(parent);
    for index in 0..values.len() {
        Row(
            index,
            values[index],
            (index == values.len() / 2).then_some(insert),
            Rc::clone(&updates),
        );
    }
    pop_parent();
}

#[composable]
fn Row(
    index: usize,
    value: MutableState<usize>,
    insert: Option<MutableState<bool>>,
    updates: Rc<Cell<usize>>,
) {
    black_box(value.get());
    if insert.is_some_and(|insert| insert.get()) {
        Extra(index, Rc::clone(&updates));
    }
    emit_leaf(&updates);
}

#[composable]
fn Extra(_index: usize, updates: Rc<Cell<usize>>) {
    emit_leaf(&updates);
}

/// Emits a leaf and counts the emit.
fn emit_leaf(emits: &Cell<usize>) {
    emits.set(emits.get() + 1);
    with_current_composer(|composer| {
        composer.emit_node(|| ParentTracked::new(Leaf));
    });
}

struct Fixture {
    composition: Composition<MemoryApplier>,
    values: Rc<Vec<MutableState<usize>>>,
    insert: MutableState<bool>,
    updates: Rc<Cell<usize>>,
    invalidated_rows: Vec<usize>,
    frame: usize,
}

impl Fixture {
    fn new(sibling_count: usize) -> Self {
        let mut composition = Composition::new(MemoryApplier::new());
        let runtime = composition.runtime_handle();
        let values = Rc::new(
            (0..sibling_count)
                .map(|_| MutableState::with_runtime(0, runtime.clone()))
                .collect(),
        );
        let insert = MutableState::with_runtime(false, runtime);
        let updates = Rc::new(Cell::new(0));
        let root_key = location_key(file!(), line!(), column!());
        composition
            .render(root_key, || {
                Scene(Rc::clone(&values), insert, Rc::clone(&updates));
            })
            .expect("initial composition");
        let invalidated_rows = [
            0,
            sibling_count / 3,
            sibling_count * 2 / 3,
            sibling_count - 1,
        ]
        .into_iter()
        .collect();
        Self {
            composition,
            values,
            insert,
            updates,
            invalidated_rows,
            frame: 0,
        }
    }

    fn frame(&mut self) {
        for &row in &self.invalidated_rows {
            self.values[row].set(self.frame);
        }
        self.insert.set(self.frame.is_multiple_of(2));
        self.composition
            .process_invalid_scopes()
            .expect("scope recomposition");
        self.frame += 1;
        black_box(self.updates.get());
    }
}

fn run_case(sibling_count: usize) {
    let mut fixture = Fixture::new(sibling_count);
    for _ in 0..32 {
        fixture.frame();
    }
    let updates_before = fixture.updates.get();
    let start = Instant::now();
    for _ in 0..FRAMES {
        fixture.frame();
    }
    let elapsed = start.elapsed().as_nanos();
    println!(
        "{{\"benchmark\":\"recompose_sibling_cursor\",\"siblings\":{sibling_count},\"invalidated_rows_per_frame\":{},\"insert_control\":true,\"warmup_frames\":32,\"frames\":{FRAMES},\"node_emits\":{},\"elapsed_ns\":{elapsed},\"ns_per_frame\":{}}}",
        INVALIDATED_ROWS + 1,
        fixture.updates.get() - updates_before,
        elapsed / FRAMES as u128,
    );
}

fn main() {
    for sibling_count in [64, 256, 1024] {
        run_case(sibling_count);
    }
}
