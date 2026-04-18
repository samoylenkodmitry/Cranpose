use super::{
    ChildReusePolicy, GapMetadata, GroupFrame, NodeSlotState, OrphanedNode, OrphanedNodeIds, Slot,
    SlotTable,
};
use crate::{AnchorId, Owned, RecomposeScope};

#[test]
fn large_slot_tables_grow_incrementally_instead_of_doubling() {
    let mut table = SlotTable::new();
    table.append_gap_slots(SlotTable::LARGE_GROWTH_THRESHOLD);

    table.grow_slots();

    assert_eq!(table.slots.len(), 40_960);
    assert!(table.slots.capacity() >= table.slots.len());

    table.grow_slots();

    assert_eq!(table.slots.len(), 51_200);
    assert!(table.slots.capacity() >= table.slots.len());
}

#[test]
fn flushing_large_pending_slot_drops_releases_excess_capacity() {
    let mut table = SlotTable::new();
    for _ in 0..4096 {
        table.pending_slot_drops.push(Slot::default());
    }

    table.flush_pending_slot_drops();

    let stats = table.debug_stats();
    assert_eq!(stats.pending_slot_drops_len, 0);
    assert!(
        stats.pending_slot_drops_cap <= SlotTable::MIN_RETAINED_PENDING_SLOT_DROPS_CAPACITY * 4,
        "pending slot drop capacity stayed too large after flush: {stats:?}",
    );
}

#[test]
fn draining_large_orphaned_node_ids_releases_excess_capacity() {
    let mut table = SlotTable::new();
    for node_id in 0..4096 {
        table
            .orphaned_node_ids
            .push(OrphanedNode::new(node_id, 0, AnchorId::INVALID));
    }

    let mut drained = Vec::new();
    table.drain_orphaned_node_ids_with(|node| drained.push(node));

    let stats = table.debug_stats();
    assert_eq!(stats.orphaned_node_ids_len, 0);
    assert_eq!(drained.len(), 4096);
    assert_eq!(drained[0], OrphanedNode::new(0, 0, AnchorId::INVALID));
    assert_eq!(drained[4095], OrphanedNode::new(4095, 0, AnchorId::INVALID));
    assert!(
        stats.orphaned_node_ids_cap <= OrphanedNodeIds::INITIAL_CAPACITY * 4,
        "orphaned node id capacity stayed too large after drain: {stats:?}",
    );
}

#[test]
fn compaction_rehouses_surviving_value_payloads() {
    let mut table = SlotTable::new();
    let survivor_idx = table.use_value_slot(|| String::from("survivor"));
    let survivor_before = table.read_value::<String>(survivor_idx) as *const String;

    for index in 0..4096 {
        table.use_value_slot(|| format!("drop-{index}"));
    }

    table.cursor = 1;
    assert!(table.trim_to_cursor(), "test setup should produce gaps");
    table.compact();

    let survivor_after = table.read_value::<String>(0);
    let survivor_after_ptr = survivor_after as *const String;
    assert_eq!(survivor_after, "survivor");
    assert_ne!(
        survivor_before, survivor_after_ptr,
        "compaction should move surviving value payloads into fresh boxes"
    );
}

#[test]
fn compaction_releases_peak_anchor_storage() {
    let mut table = SlotTable::new();
    table.use_value_slot(|| String::from("survivor"));
    for index in 0..4096 {
        table.use_value_slot(|| format!("drop-{index}"));
    }

    table.cursor = 1;
    assert!(table.trim_to_cursor(), "test setup should produce gaps");
    table.compact();

    let stats = table.debug_stats();
    assert!(
        stats.anchors_cap <= stats.slots_len.saturating_mul(4).max(8),
        "anchor storage stayed too large after compaction: {stats:?}",
    );
    assert!(
        stats.free_anchor_ids_cap <= 8,
        "free anchor id storage stayed too large after compaction: {stats:?}",
    );
}

