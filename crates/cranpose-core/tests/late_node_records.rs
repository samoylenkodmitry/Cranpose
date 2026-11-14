// A grid whose rows put their nodes into the grid's own group, each one
// after the cells of the rows before it, as a column of rows does: past the
// cells of a few rows each such record waits to join the grid's group with
// the ones after it. The first composition, and each recomposition that
// changes, adds or drops rows, leaves every row under the grid and every
// cell under its row, in order, and keeps the row nodes at the places that
// stay.

use std::{cell::RefCell, fmt::Write};

use cranpose_core::{
    Composition, MemoryApplier, MutableState, NodeId, location_key, pop_parent, push_parent,
    with_current_composer,
};

use super::{
    ordered_parent::{FixtureLeaf, ParentTracked},
    tracked_parent::TrackedParent,
};

// The cells of a row: two rows of them are more records than a record of
// the grid moves in place.
const CELLS: u32 = 40;

struct Label(RefCell<String>);

impl FixtureLeaf for Label {}

fn parent_node() -> NodeId {
    with_current_composer(|composer| composer.emit_node(TrackedParent::default))
}

#[cranpose_macros::composable]
fn Cell(row: u32, cell: u32) {
    with_current_composer(|composer| {
        let id = composer.emit_node(|| ParentTracked::new(Label(RefCell::default())));
        composer
            .with_node_mut::<ParentTracked<Label>, _>(id, |leaf| {
                *leaf.0.borrow_mut() = format!("{row}.{cell}");
            })
            .expect("a label node");
    });
}

// The grid and its rows emit their nodes in the grid's group; the cells
// compose in groups of their own.
#[cranpose_macros::composable]
fn Grid(rows: MutableState<Vec<u32>>) {
    push_parent(parent_node());
    for row in rows.get() {
        push_parent(parent_node());
        for cell in 0..CELLS {
            Cell(row, cell);
        }
        pop_parent();
    }
    pop_parent();
}

fn expected_shape(rows: &[u32]) -> String {
    let mut out = String::from("[");
    for row in rows {
        out.push('[');
        for cell in 0..CELLS {
            write!(out, "{row}.{cell} ").expect("write to a string");
        }
        out.push(']');
    }
    out.push(']');
    out
}

// Writes the tree under `node`, each parent's children in brackets.
fn shape_of(applier: &mut MemoryApplier, node: NodeId, out: &mut String) {
    match applier.with_node::<TrackedParent, _>(node, |parent| parent.inner.children.clone()) {
        Ok(children) => {
            out.push('[');
            for child in children {
                shape_of(applier, child, out);
            }
            out.push(']');
        }
        Err(_) => {
            let text = applier
                .with_node::<ParentTracked<Label>, _>(node, |leaf| leaf.0.borrow().clone())
                .expect("every node is a parent or a label");
            write!(out, "{text} ").expect("write to a string");
        }
    }
}

struct Harness {
    composition: Composition<MemoryApplier>,
    rows: MutableState<Vec<u32>>,
}

impl Harness {
    fn new(rows: Vec<u32>) -> Self {
        let mut composition = Composition::new(MemoryApplier::new());
        let rows = MutableState::with_runtime(rows, composition.runtime_handle());
        composition
            .render(location_key(file!(), line!(), column!()), move || {
                Grid(rows);
            })
            .expect("the first composition");
        Self { composition, rows }
    }

    fn show(&mut self, rows: Vec<u32>) {
        self.rows.set(rows);
        while self
            .composition
            .process_invalid_scopes()
            .expect("recomposition")
        {}
    }

    fn shape(&mut self) -> String {
        let root = self.composition.root().expect("the grid node");
        let mut out = String::new();
        shape_of(&mut self.composition.applier_mut(), root, &mut out);
        out
    }

    fn row_nodes(&mut self) -> Vec<NodeId> {
        let root = self.composition.root().expect("the grid node");
        self.composition
            .applier_mut()
            .with_node::<TrackedParent, _>(root, |grid| grid.inner.children.clone())
            .expect("the grid node")
    }
}

#[test]
fn rows_composed_for_the_first_time_sit_in_order_under_the_grid() {
    let rows: Vec<u32> = (0..6).collect();
    let mut harness = Harness::new(rows.clone());
    assert_eq!(harness.shape(), expected_shape(&rows));
}

#[test]
fn recomposed_rows_stay_in_order_and_keep_their_nodes() {
    let mut harness = Harness::new((0..6).collect());
    let mut previous = harness.row_nodes();

    for rows in [
        vec![0, 1, 7, 3, 4, 5],
        vec![0, 1, 3, 8, 9, 10, 11],
        vec![0, 1],
        (0..6).collect(),
    ] {
        harness.show(rows.clone());
        assert_eq!(harness.shape(), expected_shape(&rows), "rows {rows:?}");
        // The rows are not keyed: each row node stays at its place.
        let nodes = harness.row_nodes();
        let kept = previous.len().min(nodes.len());
        assert_eq!(
            nodes[..kept],
            previous[..kept],
            "the rows at the places that stay keep their nodes, rows {rows:?}"
        );
        previous = nodes;
    }
}
