use cranpose_core::{Applier, MemoryApplier, NodeId, NodeMarks};

use super::ordered_parent::{FixtureLeaf, OrderedParent, ParentTracked};

struct Leaf;
impl FixtureLeaf for Leaf {}

fn attach(applier: &mut MemoryApplier, child: NodeId, parent: NodeId) {
    applier
        .get_mut(child)
        .expect("child exists")
        .on_attached_to_parent(parent);
}

fn leaves(applier: &mut MemoryApplier, count: usize) -> Vec<NodeId> {
    (0..count)
        .map(|_| applier.create(Box::new(ParentTracked::new(Leaf))))
        .collect()
}

fn marked(marks: &NodeMarks, applier: &MemoryApplier, ids: &[NodeId]) -> usize {
    ids.iter()
        .filter(|&&id| marks.get(applier, id) != 0)
        .count()
}

// The node counts of a small applier, whose reset clears every mark at once,
// and of a large one, whose reset clears the few marks a walk set one by one.
const SIZES: [usize; 2] = [256, 20_000];

// A reset clears the few marks a walk set and many marks: both leave no node
// marked, whichever way the walk marked them and however many nodes there are.
#[test]
fn a_reset_unmarks_every_node_a_walk_marked_few_or_many() {
    for size in SIZES {
        reset_unmarks_few_or_many(size);
    }
}

fn reset_unmarks_few_or_many(size: usize) {
    let mut applier = MemoryApplier::new();
    let nodes = leaves(&mut applier, size);
    let mut marks = NodeMarks::default();
    marks.reset(&applier);

    let listed = size / 32;
    for count in [1, 7, listed, listed + 1, size / 2, size] {
        for &id in &nodes[..count] {
            marks.add(&applier, id, 1);
        }
        assert_eq!(
            marked(&marks, &applier, &nodes),
            count,
            "{count} marks added"
        );
        marks.reset(&applier);
        assert_eq!(marked(&marks, &applier, &nodes), 0, "{count} marks");
    }

    // A walk that marks a node again, or with no bits, keeps the reset whole.
    for _ in 0..20 {
        marks.set(&applier, nodes[3], 0);
        marks.add(&applier, nodes[3], 2);
    }
    marks.add_with_ancestors(&applier, nodes[200], 1, 1);
    marks.reset(&applier);
    assert_eq!(
        marked(&marks, &applier, &nodes),
        0,
        "marks left after a reset of repeated marks"
    );
}

// Nodes the applier stored after the reset, nodes it holds apart and the
// slots of removed nodes lose their marks at the next reset too.
#[test]
fn a_reset_unmarks_nodes_stored_after_it_and_nodes_kept_apart() {
    for size in SIZES {
        reset_unmarks_later_and_apart(size);
    }
}

fn reset_unmarks_later_and_apart(size: usize) {
    let mut applier = MemoryApplier::new();
    let first = leaves(&mut applier, size);
    let mut marks = NodeMarks::default();
    marks.reset(&applier);

    let later = leaves(&mut applier, 64);
    let missing = NodeId(1_000_000);
    for &id in later.iter().chain(&first[..2]).chain([&missing]) {
        marks.set(&applier, id, 3);
    }
    assert_eq!(
        marks.get(&applier, missing),
        3,
        "the mark of a missing node"
    );
    marks.reset(&applier);
    assert_eq!(marked(&marks, &applier, &first), 0, "first nodes marked");
    assert_eq!(marked(&marks, &applier, &later), 0, "later nodes marked");
    assert_eq!(
        marks.get(&applier, missing),
        0,
        "the mark of a missing node after a reset"
    );

    for &id in &later {
        marks.set(&applier, id, 1);
        applier.remove(id).expect("the leaf exists");
    }
    applier.compact();
    marks.reset(&applier);
    let refilled = leaves(&mut applier, 64);
    marks.reset(&applier);
    assert_eq!(
        marked(&marks, &applier, &refilled),
        0,
        "nodes in reused slots marked"
    );
    assert_eq!(
        marked(&marks, &applier, &first),
        0,
        "first nodes marked after the refill"
    );
}

#[test]
fn a_node_marks_its_ancestors_up_to_the_first_that_has_the_mark() {
    let mut applier = MemoryApplier::new();
    let root = applier.create(Box::new(OrderedParent::default()));
    let middle = applier.create(Box::new(ParentTracked::new(Leaf)));
    let first = applier.create(Box::new(ParentTracked::new(Leaf)));
    let second = applier.create(Box::new(ParentTracked::new(Leaf)));
    attach(&mut applier, middle, root);
    attach(&mut applier, first, middle);
    attach(&mut applier, second, middle);
    let mut marks = NodeMarks::default();
    marks.reset(&applier);

    marks.add_with_ancestors(&applier, first, 2, 1);
    let read = |marks: &NodeMarks, ids: [NodeId; 4]| ids.map(|id| marks.get(&applier, id));
    assert_eq!(read(&marks, [first, second, middle, root]), [2, 0, 1, 1]);

    // `middle` has the mark: the walk from `second` ends there.
    marks.set(&applier, root, 0);
    marks.add_with_ancestors(&applier, second, 2, 1);
    assert_eq!(read(&marks, [first, second, middle, root]), [2, 2, 1, 0]);
}