#[test]
fn compaction_reclaims_large_fractional_gap_blocks() {
    let mut table = SlotTable::new();

    for index in 0..1536 {
        table.use_value_slot(|| format!("slot-{index}"));
    }

    table.cursor = 1024;
    assert!(
        table.trim_to_cursor(),
        "test setup should produce a large gap block"
    );
    table.compact();

    assert_eq!(
        table.slots.len(),
        1024,
        "fractional large gap blocks should compact once hidden payloads become significant",
    );
}

#[test]
fn compaction_preserves_anchor_identity_for_shifted_slots() {
    let runtime = crate::runtime::TestRuntime::new();
    let mut table = SlotTable::new();

    table.reset();
    let first_group = table.begin_scoped_group(1, || RecomposeScope::new(runtime.handle()));
    let first_group_index = first_group.group.0;
    let first_group_scope_id = first_group.scope.id();
    table.use_value_slot(|| String::from("drop-a"));
    table.use_value_slot(|| String::from("drop-b"));
    table.end();

    let second_group = table.start(2);
    table.set_group_scope(second_group, 22);
    let (survivor_index, survivor_anchor) = table.remember_with_anchor(|| String::from("survivor"));
    table.end();
    table.flush_anchors_if_dirty();

    assert_eq!(
        table
            .read_value_by_anchor::<Owned<String>>(survivor_anchor)
            .map(|value| value.with(|text| text.clone())),
        Some(String::from("survivor"))
    );
    assert_eq!(table.resolve_anchor(survivor_anchor), Some(survivor_index));

    table.reset();
    let started = table
        .start_recranpose_at_anchor(first_group.anchor, first_group_scope_id)
        .expect("recompose scope should be found");
    assert_eq!(started, first_group_index);
    table.end_recompose();
    assert!(
        table.needs_compact,
        "shrinking the first group should create gaps"
    );

    table.compact();

    let shifted_index = table
        .resolve_anchor(survivor_anchor)
        .expect("survivor anchor should still resolve after compaction");
    assert!(
        shifted_index < survivor_index,
        "survivor slot should move left when leading gaps are compacted"
    );
    assert_eq!(
        table
            .read_value_by_anchor::<Owned<String>>(survivor_anchor)
            .map(|value| value.with(|text| text.clone())),
        Some(String::from("survivor"))
    );

    table
        .read_value_mut_by_anchor::<Owned<String>>(survivor_anchor)
        .expect("mutable anchor lookup should remain valid after compaction")
        .replace(String::from("survivor-updated"));
    assert_eq!(
        table
            .read_value::<Owned<String>>(shifted_index)
            .with(|text| text.clone()),
        "survivor-updated"
    );
}

#[test]
fn sibling_search_does_not_steal_nested_matching_group() {
    let mut table = SlotTable::new();
    let root_key = crate::hash_key(&"root");
    let target_key = crate::location_key("target", 10, 20);
    let container_key = crate::location_key("container", 11, 21);

    table.reset();
    let root = table.start(root_key);
    table.set_group_scope(root, 1);

    let container = table.start(container_key);
    table.set_group_scope(container, 10);

    let nested = table.start(target_key);
    table.set_group_scope(nested, 22);
    table.use_value_slot(|| String::from("nested"));
    table.end();

    table.end();
    table.end();
    table.flush_anchors_if_dirty();

    let root_end = match table.slots[root] {
        Slot::Group { len, .. } => root + super::unpack_slot_len(len),
        _ => panic!("root should remain a group"),
    };

    table.cursor = container;
    table.group_stack.push(GroupFrame {
        key: root_key,
        start: root,
        end: root_end,
        child_reuse: ChildReusePolicy::Normal,
        fresh_body: false,
        gap_boundary_key: root_key,
    });

    let inserted = table.start(target_key);

    assert_eq!(inserted, container);
    assert_eq!(
        table.get_group_scope(inserted),
        None,
        "search should not steal a nested descendant group",
    );
}

