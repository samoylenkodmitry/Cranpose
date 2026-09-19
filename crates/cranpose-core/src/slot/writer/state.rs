use super::{
    super::{DetachedSubtree, SlotPassMode, SlotTable},
    frames::{GroupFrame, RootFrame},
};
use crate::{AnchorId, collections::map::HashMap};

pub(in crate::slot) struct BranchFoldEntry {
    key: crate::Key,
    prev_fold: Option<crate::Key>,
    live: bool,
}

#[derive(Default)]
pub(crate) struct SlotWriteSessionState {
    pub(in crate::slot) root: RootFrame,
    pub(in crate::slot) group_stack: Vec<GroupFrame>,
    frame_pool: Vec<GroupFrame>,
    payload_location_refreshes: HashMap<AnchorId, usize>,
    rejected_restore_subtrees: Vec<DetachedSubtree>,
    branch_fold_entries: Vec<BranchFoldEntry>,
    branch_fold: Option<crate::Key>,
    dead_branch_folds: usize,
    pub(in crate::slot) removed_payload_count: usize,
    pub(in crate::slot) removed_node_count: usize,
    pub(in crate::slot) removed_group_count: usize,
    pub(crate) request_compaction: bool,
    pub(crate) request_anchor_storage_compaction: bool,
    pub(crate) request_payload_storage_compaction: bool,
}

impl SlotWriteSessionState {
    pub(in crate::slot) const COMPACT_PAYLOAD_THRESHOLD: usize = 16 * 1024;
    const COMPACT_NODE_THRESHOLD: usize = 16 * 1024;
    const COMPACT_GROUP_THRESHOLD: usize = 32 * 1024;

    pub(crate) fn reset_for_pass(&mut self, mode: SlotPassMode) {
        self.root.reset(matches!(mode, SlotPassMode::Compose));
        while let Some(frame) = self.group_stack.pop() {
            self.recycle_group_frame(frame);
        }
        self.payload_location_refreshes.clear();
        if !self.rejected_restore_subtrees.is_empty() {
            log::error!(
                "slot writer reset discarded {} rejected restore subtrees that were not finalized",
                self.rejected_restore_subtrees.len()
            );
            self.rejected_restore_subtrees.clear();
        }
        if !self.branch_fold_entries.is_empty() {
            log::error!(
                "slot writer reset discarded {} branch folds whose guards never closed",
                self.branch_fold_entries.len()
            );
            self.branch_fold_entries.clear();
            self.branch_fold = None;
            self.dead_branch_folds = 0;
        }
        self.removed_payload_count = 0;
        self.removed_node_count = 0;
        self.removed_group_count = 0;
        self.request_compaction = false;
        self.request_anchor_storage_compaction = false;
        self.request_payload_storage_compaction = false;
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

    #[cfg(any(test, debug_assertions))]
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

    pub(crate) fn push_branch_fold(&mut self, key: crate::Key) -> usize {
        let prev_fold = self.branch_fold;
        if let Some(fold) = prev_fold {
            self.branch_fold = Some((fold ^ key).wrapping_mul(0x0000_0100_0000_01b3));
        }
        self.branch_fold_entries.push(BranchFoldEntry {
            key,
            prev_fold,
            live: true,
        });
        self.branch_fold_entries.len() - 1
    }

    fn fold_watermark(&self) -> usize {
        self.group_stack
            .last()
            .map(|frame| frame.fold_watermark)
            .unwrap_or(0)
            .min(self.branch_fold_entries.len())
    }

    pub(crate) fn close_branch_fold(&mut self, token: usize) {
        if token + 1 == self.branch_fold_entries.len() {
            let entry = self
                .branch_fold_entries
                .pop()
                .expect("length checked above");
            self.branch_fold = if self.dead_branch_folds == 0 {
                entry.prev_fold
            } else {
                None
            };
            while self
                .branch_fold_entries
                .last()
                .is_some_and(|entry| !entry.live)
            {
                self.branch_fold_entries.pop();
                self.dead_branch_folds -= 1;
                self.branch_fold = None;
            }
            return;
        }
        let Some(entry) = self.branch_fold_entries.get_mut(token) else {
            log::error!(
                "branch fold {token} closed past depth {}",
                self.branch_fold_entries.len()
            );
            return;
        };
        if entry.live {
            entry.live = false;
            self.dead_branch_folds += 1;
        }
        self.branch_fold = None;
    }

    pub(in crate::slot) fn branch_fold(&mut self) -> crate::Key {
        if let Some(fold) = self.branch_fold {
            return fold;
        }
        let watermark = self.fold_watermark();
        let mut fold = super::super::BRANCH_PATH_ROOT;
        for entry in &self.branch_fold_entries[watermark..] {
            if entry.live {
                fold ^= entry.key;
                fold = fold.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        self.branch_fold = Some(fold);
        fold
    }

    pub(in crate::slot) fn mix_branch_fold(&mut self, key: crate::Key) -> crate::Key {
        let fold = self.branch_fold();
        if fold == super::super::BRANCH_PATH_ROOT {
            return key;
        }
        (fold ^ key).wrapping_mul(0x0000_0100_0000_01b3)
    }

    pub(in crate::slot) fn current_parent_anchor(&self) -> AnchorId {
        self.group_stack
            .last()
            .map(|frame| frame.group_anchor)
            .unwrap_or(AnchorId::INVALID)
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
        next_child_index: usize,
        old_payload_len: usize,
        old_node_len: usize,
    ) {
        let mut frame = self.frame_pool.pop().unwrap_or_default();
        frame.reset(anchor, next_child_index, old_payload_len, old_node_len);
        frame.fold_watermark = self.branch_fold_entries.len();
        self.group_stack.push(frame);
        self.branch_fold = None;
    }

    pub(in crate::slot) fn recycle_group_frame(&mut self, mut frame: GroupFrame) {
        frame.reset_for_pool();
        self.frame_pool.push(frame);
        self.branch_fold = None;
    }
}

#[cfg(test)]
#[path = "tests/state_tests.rs"]
mod tests;
