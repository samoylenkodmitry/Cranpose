use std::rc::Rc;

use cranpose_core::{Applier, MemoryApplier, NodeId};
use cranpose_ui::{
    AppContext, LayoutNode, Modifier, SemanticsNode, Size, layout::policies::LeafMeasurePolicy,
};

use super::{assert_matches_rebuild, first_difference};

fn labelled_leaf(applier: &mut MemoryApplier, label: &str) -> NodeId {
    let node = applier.create(Box::new(LayoutNode::new(
        Modifier::empty().content_description(label),
        Rc::new(LeafMeasurePolicy::new(Size::new(10.0, 10.0))),
    )));
    cranpose_ui::measure_layout(&mut *applier, node, Size::new(100.0, 100.0))
        .map_or_else(|err| panic!("the leaf lays out: {err}"), |_| node)
}

#[test]
fn a_tree_that_says_what_a_rebuild_does_passes() {
    AppContext::new().enter(|| {
        let mut applier = MemoryApplier::new();
        let leaf = labelled_leaf(&mut applier, "Saved");
        let tree = cranpose_ui::build_semantics_tree_from_applier(&mut applier, leaf)
            .unwrap_or_else(|err| panic!("the leaf builds: {err}"));
        assert_matches_rebuild(&mut applier, leaf, tree.as_ref());
    });
}

#[test]
#[should_panic(expected = "differs from a rebuilt one at node #")]
fn a_tree_that_says_something_else_names_the_node() {
    AppContext::new().enter(|| {
        let mut applier = MemoryApplier::new();
        let first = labelled_leaf(&mut applier, "First");
        let second = labelled_leaf(&mut applier, "Second");
        let tree = cranpose_ui::build_semantics_tree_from_applier(&mut applier, first)
            .unwrap_or_else(|err| panic!("the leaf builds: {err}"));
        assert_matches_rebuild(&mut applier, second, tree.as_ref());
    });
}

#[test]
fn the_first_difference_is_the_first_node_that_differs() {
    let tree = |label: &str| SemanticsNode {
        node_id: 1,
        children: vec![
            SemanticsNode {
                node_id: 2,
                ..Default::default()
            },
            SemanticsNode {
                node_id: 3,
                description: Some(label.to_owned()),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    assert_eq!(first_difference(&tree("a"), &tree("a")), None);
    let difference = first_difference(&tree("a"), &tree("b")).unwrap_or_default();
    assert!(difference.starts_with("node #3"), "{difference}");

    let mut reordered = tree("a");
    reordered.children.reverse();
    let difference = first_difference(&tree("a"), &reordered).unwrap_or_default();
    assert!(
        difference.starts_with("the children of #1: kept [2, 3], rebuilt [3, 2]"),
        "{difference}"
    );
}