#[test]
fn explicit_keys_relocate_matching_sibling_groups() {
    let mut table = SlotTable::new();
    let root_key = crate::hash_key(&"root");
    let target_key = crate::hash_key(&"target");
    let other_key = crate::hash_key(&"other");

    table.reset();
    let root = table.start(root_key);
    table.set_group_scope(root, 1);

    let first = table.start(other_key);
    table.set_group_scope(first, 10);
    table.use_value_slot(|| String::from("other"));
    table.end();

    let later = table.start(target_key);
    table.set_group_scope(later, 22);
    table.use_value_slot(|| String::from("later"));
    table.end();

    table.end();
    table.flush_anchors_if_dirty();

    let root_end = match table.slots[root] {
        Slot::Group { len, .. } => root + super::unpack_slot_len(len),
        _ => panic!("root should remain a group"),
    };

    table.cursor = first;
    table.group_stack.push(GroupFrame {
        key: root_key,
        start: root,
        end: root_end,
        child_reuse: ChildReusePolicy::Normal,
        fresh_body: false,
        gap_boundary_key: root_key,
    });

    let relocated = table.start(target_key);

    assert_eq!(relocated, first);
    assert_eq!(
        table.get_group_scope(relocated),
        Some(22),
        "explicit keyed groups should preserve scope identity when reordered",
    );
}

#[test]
fn fresh_parent_does_not_restore_matching_child_from_replaced_subtree() {
    let mut table = SlotTable::new();
    let root_key = crate::hash_key(&"root");
    let old_parent_key = crate::hash_key(&"old-parent");
    let new_parent_key = crate::hash_key(&"new-parent");
    let shared_child_key = crate::hash_key(&"shared-child");

    table.reset();
    let root = table.start(root_key);
    table.set_group_scope(root, 1);

    let old_parent = table.start(old_parent_key);
    table.set_group_scope(old_parent, 10);

    let old_child = table.start(shared_child_key);
    table.set_group_scope(old_child, 20);
    table.use_value_slot(|| String::from("old-child"));
    table.end();

    table.end();
    table.end();
    table.flush_anchors_if_dirty();

    table.reset();
    let reused_root = table.start(root_key);
    assert_eq!(reused_root, root);

    let fresh_parent = table.start(new_parent_key);
    assert_eq!(fresh_parent, old_parent);

    let fresh_child = table.start(shared_child_key);
    let fresh_value = table.use_value_slot(|| String::from("fresh-child"));

    assert_eq!(fresh_child, old_child);
    assert_eq!(
        table.get_group_scope(fresh_child),
        None,
        "a fresh parent must not restore a descendant from the subtree it replaced",
    );
    assert_eq!(
        fresh_value,
        old_child + 1,
        "fresh child should reuse the slot position but not the previous descendant value slot",
    );
    assert_eq!(
        table.read_value::<String>(fresh_value),
        "fresh-child",
        "fresh child must not inherit the replaced subtree's remembered values",
    );
}

#[test]
fn end_recompose_marks_shrunk_group_tail_as_gaps_for_compaction() {
    let runtime = crate::runtime::TestRuntime::new();
    let mut table = SlotTable::new();

    table.reset();
    let scoped = table.begin_scoped_group(100, || RecomposeScope::new(runtime.handle()));
    let group = scoped.group.0;
    let scope_slot = group + 1;
    let kept_slot = table.use_value_slot(|| String::from("keep"));
    let dropped_slot = table.use_value_slot(|| String::from("drop"));
    table.end();
    table.flush_anchors_if_dirty();

    assert_eq!(
        table.read_value::<RecomposeScope>(scope_slot).id(),
        scoped.scope.id()
    );
    assert_eq!(table.read_value::<String>(kept_slot), "keep");
    assert_eq!(table.read_value::<String>(dropped_slot), "drop");

    table.reset();
    let started = table
        .start_recranpose_at_anchor(table.group_anchor_at(group), scoped.scope.id())
        .expect("recompose scope should be found");
    assert_eq!(started, group);
    let reused_kept_slot = table.use_value_slot(|| String::from("keep"));
    assert_eq!(reused_kept_slot, kept_slot);
    table.end_recompose();

    assert!(
        table.needs_compact,
        "shrinking recompose should mark the old tail as gaps"
    );

    table.compact();

    assert_eq!(table.slots.len(), 3);
    assert_eq!(
        table.read_value::<RecomposeScope>(scope_slot).id(),
        scoped.scope.id()
    );
    assert_eq!(table.read_value::<String>(kept_slot), "keep");
}

