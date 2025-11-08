use compose_core::{Node, NodeId};

mod button_node;
mod layout_node;
mod spacer_node;
mod text_node;

pub use button_node::ButtonNode;
pub use layout_node::bubble_dirty_flags;
pub use layout_node::IntrinsicKind;
pub use layout_node::LayoutNode;
pub(crate) use layout_node::LayoutNodeCacheHandles;
pub use spacer_node::SpacerNode;
pub use text_node::TextNode;

pub fn compose_node<N: Node + 'static>(init: impl FnOnce() -> N) -> NodeId {
    compose_core::with_current_composer(|composer| composer.emit_node(init))
}
