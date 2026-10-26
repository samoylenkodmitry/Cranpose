use std::collections::HashMap;

use smallvec::SmallVec;

use crate::{
    Applier, ChildList, Command, DeferredChildCleanup, DeferredChildCleanupQueue, DirtyBubble,
    NodeError, NodeId, SMALL_CHILD_SYNC_LINEAR_THRESHOLD, build_child_positions,
    insert_child_into_diff_state, move_child_in_diff_state,
};
pub(crate) fn attach_child_at(
    applier: &mut dyn Applier,
    parent_id: NodeId,
    child_id: NodeId,
    insert_index: Option<usize>,
    bubble: DirtyBubble,
) {
    if insert_child_with_reparenting(applier, parent_id, child_id) {
        if let Some(target) = insert_index {
            move_attached_child_left_to(applier, parent_id, child_id, target);
        }
        bubble.apply(applier, parent_id);
    } else if let Ok(child) = applier.get_mut(child_id) {
        let dirty_bubble = DirtyBubble {
            layout: child.layout_dirty(),
            measure: child.needs_measure() || child.descendant_needs_measure(),
            semantics: false,
        };
        dirty_bubble.apply_to_ancestors(applier, child_id);
    }
}

fn move_attached_child_left_to(
    applier: &mut dyn Applier,
    parent_id: NodeId,
    child_id: NodeId,
    target: usize,
) {
    let Ok(parent_node) = applier.get_mut(parent_id) else {
        return;
    };
    let Some(current_index) = parent_node.owned_child_index(child_id) else {
        return;
    };
    if target < current_index {
        parent_node.move_child(current_index, target);
        note_structural_move(parent_id, current_index, target);
    }
}

pub(crate) fn insert_child_with_reparenting(
    applier: &mut dyn Applier,
    parent_id: NodeId,
    child_id: NodeId,
) -> bool {
    if parent_id == child_id {
        debug_assert_ne!(
            parent_id, child_id,
            "a node cannot be attached as its own child"
        );
        return false;
    }

    let old_parent = applier
        .get_mut(child_id)
        .ok()
        .and_then(|node| node.parent());
    if old_parent != Some(parent_id) {
        applier.forget_bubble_passes();
    }
    if let Some(old_parent_id) = old_parent
        && old_parent_id != parent_id
    {
        let removed = applier
            .get_mut(old_parent_id)
            .is_ok_and(|old_parent_node| old_parent_node.remove_child(child_id));
        if let Ok(child_node) = applier.get_mut(child_id) {
            child_node.on_removed_from_parent();
        }
        if removed {
            DirtyBubble::LAYOUT_AND_MEASURE.apply(applier, old_parent_id);
            note_structural("reparent-detach", old_parent_id, child_id);
            applier.record_structural_change(old_parent_id);
        }
    }

    let inserted = applier
        .get_mut(parent_id)
        .is_ok_and(|parent_node| parent_node.insert_child(child_id));
    if inserted {
        note_structural("attach", parent_id, child_id);
        applier.record_structural_change(parent_id);
    }
    if let Ok(child_node) = applier.get_mut(child_id) {
        child_node.on_attached_to_parent(parent_id);
    }
    inserted
}

pub(crate) fn apply_remove_child(
    applier: &mut dyn Applier,
    parent_id: NodeId,
    child_id: NodeId,
    deferred_cleanup: &mut DeferredChildCleanupQueue,
) -> Result<(), NodeError> {
    detach_child_from_parent(applier, parent_id, child_id)?;

    let generation = applier.node_generation(child_id);
    let removed_from_parent = if let Ok(node) = applier.get_mut(child_id) {
        node.parent().is_none()
    } else {
        return Ok(());
    };
    deferred_cleanup.push(DeferredChildCleanup {
        child_id,
        generation,
        removed_from_parent,
    });
    Ok(())
}

pub(crate) fn detach_child_from_parent(
    applier: &mut dyn Applier,
    parent_id: NodeId,
    child_id: NodeId,
) -> Result<(), NodeError> {
    let removed = applier
        .get_mut(parent_id)
        .is_ok_and(|parent_node| parent_node.remove_child(child_id));
    if removed {
        DirtyBubble::LAYOUT_AND_MEASURE.apply(applier, parent_id);
        note_structural("detach", parent_id, child_id);
        applier.record_structural_change(parent_id);
    }

    if let Ok(node) = applier.get_mut(child_id) {
        match node.parent() {
            Some(existing_parent_id) if existing_parent_id == parent_id => {
                node.on_removed_from_parent();
                applier.forget_bubble_passes();
            }
            None => {}
            Some(_) => return Ok(()),
        }
    } else {
        return Ok(());
    }

    Ok(())
}