#[test]
fn restoring_scoped_gap_group_preserves_scope_slot_layout() {
    let runtime = crate::runtime::TestRuntime::new();
    let mut table = SlotTable::new();

    table.reset();
    let root = table.start(1);
    let scoped = table.begin_scoped_group(2, || RecomposeScope::new(runtime.handle()));
    let group = scoped.group.0;
    let body_slot = table.use_value_slot(|| String::from("body"));
    table.end_group();
    table.end();
    table.flush_anchors_if_dirty();

    table.reset();
    let reused_root = table.start(1);
    assert_eq!(reused_root, root);
    assert!(
        table.trim_to_cursor(),
        "omitted child should become a preserved gap"
    );
    table.end();

    let metadata = table
        .gap_metadata_at(group)
        .expect("scoped group gap metadata should be preserved");
    assert!(
        metadata.has_scope_slot,
        "preserved scoped groups must keep their scope-slot layout"
    );

    table.reset();
    let reused_root = table.start(1);
    assert_eq!(reused_root, root);
    let restored = table.begin_scoped_group(2, || RecomposeScope::new(runtime.handle()));
    assert_eq!(restored.group.0, group);
    assert!(
        restored.restored_from_gap,
        "restoring a preserved scoped group should report gap restoration",
    );
    assert_eq!(restored.scope.id(), scoped.scope.id());

    let reused_body_slot = table.use_value_slot(|| String::from("body"));
    assert_eq!(
        reused_body_slot, body_slot,
        "restored scoped groups must keep the first body slot after the scope slot",
    );
    assert_eq!(table.read_value::<String>(reused_body_slot), "body");
    table.end_group();
    table.end();
}

#[test]
fn marking_existing_nested_gap_preserves_child_group_metadata() {
    let mut table = SlotTable::new();

    table.reset();
    let root = table.start(1);
    table.set_group_scope(root, 1);

    let parent = table.start(2);
    table.set_group_scope(parent, 2);

    let child = table.start(3);
    table.set_group_scope(child, 3);
    table.use_value_slot(|| String::from("child"));
    table.end();

    table.end();
    table.end();
    table.flush_anchors_if_dirty();

    let parent_end = match table.slots[parent] {
        Slot::Group { len, .. } => parent + super::unpack_slot_len(len),
        _ => panic!("parent should remain a group"),
    };

    assert!(
        table.mark_range_as_gaps(parent, parent_end, Some(root)),
        "initial mark should convert the subtree to gaps",
    );
    assert!(
        table.mark_range_as_gaps(parent, parent_end, Some(root)),
        "re-marking an existing gap subtree should still preserve child metadata",
    );

    assert!(
        matches!(table.slots[parent], Slot::Gap { .. }),
        "parent should be a preserved gap",
    );
    let parent_metadata = table
        .gap_metadata_at(parent)
        .expect("parent gap metadata should be preserved");
    assert_eq!(parent_metadata.group_key, Some(2));
    assert_eq!(parent_metadata.group_scope.to_option(), Some(2));
    assert_eq!(super::unpack_slot_len(parent_metadata.group_len), 3);

    assert!(
        matches!(table.slots[child], Slot::Gap { .. }),
        "child should be a preserved gap",
    );
    let child_metadata = table
        .gap_metadata_at(child)
        .expect("child gap metadata should be preserved");
    assert_eq!(
        child_metadata.group_key,
        Some(3),
        "child gap metadata must survive repeated ancestor gap marking",
    );
    assert_eq!(child_metadata.group_scope.to_option(), Some(3));
    assert_eq!(super::unpack_slot_len(child_metadata.group_len), 2);
}

