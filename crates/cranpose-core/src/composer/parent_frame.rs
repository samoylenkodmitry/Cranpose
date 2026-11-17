//! The frames of a composer's parent stack: a node whose children the pass
//! composes, and how they join it.

use smallvec::SmallVec;

use crate::{ChildList, NodeId};

pub(crate) struct ParentFrame {
    pub(crate) id: NodeId,
    // The node's children when the frame opened, in the list the node filled:
    // the frame copies none of them.
    pub(crate) previous: SmallVec<[NodeId; 8]>,
    pub(crate) new_children: ChildList,
    pub(crate) attach_mode: ParentAttachMode,
    pub(crate) synthetic_root: bool,
    // The node the pass made last under the frame: a push of it as a parent
    // opens a made frame.
    pub(crate) last_made: Option<NodeId>,
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
    // The frame's node is one the pass made, as is each child so far: each
    // child mounts under it as it is made, and the children join it when
    // the frame closes, with no command and no sync of its list.
    Made,
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
            previous: SmallVec::new(),
            new_children: ChildList::new(),
            attach_mode: ParentAttachMode::DeferredSync,
            synthetic_root: matches!(initial_parent_frame, InitialParentFrame::SyntheticRoot),
            last_made: None,
        });
    }
}
