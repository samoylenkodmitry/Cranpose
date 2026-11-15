//! The frames of a composer's parent stack: a node whose children the pass
//! composes, and how they join it.

use crate::{ChildList, NodeId};

pub(crate) struct ParentFrame {
    pub(crate) id: NodeId,
    pub(crate) previous: ChildList,
    pub(crate) new_children: ChildList,
    pub(crate) attach_mode: ParentAttachMode,
    pub(crate) synthetic_root: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum InitialParentFrame {
    SyntheticRoot,
    RealParent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ParentAttachMode {
    ImmediateAppend,
    DeferredSync,
}

// Opens the parent frame of a pass's top-level nodes under `root`.
pub(super) fn push_root_frame(
    parent_stack: &mut Vec<ParentFrame>,
    root: Option<NodeId>,
    initial_parent_frame: InitialParentFrame,
) {
    if let Some(root_id) = root {
        parent_stack.push(ParentFrame {
            id: root_id,
            previous: ChildList::new(),
            new_children: ChildList::new(),
            attach_mode: ParentAttachMode::DeferredSync,
            synthetic_root: matches!(initial_parent_frame, InitialParentFrame::SyntheticRoot),
        });
    }
}