#[test]
fn marking_nested_gap_preserves_each_groups_boundary_key() {
    let mut table = SlotTable::new();

    table.reset();
    let root = table.start(1);
    let parent = table.start(2);
    let child = table.start(3);
    table.end();
    table.end();
    table.end();

    match &mut table.slots[parent] {
        Slot::Group { boundary_key, .. } => *boundary_key = 20,
        _ => panic!("parent should be a group"),
    }
    match &mut table.slots[child] {
        Slot::Group { boundary_key, .. } => *boundary_key = 30,
        _ => panic!("child should be a group"),
    }

    let parent_end = match table.slots[parent] {
        Slot::Group { len, .. } => parent + super::unpack_slot_len(len),
        _ => panic!("parent should remain a group"),
    };

    assert!(
        table.mark_range_as_gaps(parent, parent_end, Some(root)),
        "initial mark should convert the subtree to gaps",
    );

    let parent_metadata = table
        .gap_metadata_at(parent)
        .expect("parent gap metadata should be preserved");
    assert_eq!(
        parent_metadata.boundary_key,
        Some(20),
        "parent gap must preserve its own boundary key instead of inheriting the owner boundary",
    );

    let child_metadata = table
        .gap_metadata_at(child)
        .expect("child gap metadata should be preserved");
    assert_eq!(
        child_metadata.boundary_key,
        Some(30),
        "nested preserved groups must keep their own boundary keys",
    );
}

#[test]
fn start_recranpose_at_anchor_rejects_mismatched_scope_owner() {
    let runtime = crate::runtime::TestRuntime::new();
    let mut table = SlotTable::new();
    let root_key = crate::hash_key(&"root");
    let group_key = crate::hash_key(&"group");

    table.reset();
    let root = table.start(root_key);
    table.set_group_scope(root, 1);

    let first_group = table.begin_scoped_group(group_key, || RecomposeScope::new(runtime.handle()));
    let group = first_group.group.0;
    let first_scope_id = first_group.scope.id();
    table.use_value_slot(|| String::from("first"));
    table.end();
    table.end();
    table.flush_anchors_if_dirty();

    let anchor = first_group.anchor;

    table.reset();
    let reused_root = table.start(root_key);
    assert_eq!(reused_root, root);
    let reused_group =
        table.begin_scoped_group(group_key, || RecomposeScope::new(runtime.handle()));
    assert_eq!(reused_group.group.0, group);
    assert_eq!(
        reused_group.scope.id(),
        first_scope_id,
        "scoped group reuse should preserve the existing owner scope",
    );
    table.use_value_slot(|| String::from("second"));
    table.end();
    table.end();
    table.flush_anchors_if_dirty();

    table.reset();
    assert_eq!(
        table.start_recranpose_at_anchor(anchor, first_scope_id + 1000),
        None,
        "a non-owner scope must not enter the live scoped group through its anchor",
    );
    assert_eq!(
        table.start_recranpose_at_anchor(anchor, first_scope_id),
        Some(group),
        "the current owner should still enter the group through its anchor",
    );
}

#[test]
fn gapped_group_preserves_child_value_slots_for_restore() {
    let mut table = SlotTable::new();

    table.reset();
    let root = table.start(1);
    let group = table.start(2);
    let value_slot = table.use_value_slot(|| String::from("persist"));
    table.end();
    table.end();
    table.flush_anchors_if_dirty();

    let group_end = match table.slots[group] {
        Slot::Group { len, .. } => group + super::unpack_slot_len(len),
        _ => panic!("group should remain a group"),
    };

    assert!(
        table.mark_range_as_gaps(group, group_end, Some(root)),
        "group should become hidden behind a gap",
    );
    assert!(
        matches!(table.slots[value_slot], Slot::Value { .. }),
        "group child value slots must survive while the parent group is hidden",
    );

    table.reset();
    let reused_root = table.start(1);
    assert_eq!(reused_root, root);
    let restored_group = table.start(2);
    assert_eq!(restored_group, group);
    let restored_slot = table.use_value_slot(|| String::from("new"));
    assert_eq!(restored_slot, value_slot);
    assert_eq!(table.read_value::<String>(restored_slot), "persist");
}

