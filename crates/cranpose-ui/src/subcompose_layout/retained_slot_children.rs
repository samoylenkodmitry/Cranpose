// Keeping a slot's retained composition for a measure pass without composing
// it: a clean slot, and an exact slot whose children a lazy item knows.

use cranpose_core::{Composer, NodeId, SlotId, SubcomposeState};
use cranpose_foundation::lazy::SmallNodeVec;

use super::{SubcomposeMeasureScopeImpl, record_clean_slot_skip};
use crate::widgets::nodes::LayoutNode;

pub(crate) enum RetainedSlotChildren {
    Clean,
    Remeasure {
        children: SmallNodeVec,
        children_match: bool,
    },
}

/// Activates `slot_id`'s retained composition in `state` for this pass and
/// hangs its slot hosts under `root_id`; returns the hosts.
fn activate_slot_hosts<'s>(
    state: &'s mut SubcomposeState,
    composer: &Composer,
    root_id: NodeId,
    slot_id: SlotId,
) -> Option<&'s [NodeId]> {
    let virtual_node_ids = state.activate_current_active_slot(slot_id)?;
    for &virtual_node_id in virtual_node_ids {
        let _ = composer.with_node_mut::<LayoutNode, _>(virtual_node_id, |node| {
            node.set_parent(root_id);
        });
    }
    Some(virtual_node_ids)
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "retained slot activation in its own module"
)]
impl SubcomposeMeasureScopeImpl<'_> {
    /// Whether `slot_id`'s retained composition is clean and its nodes are
    /// in place, so the pass may keep it without composing it.
    fn clean_slot_ready(&mut self, slot_id: SlotId) -> bool {
        !self.state.has_pending_precompositions(slot_id)
            && self
                .state
                .slot_content_generation_current(slot_id, self.owner_chain_deactivation_epoch())
            && self.ensure_pending_commands_applied()
    }

    /// Keeps `slot_id`'s composition for this pass as
    /// [`SubcomposeMeasureScope::subcompose`] does with a clean slot of
    /// `key`, without collecting its children. False when the slot needs a
    /// compose, which `subcompose` runs.
    pub(crate) fn keep_clean_retained_slot<K: PartialEq + 'static>(
        &mut self,
        slot_id: SlotId,
        key: &K,
    ) -> bool {
        let kept = self.state.retained_capture_key_matches(slot_id, key)
            && self.clean_slot_ready(slot_id)
            && activate_slot_hosts(&mut *self.state, &self.composer, self.root_id, slot_id)
                .is_some();
        if kept {
            record_clean_slot_skip();
        }
        kept
    }

    // `SubcomposeMeasureScope::subcompose`, with the slot's root nodes in an
    // inline list.
    pub(crate) fn subcompose_roots<K, Content>(
        &mut self,
        slot_id: SlotId,
        key: K,
        content: Content,
    ) -> SmallNodeVec
    where
        K: PartialEq + 'static,
        Content: FnMut() + 'static,
    {
        if self.state.retained_capture_key_matches(slot_id, &key)
            && let Some(children) = self.activate_clean_retained_slot(slot_id)
        {
            #[cfg(debug_assertions)]
            self.shadow_verify_clean_slot(slot_id, content);
            return children;
        }
        self.state.store_retained_capture_key(slot_id, key);
        self.perform_subcompose(slot_id, content)
    }

    fn activate_clean_retained_slot(&mut self, slot_id: SlotId) -> Option<SmallNodeVec> {
        if !self.clean_slot_ready(slot_id) {
            return None;
        }
        let virtual_node_ids =
            activate_slot_hosts(&mut *self.state, &self.composer, self.root_id, slot_id)?;

        let mut children = SmallNodeVec::new();
        for virtual_node_id in virtual_node_ids {
            children.extend(self.composer.get_node_children(*virtual_node_id));
        }
        record_clean_slot_skip();

        #[cfg(debug_assertions)]
        {
            self.shadow_stash = Some((virtual_node_ids.to_vec(), children.clone()));
        }

        Some(children)
    }

    #[cfg(debug_assertions)]
    fn shadow_verify_clean_slot<Content>(&mut self, slot_id: SlotId, content: Content)
    where
        Content: FnMut() + 'static,
    {
        let Some((virtual_node_ids, skipped_children)) = self.shadow_stash.take() else {
            return;
        };
        if virtual_node_ids.len() != 1 {
            return;
        }
        let composed = self.compose_into_slot(slot_id, virtual_node_ids[0], content);
        assert!(
            composed == skipped_children,
            "clean-slot skip diverged for slot {slot_id:?}: recomposing produced root \
             children {composed:?} but the retained slot held {skipped_children:?}. The slot content \
             read a value that changed between measure passes without any \
             invalidation path — make that value reactive state, or part of \
             the subcompose capture key",
        );
    }

    pub(crate) fn activate_exact_retained_slot_with_known_children(
        &mut self,
        slot_id: SlotId,
        known_children: &[NodeId],
    ) -> Option<RetainedSlotChildren> {
        if !self.ensure_pending_commands_applied() {
            return None;
        }

        let activation = self.state.activate_exact_slot(slot_id)?;
        for &virtual_node_id in activation.nodes {
            if activation.was_recycled {
                let _ = self
                    .composer
                    .with_node_mut::<LayoutNode, _>(virtual_node_id, |node| {
                        node.set_parent(self.root_id);
                    });
            }
        }

        let mut expected = known_children.iter().copied();
        let mut children_match = true;
        let mut needs_relayout = false;
        for &virtual_node_id in activation.nodes {
            self.composer
                .visit_node_children(virtual_node_id, |child, child_needs_relayout| {
                    children_match &= expected.next() == Some(child);
                    needs_relayout |= child_needs_relayout;
                });
        }
        children_match &= expected.next().is_none();
        if children_match && !needs_relayout {
            return Some(RetainedSlotChildren::Clean);
        }

        let mut children = SmallNodeVec::new();
        for &virtual_node_id in activation.nodes {
            children.extend(self.composer.get_node_children(virtual_node_id));
        }
        Some(RetainedSlotChildren::Remeasure {
            children,
            children_match,
        })
    }
}
