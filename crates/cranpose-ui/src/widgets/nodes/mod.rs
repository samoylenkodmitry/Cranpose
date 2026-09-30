pub(crate) mod layout_node;

pub use layout_node::{IntrinsicKind, LayoutNode, LayoutState};
pub(crate) use layout_node::{
    LayoutNodeCacheHandles, allocate_virtual_node_id, begin_placement_pass, is_virtual_node,
    placement_pass_with_unplaced_nodes, register_layout_node,
};