#[test]
fn compaction_drops_preserved_child_values_inside_hidden_gap_group() {
    let mut table = SlotTable::new();

    table.reset();
    let root = table.start(1);
    let group = table.start(2);
    let _value_slot = table.use_value_slot(|| String::from("persist"));
    table.end();
    table.end();

    let group_end = match table.slots[group] {
        Slot::Group { len, .. } => group + super::unpack_slot_len(len),
        _ => panic!("group should remain a group"),
    };

    assert!(
        table.mark_range_as_gaps(group, group_end, Some(root)),
        "group should become hidden behind a gap",
    );
    table.compact();

    assert_eq!(
        table.slots.len(),
        1,
        "compaction should remove the entire hidden group block, including preserved child values",
    );
    match table.slots[0] {
        Slot::Group { len, .. } => assert_eq!(super::unpack_slot_len(len), 1),
        _ => panic!("root group should remain after compaction"),
    }
}

#[test]
fn restored_hidden_group_can_be_gapped_again_before_compaction() {
    let mut table = SlotTable::new();

    table.reset();
    let root = table.start(1);
    let group = table.start(2);
    let value_slot = table.use_value_slot(|| String::from("persist"));
    table.end();
    table.end();
    table.flush_anchors_if_dirty();

    let group_anchor = table.group_anchor_at(group);
    let group_end = match table.slots[group] {
        Slot::Group { len, .. } => group + super::unpack_slot_len(len),
        _ => panic!("group should remain a group"),
    };

    assert!(
        table.mark_range_as_gaps(group, group_end, Some(root)),
        "group should become hidden behind a gap",
    );

    table.reset();
    let reused_root = table.start(1);
    assert_eq!(reused_root, root);
    let restored_group = table.start(2);
    assert_eq!(restored_group, group);
    let restored_slot = table.use_value_slot(|| String::from("new"));
    assert_eq!(restored_slot, value_slot);
    assert_eq!(table.read_value::<String>(restored_slot), "persist");
    table.end();
    table.end();
    table.flush_anchors_if_dirty();

    assert_eq!(
        table.resolve_anchor(group_anchor),
        Some(group),
        "restored group should continue to own the original anchor before being hidden again",
    );

    let restored_group_end = match table.slots[group] {
        Slot::Group { len, .. } => group + super::unpack_slot_len(len),
        _ => panic!("restored group should remain live before re-gapping"),
    };

    assert!(
        table.mark_range_as_gaps(group, restored_group_end, Some(root)),
        "restored group should be able to hide behind a gap again immediately",
    );

    table.compact();

    assert_eq!(
        table.slots.len(),
        1,
        "compaction should remove the re-gapped restored group and its child values",
    );
    assert_eq!(
        table.resolve_anchor(group_anchor),
        None,
        "re-gapped group anchor should be released once compaction removes the hidden group",
    );
}

#[test]
fn orphaned_node_state_reports_preserved_gap_nodes() {
    let mut table = SlotTable::new();
    let anchor = AnchorId(1);
    table.slots.push(Slot::Gap { anchor });
    table.register_anchor(anchor, 0);
    table.set_gap_metadata(
        anchor,
        GapMetadata {
            preserved_node: Some((42, 7)),
            ..GapMetadata::default()
        },
    );

    assert_eq!(
        table.orphaned_node_state(OrphanedNode::new(42, 7, anchor)),
        NodeSlotState::PreservedGap
    );
}

#[test]
fn orphaned_node_state_reports_restored_active_node() {
    let mut table = SlotTable::new();
    let anchor = AnchorId(1);
    table.slots.push(Slot::Gap { anchor });
    table.register_anchor(anchor, 0);
    table.set_gap_metadata(
        anchor,
        GapMetadata {
            preserved_node: Some((42, 7)),
            ..GapMetadata::default()
        },
    );
    table.set_slot_tracked(
        0,
        Slot::Node {
            anchor,
            id: 42,
            gen: 7,
        },
    );

    assert_eq!(
        table.orphaned_node_state(OrphanedNode::new(42, 7, anchor)),
        NodeSlotState::Active
    );
}

