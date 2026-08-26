pub(crate) mod layout_node;

pub use layout_node::{IntrinsicKind, LayoutNode, LayoutState};
pub(crate) use layout_node::{
    LayoutNodeCacheHandles, allocate_virtual_node_id, is_virtual_node, register_layout_node,
};
