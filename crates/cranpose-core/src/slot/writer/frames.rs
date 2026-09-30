use super::{key_state::FrameKeyState, siblings::SiblingIndex};
use crate::AnchorId;

#[derive(Default)]
pub(in crate::slot) struct RootFrame {
    pub(in crate::slot) next_child_index: usize,
    pub(in crate::slot) detach_remaining_children: bool,
    pub(in crate::slot) keys: FrameKeyState,
    pub(in crate::slot) sibling_index: Option<SiblingIndex>,
}

#[derive(Default)]
pub(in crate::slot) struct GroupStack {
    frames: Vec<GroupFrame>,
    active_len: usize,
}

impl GroupStack {
    pub(in crate::slot) fn push(&mut self) -> &mut GroupFrame {
        if self.active_len == self.frames.len() {
            self.frames.push(GroupFrame::default());
        }
        let frame = &mut self.frames[self.active_len];
        self.active_len += 1;
        frame
    }

    pub(in crate::slot) fn pop(&mut self) -> Option<AnchorId> {
        self.active_len = self.active_len.checked_sub(1)?;
        let frame = &mut self.frames[self.active_len];
        let anchor = frame.group_anchor;
        frame.reset(AnchorId::INVALID, 0, 0, 0);
        Some(anchor)
    }

    #[cfg(test)]
    pub(in crate::slot) fn capacity(&self) -> usize {
        self.frames.capacity()
    }
}

impl std::ops::Deref for GroupStack {
    type Target = [GroupFrame];

    fn deref(&self) -> &Self::Target {
        &self.frames[..self.active_len]
    }
}

impl std::ops::DerefMut for GroupStack {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.frames[..self.active_len]
    }
}

pub(in crate::slot) struct GroupFrame {
    pub(in crate::slot) group_anchor: AnchorId,
    pub(in crate::slot) next_child_index: usize,
    pub(in crate::slot) payload_cursor: usize,
    pub(in crate::slot) old_payload_len: usize,
    pub(in crate::slot) node_cursor: usize,
    pub(in crate::slot) old_node_len: usize,
    pub(in crate::slot) keys: FrameKeyState,
    pub(in crate::slot) sibling_index: Option<SiblingIndex>,
    pub(in crate::slot) body_finished: bool,
    pub(in crate::slot) was_skipped: bool,
    pub(in crate::slot) fold_watermark: usize,
}

impl RootFrame {
    pub(in crate::slot) fn reset(&mut self, detach_remaining_children: bool) {
        self.next_child_index = 0;
        self.detach_remaining_children = detach_remaining_children;
        self.keys.clear();
        self.sibling_index = None;
    }
}

impl Default for GroupFrame {
    fn default() -> Self {
        Self {
            group_anchor: AnchorId::INVALID,
            next_child_index: 0,
            payload_cursor: 0,
            old_payload_len: 0,
            node_cursor: 0,
            old_node_len: 0,
            keys: FrameKeyState::default(),
            sibling_index: None,
            body_finished: false,
            was_skipped: false,
            fold_watermark: 0,
        }
    }
}

impl GroupFrame {
    pub(in crate::slot) fn reset(
        &mut self,
        anchor: AnchorId,
        next_child_index: usize,
        old_payload_len: usize,
        old_node_len: usize,
    ) {
        self.group_anchor = anchor;
        self.next_child_index = next_child_index;
        self.payload_cursor = 0;
        self.old_payload_len = old_payload_len;
        self.node_cursor = 0;
        self.old_node_len = old_node_len;
        self.keys.clear();
        self.sibling_index = None;
        self.body_finished = false;
        self.was_skipped = false;
        self.fold_watermark = 0;
    }

    pub(in crate::slot) fn mark_body_finished(&mut self) -> bool {
        if self.body_finished {
            return false;
        }
        self.body_finished = true;
        true
    }

    pub(in crate::slot) fn advance_payload_cursor(&mut self) {
        self.payload_cursor += 1;
    }

    pub(in crate::slot) fn advance_node_cursor(&mut self) {
        self.node_cursor += 1;
    }

    pub(in crate::slot) fn skip_to_existing_group_end(
        &mut self,
        group_index: usize,
        subtree_len: usize,
    ) {
        self.next_child_index = group_index + subtree_len;
        self.payload_cursor = self.old_payload_len;
        self.node_cursor = self.old_node_len;
        self.was_skipped = true;
    }
}

#[cfg(test)]
#[path = "tests/frames_tests.rs"]
mod tests;
