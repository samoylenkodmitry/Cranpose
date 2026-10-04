use super::*;
use crate::render_state::app_context_test_scope;

fn drained_requests() -> Vec<NodeId> {
    let mut drained = Vec::new();
    drain_lazy_prefetch_requests(|node| drained.push(node));
    drained
}

#[test]
fn a_request_is_drained_once_and_names_each_list_once() {
    let _scope = app_context_test_scope();
    assert!(!has_lazy_prefetch_requests());
    request_lazy_prefetch(3);
    request_lazy_prefetch(3);
    request_lazy_prefetch(5);
    assert!(has_lazy_prefetch_requests());
    assert_eq!(drained_requests(), vec![3, 5]);
    assert!(drained_requests().is_empty());
    assert!(!has_lazy_prefetch_requests());
}

#[test]
fn the_initial_pass_estimate_uses_the_recent_item_cost() {
    let _scope = app_context_test_scope();
    assert_eq!(lazy_prefetch_pass_cost(), Duration::ZERO);
    record_lazy_item_cost(Duration::from_millis(4));
    assert_eq!(lazy_prefetch_pass_cost(), Duration::from_millis(4));
    record_lazy_item_cost(Duration::ZERO);
    assert_eq!(
        lazy_prefetch_pass_cost(),
        Duration::from_millis(3),
        "a new item weighs a quarter"
    );
}

#[test]
fn a_prefetch_pass_is_one_only_while_it_runs() {
    let _scope = app_context_test_scope();
    assert!(!in_lazy_prefetch_pass());
    assert!(with_lazy_prefetch_pass(in_lazy_prefetch_pass));
    assert!(!in_lazy_prefetch_pass());
}

#[test]
fn warming_builds_the_slices_of_every_node_under_a_prefetched_item() {
    use cranpose_core::Applier as _;

    use crate::{layout::policies::EmptyMeasurePolicy, widgets::nodes::LayoutNode};

    let _scope = app_context_test_scope();
    let mut applier = cranpose_core::MemoryApplier::new();
    let node = || {
        LayoutNode::new(
            crate::Modifier::empty().padding(2.0),
            std::rc::Rc::new(EmptyMeasurePolicy),
        )
    };
    let leaf = applier.create(Box::new(node()));
    let mut row = node();
    row.children.push(leaf);
    let row = applier.create(Box::new(row));
    let outside = applier.create(Box::new(node()));
    let dirty = |applier: &mut cranpose_core::MemoryApplier, id| {
        applier
            .with_node::<LayoutNode, _>(id, |node| !node.modifier_slices_ready())
            .expect("a layout node")
    };
    assert!(dirty(&mut applier, row) && dirty(&mut applier, leaf));

    note_prefetched_item(&[row as u64]);
    warm_prefetched_slices(&mut applier);

    assert!(!dirty(&mut applier, row), "the item's root is warm");
    assert!(!dirty(&mut applier, leaf), "and so is every node under it");
    assert!(
        dirty(&mut applier, outside),
        "nodes outside it are left alone"
    );
    note_prefetched_item(&[]);
    warm_prefetched_slices(&mut applier);
    assert!(
        dirty(&mut applier, outside),
        "a warmed item is not warmed again"
    );
}