pub(crate) fn cleanup_detached_child(
    applier: &mut dyn Applier,
    cleanup: DeferredChildCleanup,
) -> Result<(), NodeError> {
    if applier.node_generation(cleanup.child_id) != cleanup.generation {
        return Ok(());
    }

    let parent_id = match applier.get_mut(cleanup.child_id) {
        Ok(node) => node.parent(),
        Err(NodeError::Missing { .. }) => return Ok(()),
        Err(err) => return Err(err),
    };
    if parent_id.is_some() {
        return Ok(());
    }

    if let Ok(node) = applier.get_mut(cleanup.child_id) {
        if !cleanup.removed_from_parent {
            node.on_removed_from_parent();
        }
        node.unmount();
    }
    match applier.remove(cleanup.child_id) {
        Ok(()) | Err(NodeError::Missing { .. }) => Ok(()),
        Err(err) => Err(err),
    }
}

pub(crate) fn remove_child_and_cleanup_now(
    applier: &mut dyn Applier,
    parent_id: NodeId,
    child_id: NodeId,
) -> Result<(), NodeError> {
    let mut deferred_cleanup = DeferredChildCleanupQueue::default();
    apply_remove_child(applier, parent_id, child_id, &mut deferred_cleanup)?;
    deferred_cleanup.flush(applier)
}

fn collect_current_children(applier: &mut dyn Applier, parent_id: NodeId) -> ChildList {
    let mut scratch = SmallVec::<[NodeId; 8]>::new();
    if let Ok(node) = applier.get_mut(parent_id) {
        node.collect_children_into(&mut scratch);
    }
    let mut current = ChildList::new();
    current.extend(scratch);
    current
}

fn sync_children(
    applier: &mut dyn Applier,
    parent_id: NodeId,
    expected_children: &[NodeId],
    deferred_cleanup: &mut DeferredChildCleanupQueue,
) -> Result<(), NodeError> {
    let mut current = collect_current_children(applier, parent_id);
    let children_changed = current.as_slice() != expected_children;

    if children_changed {
        if current.len().max(expected_children.len()) <= SMALL_CHILD_SYNC_LINEAR_THRESHOLD {
            sync_children_small(
                applier,
                parent_id,
                &mut current,
                expected_children,
                deferred_cleanup,
            )?;
        } else {
            let mut target_positions: HashMap<NodeId, usize> = HashMap::default();
            target_positions.reserve(expected_children.len());
            for (index, &child) in expected_children.iter().enumerate() {
                target_positions.insert(child, index);
            }

            for index in (0..current.len()).rev() {
                let child = current[index];
                if !target_positions.contains_key(&child) {
                    current.remove(index);
                    apply_remove_child(applier, parent_id, child, deferred_cleanup)?;
                }
            }

            let mut current_positions = build_child_positions(&current);
            for (target_index, &child) in expected_children.iter().enumerate() {
                if let Some(current_index) = current_positions.get(&child).copied() {
                    if current_index != target_index {
                        let from_index = current_index;
                        let to_index = move_child_in_diff_state(
                            &mut current,
                            &mut current_positions,
                            from_index,
                            target_index,
                        );
                        Command::MoveChild {
                            parent_id,
                            from_index,
                            to_index,
                            bubble: DirtyBubble::LAYOUT_AND_MEASURE,
                        }
                        .apply(applier)?;
                    }
                } else {
                    let insert_index = target_index.min(current.len());
                    let appended_index = current.len();
                    insert_child_into_diff_state(
                        &mut current,
                        &mut current_positions,
                        insert_index,
                        child,
                    );
                    Command::InsertChild {
                        parent_id,
                        child_id: child,
                        appended_index,
                        insert_index,
                        bubble: DirtyBubble::LAYOUT_AND_MEASURE,
                    }
                    .apply(applier)?;
                }
            }
        }
    }

    reconcile_children(applier, parent_id, expected_children, !children_changed)
}

