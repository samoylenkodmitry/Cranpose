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