#[test]
fn orphaned_node_state_reports_missing_after_anchor_is_gone() {
    let mut table = SlotTable::new();
    let anchor = AnchorId(1);
    table.slots.push(Slot::Node {
        anchor,
        id: 42,
        gen: 7,
    });
    table.register_anchor(anchor, 0);
    table.free_anchor(anchor);

    assert_eq!(
        table.orphaned_node_state(OrphanedNode::new(42, 7, anchor)),
        NodeSlotState::Missing
    );
}

#[test]
fn exact_live_group_reuses_even_when_parent_restore_restricts_gap_search() {
    let mut table = SlotTable::new();

    table.reset();
    let root = table.start(1);
    let child = table.start(2);
    table.set_group_scope(child, 2);
    let grandchild = table.start(3);
    table.set_group_scope(grandchild, 3);
    table.end();
    table.end();
    table.end();
    table.flush_anchors_if_dirty();

    table.reset();
    table.cursor = child;
    table.group_stack.push(GroupFrame {
        key: 1,
        start: root,
        end: grandchild + 1,
        child_reuse: ChildReusePolicy::ParentRestoredFromGap,
        fresh_body: true,
        gap_boundary_key: 1,
    });

    let reused_child = table.start(2);
    assert_eq!(
        reused_child, child,
        "restored parents must still reuse an exact live child group at the cursor",
    );
    assert_eq!(
        table.get_group_scope(reused_child),
        Some(2),
        "exact live group reuse must preserve the child scope",
    );

    let reused_grandchild = table.start(3);
    assert_eq!(
        reused_grandchild, grandchild,
        "once the exact live child group is reused, its live descendants should stay aligned",
    );
    assert_eq!(table.get_group_scope(reused_grandchild), Some(3));
}

#[test]
fn exact_live_node_slot_reuses_even_when_parent_restore_restricts_other_live_slots() {
    let mut table = SlotTable::new();

    table.reset();
    let root = table.start(1);
    let node_slot = table.cursor;
    table.record_node(42, 7);
    table.end();
    table.flush_anchors_if_dirty();

    table.reset();
    table.cursor = node_slot;
    table.group_stack.push(GroupFrame {
        key: 1,
        start: root,
        end: node_slot + 1,
        child_reuse: ChildReusePolicy::ParentRestoredFromGap,
        fresh_body: true,
        gap_boundary_key: 1,
    });

    assert_eq!(
        table.peek_node(),
        Some((42, 7)),
        "restored parents must still expose an exact live node slot at the cursor",
    );

    table.record_node(42, 7);

    assert_eq!(
        table.cursor,
        node_slot + 1,
        "recording the same node should reuse the existing slot instead of forcing a gap",
    );
    match table.slots.get(node_slot) {
        Some(Slot::Node { id, gen, .. }) => assert_eq!((*id, *gen), (42, 7)),
        _ => panic!("node slot should stay live after exact reuse"),
    }
}

#[test]
fn exact_live_value_slot_reuses_when_parent_is_restored_from_gap() {
    let mut table = SlotTable::new();

    table.reset();
    let root = table.start(1);
    let value_slot = table.use_value_slot(|| String::from("persist"));
    table.end();
    table.flush_anchors_if_dirty();

    table.reset();
    table.cursor = value_slot;
    table.group_stack.push(GroupFrame {
        key: 1,
        start: root,
        end: value_slot + 1,
        child_reuse: ChildReusePolicy::ParentRestoredFromGap,
        fresh_body: true,
        gap_boundary_key: 1,
    });

    let reused_slot = table.use_value_slot(|| String::from("fresh"));
    assert_eq!(
        reused_slot, value_slot,
        "restored parents must reuse an exact live value slot at the cursor",
    );
    assert_eq!(
        table.read_value::<String>(reused_slot),
        "persist",
        "restored parents must keep their remembered value instead of recreating it",
    );
}

