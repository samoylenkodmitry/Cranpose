use std::rc::Rc;

use super::{
    super::{BranchFolds, DetachedSubtree, SlotPassMode, SlotTable, branch_folds::mix_fold},
    frames::{GroupFrameStack, RootFrame},
};
use crate::{AnchorId, collections::map::HashMap};

pub(crate) struct SlotWriteSessionState {
    pub(in crate::slot) root: RootFrame,
    pub(in crate::slot) group_stack: GroupFrameStack,
    payload_location_refreshes: HashMap<AnchorId, usize>,
    rejected_restore_subtrees: Vec<DetachedSubtree>,
    /// The composer's open branch guards; the pass's root-level groups fold
    /// the entries from `root_fold_watermark`.
    folds: Rc<BranchFolds>,
    root_fold_watermark: usize,
    pub(in crate::slot) removed_payload_count: usize,
    pub(in crate::slot) removed_node_count: usize,
    pub(in crate::slot) removed_group_count: usize,
    pub(crate) request_compaction: bool,
    pub(crate) request_anchor_storage_compaction: bool,
    pub(crate) request_payload_storage_compaction: bool,
}

#[cfg(test)]
impl Default for SlotWriteSessionState {
    fn default() -> Self {
        Self::new(Rc::default())
    }
}

impl SlotWriteSessionState {
    pub(crate) fn new(folds: Rc<BranchFolds>) -> Self {
        Self {
            root: RootFrame::default(),
            group_stack: GroupFrameStack::default(),
            payload_location_refreshes: HashMap::default(),
            rejected_restore_subtrees: Vec::new(),
            folds,
            root_fold_watermark: 0,
            removed_payload_count: 0,
            removed_node_count: 0,
            removed_group_count: 0,
            request_compaction: false,
            request_anchor_storage_compaction: false,
            request_payload_storage_compaction: false,
        }
    }

    pub(in crate::slot) const COMPACT_PAYLOAD_THRESHOLD: usize = 16 * 1024;
    const COMPACT_NODE_THRESHOLD: usize = 16 * 1024;
    const COMPACT_GROUP_THRESHOLD: usize = 32 * 1024;

    pub(crate) fn reset_for_pass(&mut self, mode: SlotPassMode) {
        self.root.reset(matches!(mode, SlotPassMode::Compose));
        self.group_stack.clear();
        self.payload_location_refreshes.clear();
        if !self.rejected_restore_subtrees.is_empty() {
            log::error!(
                "slot writer reset discarded {} rejected restore subtrees that were not finalized",
                self.rejected_restore_subtrees.len()
            );
            self.rejected_restore_subtrees.clear();
        }
        self.root_fold_watermark = self.folds.len();
        self.folds.watermark_moved();
        self.removed_payload_count = 0;
        self.removed_node_count = 0;
        self.removed_group_count = 0;
        self.request_compaction = false;
        self.request_anchor_storage_compaction = false;
        self.request_payload_storage_compaction = false;
    }

    pub(crate) fn removed_nothing(&self) -> bool {
        self.removed_payload_count == 0
            && self.removed_node_count == 0
            && self.removed_group_count == 0
    }

    pub(in crate::slot) fn note_removed_payloads(&mut self, count: usize) {
        self.removed_payload_count += count;
        self.update_compaction_hint();
    }

    pub(in crate::slot) fn note_payload_location_refresh(&mut self, owner: AnchorId, start: usize) {
        self.payload_location_refreshes
            .entry(owner)
            .and_modify(|current| *current = (*current).min(start))
            .or_insert(start);
    }

    pub(in crate::slot) fn has_pending_payload_location_refreshes(&self) -> bool {
        !self.payload_location_refreshes.is_empty()
    }

    #[cfg(any(test, debug_assertions))]
    pub(in crate::slot) fn pending_payload_location_refresh_count(&self) -> usize {
        self.payload_location_refreshes.len()
    }

    pub(crate) fn flush_payload_location_refreshes(&mut self, table: &mut SlotTable) {
        table.flush_payload_location_refreshes(self);
        #[cfg(any(test, debug_assertions))]
        self.debug_assert_no_pending_payload_location_refreshes("writer payload refresh flush");
    }

    #[cfg(test)]
    pub(in crate::slot) fn pending_payload_location_refresh_start(
        &self,
        owner: AnchorId,
    ) -> Option<usize> {
        self.payload_location_refreshes.get(&owner).copied()
    }

