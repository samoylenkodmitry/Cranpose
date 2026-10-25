use super::{key_state::FrameKeyState, siblings::SiblingIndex};
use crate::AnchorId;

#[derive(Default)]
pub(in crate::slot) struct RootFrame {
    pub(in crate::slot) next_child_index: usize,
    pub(in crate::slot) detach_remaining_children: bool,
    pub(in crate::slot) keys: FrameKeyState,
    pub(in crate::slot) sibling_index: Option<SiblingIndex>,
}

pub(in crate::slot) struct GroupFrame {
    pub(in crate::slot) group_anchor: AnchorId,
    /// Where the group sat when the frame opened. Groups move, enter or
    /// leave only at the writer's cursor, which lies past every open group,
    /// so this stays the group's index while the frame is open.
    pub(in crate::slot) group_index: usize,
    pub(in crate::slot) next_child_index: usize,
    /// How many direct child groups the frame's body has ended so far.
    pub(in crate::slot) ended_children: u32,
    pub(in crate::slot) payload_cursor: usize,
    pub(in crate::slot) old_payload_len: usize,
    pub(in crate::slot) node_cursor: usize,
    pub(in crate::slot) old_node_len: usize,
    pub(in crate::slot) keys: FrameKeyState,
    pub(in crate::slot) sibling_index: Option<SiblingIndex>,
    pub(in crate::slot) body_finished: bool,
    skip_end: Option<usize>,
    pub(in crate::slot) fold_watermark: usize,
}

#[derive(Default)]
pub(in crate::slot) struct GroupFrameStack {
    frames: Vec<GroupFrame>,
    depth: usize,
}

// The frames of a stack between passes, which the hosts on a thread share: a
// frame takes about 100 bytes on 32-bit targets, and a storage per host would
// hold as many frames for each item of a lazy list as for the root.
pub(crate) struct GroupFrameStorage(Vec<GroupFrame>);

// Passes of more hosts at once are rare; they take new storage.
const SPARE_GROUP_FRAME_STORAGES: usize = 2;

// A host whose passes need more frames is deep, such as an app's root, and
// keeps its own.
const SPARE_GROUP_FRAME_CAPACITY: usize = 64;

impl GroupFrameStack {
    // A stack that kept its frames, a deep host's or one of a pass that ended
    // early, keeps them.
    pub(in crate::slot) fn adopt_spare(&mut self) {
        if self.frames.capacity() > 0 {
            return;
        }
        if let Some(GroupFrameStorage(frames)) =
            crate::thread_state::try_runtimes(|state| state.spare_group_frames.borrow_mut().pop())
                .flatten()
        {
            self.frames = frames;
        }
    }

    pub(in crate::slot) fn recycle(&mut self) {
        self.depth = 0;
        let capacity = self.frames.capacity();
        if capacity == 0 || capacity > SPARE_GROUP_FRAME_CAPACITY {
            return;
        }
        let frames = std::mem::take(&mut self.frames);
        crate::thread_state::try_runtimes(|state| {
            let mut spare = state.spare_group_frames.borrow_mut();
            if spare.len() < SPARE_GROUP_FRAME_STORAGES {
                spare.push(GroupFrameStorage(frames));
            }
        });
    }

    pub(in crate::slot) fn push(&mut self) -> &mut GroupFrame {
        if self.depth == self.frames.len() {
            self.frames.push(GroupFrame::default());
        }
        let index = self.depth;
        self.depth += 1;
        &mut self.frames[index]
    }

    pub(in crate::slot) fn pop(&mut self) -> Option<&GroupFrame> {
        self.depth = self.depth.checked_sub(1)?;
        self.frames.get(self.depth)
    }

    pub(in crate::slot) fn last(&self) -> Option<&GroupFrame> {
        self.depth
            .checked_sub(1)
            .and_then(|index| self.frames.get(index))
    }

    pub(in crate::slot) fn last_mut(&mut self) -> Option<&mut GroupFrame> {
        self.depth
            .checked_sub(1)
            .and_then(|index| self.frames.get_mut(index))
    }

    #[cfg(any(test, debug_assertions))]
    pub(in crate::slot) fn len(&self) -> usize {
        self.depth
    }

    pub(in crate::slot) fn is_empty(&self) -> bool {
        self.depth == 0
    }

    #[cfg(any(test, debug_assertions))]
    pub(in crate::slot) fn iter(&self) -> impl Iterator<Item = &GroupFrame> {
        self.frames.iter().take(self.depth)
    }

    pub(in crate::slot) fn clear(&mut self) {
        self.depth = 0;
    }
}

impl std::ops::Index<usize> for GroupFrameStack {
    type Output = GroupFrame;

    fn index(&self, index: usize) -> &GroupFrame {
        &self.frames[..self.depth][index]
    }
}

impl std::ops::IndexMut<usize> for GroupFrameStack {
    fn index_mut(&mut self, index: usize) -> &mut GroupFrame {
        &mut self.frames[..self.depth][index]
    }
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
            group_index: 0,
            next_child_index: 0,
            ended_children: 0,
            payload_cursor: 0,
            old_payload_len: 0,
            node_cursor: 0,
            old_node_len: 0,
            keys: FrameKeyState::default(),
            sibling_index: None,
            body_finished: false,
            skip_end: None,
            fold_watermark: 0,
        }
    }
}

impl GroupFrame {
    pub(in crate::slot) fn reset(
        &mut self,
        anchor: AnchorId,
        group_index: usize,
        old_payload_len: usize,
        old_node_len: usize,
    ) {
        self.group_anchor = anchor;
        self.group_index = group_index;
        self.next_child_index = group_index + 1;
        self.ended_children = 0;
        self.payload_cursor = 0;
        self.old_payload_len = old_payload_len;
        self.node_cursor = 0;
        self.old_node_len = old_node_len;
        self.keys.clear();
        self.sibling_index = None;
        self.body_finished = false;
        self.skip_end = None;
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

    pub(in crate::slot) fn skip_to_existing_group_end(&mut self, end: usize) {
        self.next_child_index = end;
        self.payload_cursor = self.old_payload_len;
        self.node_cursor = self.old_node_len;
        self.skip_end = Some(end);
    }

    pub(in crate::slot) fn was_skipped(&self) -> bool {
        self.skip_end.is_some()
    }

    pub(in crate::slot) fn untouched_since_skip(&self) -> bool {
        self.skip_end == Some(self.next_child_index)
            && self.payload_cursor == self.old_payload_len
            && self.node_cursor == self.old_node_len
    }
}