#[test]
fn begin_group_clears_stale_scope_identity() {
    let runtime = crate::runtime::TestRuntime::new();
    let mut table = SlotTable::new();

    table.reset();
    let scoped = table.begin_scoped_group(1, || RecomposeScope::new(runtime.handle()));
    let group = scoped.group.0;
    let anchor = scoped.anchor;
    let scope_id = scoped.scope.id();
    table.end();
    table.flush_anchors_if_dirty();

    table.reset();
    let reused = table.begin_group(1);
    assert_eq!(reused.group.0, group);
    table.end();
    table.flush_anchors_if_dirty();

    assert_eq!(
        table.get_group_scope(group),
        None,
        "switching the same keyed group to unscoped must clear the stored scope id",
    );
    assert_eq!(
        table.start_recranpose_at_anchor(anchor, scope_id),
        None,
        "stale scope owners must not re-enter a group that no longer has a scoped layout",
    );
}

#[test]
fn begin_scoped_group_inserts_scope_slot_without_overwriting_body() {
    let runtime = crate::runtime::TestRuntime::new();
    let mut table = SlotTable::new();

    table.reset();
    let root = table.start(1);
    let group = table.begin_group(2).group.0;
    let body_slot = table.use_value_slot(|| String::from("body"));
    table.end();
    table.end();
    table.flush_anchors_if_dirty();

    table.reset();
    let reused_root = table.start(1);
    assert_eq!(reused_root, root);
    let scoped = table.begin_scoped_group(2, || RecomposeScope::new(runtime.handle()));
    assert_eq!(scoped.group.0, group);
    assert_eq!(
        table.get_group_scope(group),
        Some(scoped.scope.id()),
        "scoped transition should record the new scope owner",
    );

    let reused_body_slot = table.use_value_slot(|| String::from("new-body"));
    assert_eq!(
        reused_body_slot,
        body_slot + 1,
        "adding a scope slot must shift the first body slot instead of overwriting it",
    );
    assert_eq!(
        table.read_value::<String>(reused_body_slot),
        "body",
        "scoped transition must preserve the original first body value",
    );
}

#[test]
fn keyed_group_matching_scans_past_sixteen_siblings() {
    let mut table = SlotTable::new();
    let root_key = crate::hash_key(&"root");
    let target_key = crate::hash_key(&"target");

    table.reset();
    let root = table.start(root_key);
    table.set_group_scope(root, 1);

    let mut target_group = None;
    for index in 0..24 {
        let key = if index == 23 {
            target_key
        } else {
            crate::hash_key(&format!("sibling-{index}"))
        };
        let group = table.start(key);
        table.set_group_scope(group, index + 10);
        table.use_value_slot(|| index);
        table.end();
        if key == target_key {
            target_group = Some(group);
        }
    }

    table.end();
    table.flush_anchors_if_dirty();

    let root_end = match table.slots[root] {
        Slot::Group { len, .. } => root + super::unpack_slot_len(len),
        _ => panic!("root should remain a group"),
    };

    let first_child = root + 1;
    table.cursor = first_child;
    table.group_stack.push(GroupFrame {
        key: root_key,
        start: root,
        end: root_end,
        child_reuse: ChildReusePolicy::Normal,
        fresh_body: false,
        gap_boundary_key: root_key,
    });

    let relocated = table.start(target_key);
    let original_target_group = target_group.expect("target sibling should exist");
    assert!(
        original_target_group > first_child + 16,
        "test setup must place the keyed sibling past the old search budget",
    );
    assert_eq!(relocated, first_child);
    assert_eq!(
        table.get_group_scope(relocated),
        Some(33),
        "stable keyed matching must search the full sibling set instead of stopping after sixteen entries",
    );
}

#[test]
fn marking_scope_slot_as_gap_deactivates_scope() {
    let runtime = crate::runtime::TestRuntime::new();
    let mut table = SlotTable::new();

    table.reset();
    let scoped = table.begin_scoped_group(1, || RecomposeScope::new(runtime.handle()));
    let group = scoped.group.0;
    table.end();

    assert!(scoped.scope.is_active(), "scope should start active");

    table.mark_range_as_gaps(group, group + 2, None);

    assert!(
        !scoped.scope.is_active(),
        "scope stored in a slot must deactivate once that slot is converted into a gap"
    );
}