    #[cfg(any(test, debug_assertions))]
    pub(in crate::slot) fn debug_assert_no_pending_payload_location_refreshes(
        &self,
        operation: &'static str,
    ) {
        debug_assert!(
            !self.has_pending_payload_location_refreshes(),
            "payload location refreshes must be flushed after {operation}"
        );
    }

    pub(in crate::slot) fn drain_payload_location_refreshes(
        &mut self,
    ) -> impl Iterator<Item = (AnchorId, usize)> + '_ {
        self.payload_location_refreshes.drain()
    }

    pub(in crate::slot) fn note_removed_nodes(&mut self, count: usize) {
        self.removed_node_count += count;
        self.update_compaction_hint();
    }

    pub(in crate::slot) fn note_detached_subtrees(&mut self, subtrees: &[DetachedSubtree]) {
        self.removed_group_count += subtrees
            .iter()
            .map(DetachedSubtree::group_count)
            .sum::<usize>();
        self.removed_payload_count += subtrees
            .iter()
            .map(DetachedSubtree::payload_count)
            .sum::<usize>();
        self.removed_node_count += subtrees
            .iter()
            .map(DetachedSubtree::node_count)
            .sum::<usize>();
        self.update_compaction_hint();
    }

    pub(in crate::slot) fn queue_rejected_restore_subtree(&mut self, subtree: DetachedSubtree) {
        self.rejected_restore_subtrees.push(subtree);
    }

    pub(in crate::slot) fn drain_rejected_restore_subtrees(&mut self) -> Vec<DetachedSubtree> {
        std::mem::take(&mut self.rejected_restore_subtrees)
    }

    fn update_compaction_hint(&mut self) {
        let payload_pressure = self.removed_payload_count >= Self::COMPACT_PAYLOAD_THRESHOLD;
        let node_pressure = self.removed_node_count >= Self::COMPACT_NODE_THRESHOLD;
        let group_pressure = self.removed_group_count >= Self::COMPACT_GROUP_THRESHOLD;
        self.request_compaction |= payload_pressure || node_pressure || group_pressure;
        self.request_anchor_storage_compaction |= group_pressure;
        self.request_payload_storage_compaction |= payload_pressure;
    }

    fn fold_watermark(&self) -> usize {
        self.group_stack
            .last()
            .map_or(self.root_fold_watermark, |frame| frame.fold_watermark)
    }

    pub(in crate::slot) fn branch_fold(&self) -> crate::Key {
        self.folds.fold(self.fold_watermark())
    }

    pub(in crate::slot) fn mix_branch_fold(&self, key: crate::Key) -> crate::Key {
        let fold = self.branch_fold();
        if fold == super::super::BRANCH_PATH_ROOT {
            return key;
        }
        mix_fold(fold, key)
    }

    pub(in crate::slot) fn current_parent_anchor(&self) -> AnchorId {
        self.group_stack
            .last()
            .map_or(AnchorId::INVALID, |frame| frame.group_anchor)
    }

    pub(in crate::slot) fn current_child_cursor(&self) -> usize {
        if let Some(frame) = self.group_stack.last() {
            frame.next_child_index
        } else {
            self.root.next_child_index
        }
    }

    pub(in crate::slot) fn advance_parent_after_child(&mut self, subtree_end: usize) {
        if let Some(parent) = self.group_stack.last_mut() {
            parent.next_child_index = subtree_end;
        } else {
            self.root.next_child_index = subtree_end;
        }
    }

    pub(in crate::slot) fn push_group_frame(
        &mut self,
        anchor: AnchorId,
        group_index: usize,
        old_payload_len: usize,
        old_node_len: usize,
    ) {
        let fold_watermark = self.folds.begin_group();
        let frame = self.group_stack.push();
        frame.reset(anchor, group_index, old_payload_len, old_node_len);
        frame.fold_watermark = fold_watermark;
    }

    /// Closes the top frame and returns its group's anchor and index.
    pub(in crate::slot) fn pop_group_frame(&mut self) -> Option<(AnchorId, usize)> {
        let frame = self.group_stack.pop()?;
        let group = (frame.group_anchor, frame.group_index);
        self.folds.watermark_moved();
        Some(group)
    }
}

#[cfg(test)]
#[path = "tests/state_tests.rs"]
mod tests;
