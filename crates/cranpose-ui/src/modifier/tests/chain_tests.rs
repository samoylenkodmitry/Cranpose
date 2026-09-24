use cranpose_foundation::{InvalidationKind, ModifierInvalidation, ModifierNode, NodeCapabilities};

use super::*;
use crate::{modifier::Color, modifier_nodes::PaddingNode};

#[test]
fn attaches_padding_node_and_invalidates_layout() {
    let mut handle = ModifierChainHandle::new();

    let _ = handle.update(&Modifier::empty().padding(8.0));

    assert_eq!(handle.chain().len(), 1);

    let invalidations = handle.take_invalidations();
    assert_eq!(
        invalidations,
        vec![ModifierInvalidation::new(
            InvalidationKind::Layout,
            NodeCapabilities::LAYOUT
        )]
    );
}

#[test]
fn reuses_nodes_between_updates() {
    let mut handle = ModifierChainHandle::new();

    let _ = handle.update(&Modifier::empty().padding(12.0));
    let first_ptr = node_ptr::<PaddingNode>(&handle);
    handle.take_invalidations();

    let _ = handle.update(&Modifier::empty().padding(12.0));
    let second_ptr = node_ptr::<PaddingNode>(&handle);

    assert_eq!(first_ptr, second_ptr, "expected the node to be reused");
    assert!(
        handle.take_invalidations().is_empty(),
        "no additional invalidations should be issued for a pure update"
    );
}

#[test]
fn modifier_slices_capture_background_and_shape() {
    use crate::modifier::slices::collect_modifier_slices;

    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(
        &Modifier::empty()
            .background(Color(0.2, 0.3, 0.4, 1.0))
            .then(Modifier::empty().rounded_corners(8.0)),
    );

    let slices = collect_modifier_slices(handle.chain());
    assert!(
        !slices.draw_commands().is_empty(),
        "Expected draw commands for background"
    );

    let _ = handle.update(
        &Modifier::empty()
            .rounded_corners(4.0)
            .then(Modifier::empty().background(Color(0.9, 0.1, 0.1, 1.0))),
    );
    let slices = collect_modifier_slices(handle.chain());
    assert!(
        !slices.draw_commands().is_empty(),
        "Expected draw commands after update"
    );

    let _ = handle.update(&Modifier::empty());
    let slices = collect_modifier_slices(handle.chain());
    assert!(
        slices.draw_commands().is_empty(),
        "Expected no draw commands with empty modifier"
    );
}

#[test]
fn capability_mask_updates_with_chain() {
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&Modifier::empty().padding(4.0));
    assert_eq!(handle.capabilities(), NodeCapabilities::LAYOUT);
    assert!(handle.has_layout_nodes());
    assert!(!handle.has_draw_nodes());
    handle.take_invalidations();

    let color = Color(0.5, 0.6, 0.7, 1.0);
    let _ = handle.update(&Modifier::empty().background(color));
    assert_eq!(handle.capabilities(), NodeCapabilities::DRAW);
    assert!(handle.has_draw_nodes());
    assert!(!handle.has_layout_nodes());
}

#[test]
fn offset_update_invalidates_layout_for_retained_placement() {
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&Modifier::empty().offset(0.0, 0.0));
    handle.take_invalidations();

    let _ = handle.update(&Modifier::empty().offset(12.0, 0.0));
    let invalidations = handle.take_invalidations();

    assert!(
        invalidations
            .iter()
            .any(|invalidation| invalidation.kind() == InvalidationKind::Layout),
        "expected offset value changes to refresh retained placement"
    );
    assert!(
        invalidations
            .iter()
            .all(|invalidation| invalidation.kind() != InvalidationKind::Draw),
        "offset value changes must not leave retained geometry stale through a draw-only invalidation"
    );
    assert_eq!(handle.resolved_modifiers().offset(), Point::new(12.0, 0.0));
}

fn node_ptr<N: ModifierNode + 'static>(handle: &ModifierChainHandle) -> *const N {
    handle
        .chain()
        .node::<N>(0)
        .map(|node| &*node as *const N)
        .expect("expected node to exist")
}
