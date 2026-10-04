use cranpose_core::{Applier, MemoryApplier, SceneNodeAttachmentScratch};

use super::ordered_parent::{FixtureLeaf, OrderedParent, ParentTracked};

struct Leaf;
impl FixtureLeaf for Leaf {}

#[test]
fn attachment_batch_scratch_observes_reparenting_between_batches() {
    let mut applier = MemoryApplier::new();
    let first_root = applier.create(Box::new(OrderedParent::default()));
    let second_root = applier.create(Box::new(OrderedParent::default()));
    let leaf = applier.create(Box::new(ParentTracked::new(Leaf)));

    applier
        .get_mut(first_root)
        .expect("first root exists")
        .insert_child(leaf);
    applier
        .get_mut(leaf)
        .expect("leaf exists")
        .on_attached_to_parent(first_root);

    let mut scratch = SceneNodeAttachmentScratch::default();
    let mut result = Vec::new();
    applier.scene_nodes_attached_to_into([leaf], first_root, &mut result, &mut scratch);
    assert_eq!(result.as_slice(), &[Some(leaf)]);

    applier
        .get_mut(first_root)
        .expect("first root exists")
        .remove_child(leaf);
    applier
        .get_mut(leaf)
        .expect("leaf exists")
        .on_removed_from_parent();
    applier
        .get_mut(second_root)
        .expect("second root exists")
        .insert_child(leaf);
    applier
        .get_mut(leaf)
        .expect("leaf exists")
        .on_attached_to_parent(second_root);

    applier.scene_nodes_attached_to_into([leaf], first_root, &mut result, &mut scratch);
    assert_eq!(result.as_slice(), &[None]);
    applier.scene_nodes_attached_to_into([leaf], second_root, &mut result, &mut scratch);
    assert_eq!(result.as_slice(), &[Some(leaf)]);

    applier.scene_nodes_attached_to_into([], second_root, &mut result, &mut scratch);
    assert!(result.is_empty());
}

#[test]
fn structural_change_apis_drain_candidates_and_filter_the_attached_parent() {
    let mut applier = MemoryApplier::new();
    let root = applier.create(Box::new(OrderedParent::default()));
    let child = applier.create(Box::new(ParentTracked::new(Leaf)));
    applier
        .get_mut(root)
        .expect("root exists")
        .insert_child(child);
    applier
        .get_mut(child)
        .expect("child exists")
        .on_attached_to_parent(root);

    applier.record_structural_change(root);
    assert_eq!(
        applier.take_structural_change_parents_attached_to(root),
        [root]
    );

    applier.record_structural_change(child);
    applier
        .get_mut(root)
        .expect("root exists")
        .remove_child(child);
    applier
        .get_mut(child)
        .expect("child exists")
        .on_removed_from_parent();
    assert!(
        applier
            .take_structural_change_parents_attached_to(root)
            .is_empty()
    );

    applier.record_structural_change(child);
    let mut candidates = Vec::new();
    applier.take_structural_change_parents_into(&mut candidates);
    assert_eq!(
        candidates,
        [child],
        "the raw API leaves filtering to its caller"
    );
}