fn sync_children_small(
    applier: &mut dyn Applier,
    parent_id: NodeId,
    current: &mut ChildList,
    expected_children: &[NodeId],
    deferred_cleanup: &mut DeferredChildCleanupQueue,
) -> Result<(), NodeError> {
    for index in (0..current.len()).rev() {
        let child = current[index];
        if !expected_children.contains(&child) {
            current.remove(index);
            apply_remove_child(applier, parent_id, child, deferred_cleanup)?;
        }
    }

    for (target_index, &child) in expected_children.iter().enumerate() {
        if let Some(current_index) = current
            .iter()
            .position(|&current_child| current_child == child)
        {
            if current_index != target_index {
                let child = current.remove(current_index);
                let to_index = target_index.min(current.len());
                current.insert(to_index, child);
                Command::MoveChild {
                    parent_id,
                    from_index: current_index,
                    to_index,
                    bubble: DirtyBubble::LAYOUT_AND_MEASURE,
                }
                .apply(applier)?;
            }
        } else {
            let insert_index = target_index.min(current.len());
            let appended_index = current.len();
            current.insert(insert_index, child);
            Command::InsertChild {
                parent_id,
                child_id: child,
                appended_index,
                insert_index,
                bubble: DirtyBubble::LAYOUT_AND_MEASURE,
            }
            .apply(applier)?;
        }
    }

    Ok(())
}

/// Attaches the expected children that do not name the parent. An unchanged
/// child list is the parent's own: a child that tracks its parent already
/// names it, and one that does not track it cannot be repaired, so only a
/// changed list visits its children.
fn reconcile_children(
    applier: &mut dyn Applier,
    parent_id: NodeId,
    expected_children: &[NodeId],
    children_unchanged: bool,
) -> Result<(), NodeError> {
    if children_unchanged {
        settle_kept_children(applier, parent_id);
        return Ok(());
    }
    let mut repaired = false;
    for &child_id in expected_children {
        let needs_attach = applier
            .get_mut(child_id)
            .is_ok_and(|node| node.parent() != Some(parent_id));
        if needs_attach {
            insert_child_with_reparenting(applier, parent_id, child_id);
            repaired = true;
        }
    }
    if repaired {
        DirtyBubble::LAYOUT_AND_MEASURE.apply(applier, parent_id);
    }
    Ok(())
}

/// Makes `parent_id`'s children `expected_children`, or, for `None`, finishes
/// a parent that kept the ones it held.
pub(crate) fn sync_or_settle_children(
    applier: &mut dyn Applier,
    parent_id: NodeId,
    expected_children: Option<&[NodeId]>,
    deferred_cleanup: &mut DeferredChildCleanupQueue,
) -> Result<(), NodeError> {
    if applier
        .get_mut(parent_id)
        .is_ok_and(|parent| parent.keeps_own_children())
    {
        return Ok(());
    }
    match expected_children {
        Some(expected_children) => {
            sync_children(applier, parent_id, expected_children, deferred_cleanup)
        }
        None => {
            settle_kept_children(applier, parent_id);
            Ok(())
        }
    }
}

/// Finishes a parent whose children stayed as they were: its own layout dirt,
/// which it may have taken while it composed, reaches its ancestors.
fn settle_kept_children(applier: &mut dyn Applier, parent_id: NodeId) {
    debug_assert!(
        collect_current_children(applier, parent_id)
            .iter()
            .all(|&child_id| applier
                .get_mut(child_id)
                .map_or(true, |node| node.parent().is_none_or(|p| p == parent_id))),
        "the children a parent holds must name no other parent"
    );
    if applier
        .get_mut(parent_id)
        .is_ok_and(|node| node.layout_dirty())
    {
        DirtyBubble::LAYOUT.apply_to_ancestors(applier, parent_id);
    }
}

#[expect(clippy::print_stderr, reason = "CRANPOSE_STRUCTURAL_DIAG asks for it")]
fn note_structural(reason: &str, parent_id: NodeId, child_id: NodeId) {
    if crate::env_flag!("CRANPOSE_STRUCTURAL_DIAG") {
        eprintln!("[structural] {reason} parent={parent_id} child={child_id}");
    }
}

#[expect(clippy::print_stderr, reason = "CRANPOSE_STRUCTURAL_DIAG asks for it")]
pub(crate) fn note_structural_move(parent_id: NodeId, from_index: usize, to_index: usize) {
    if crate::env_flag!("CRANPOSE_STRUCTURAL_DIAG") {
        eprintln!("[structural] move parent={parent_id} from={from_index} to={to_index}");
    }
}
