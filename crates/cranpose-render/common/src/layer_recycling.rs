//! The allocations of the layers a scene update replaces, which the layers
//! built in their place take instead of allocating their own. The update
//! frees whatever it did not take when it ends, so nothing is held between
//! frames.

use crate::graph::{LayerNode, PrimitiveEntry, PrimitiveNode, RenderNode, TextPrimitiveNode};

#[derive(Default)]
#[expect(
    clippy::vec_box,
    reason = "the boxes are the allocations the pool keeps for reuse"
)]
pub(crate) struct Pool {
    layers: Vec<Box<LayerNode>>,
    /// Text boxes still holding the text they drew, which the text that
    /// takes one replaces.
    texts: Vec<Box<TextPrimitiveNode>>,
    lists: Vec<Vec<RenderNode>>,
}

impl Pool {
    fn recycle_layer(&mut self, mut layer: Box<LayerNode>) {
        self.recycle_children(&mut layer);
        layer.hit_test = None;
        self.layers.push(layer);
    }

    fn recycle_children(&mut self, layer: &mut LayerNode) {
        while let Some(child) = layer.children.pop() {
            match child {
                RenderNode::Layer(layer) => self.recycle_layer(layer),
                RenderNode::Primitive(PrimitiveEntry {
                    node: PrimitiveNode::Text(text),
                    ..
                }) => self.texts.push(text),
                RenderNode::Primitive(_) | RenderNode::DrawRun(_) => {}
            }
        }
    }
}

/// Moves every layer box beneath `layer` into the pool, before the layer is
/// built again in its emptied child list.
pub(crate) fn recycle_children(layer: &mut LayerNode) {
    crate::thread_state::with(|state| state.layer_pool.borrow_mut().recycle_children(layer));
}

/// Takes `layer`'s own primitives out of its child list, keeping its child
/// layers in their order, and moves their text boxes and the layers of
/// their draw groups into the pool. A layer's own primitives come before
/// and after its child layers, so only those ends of the list are read.
pub(crate) fn recycle_primitives(layer: &mut LayerNode) {
    let children = &mut layer.children;
    let is_child_layer = |child: &RenderNode| child.draw_phase().is_none();
    let first_layer = children
        .iter()
        .position(is_child_layer)
        .unwrap_or(children.len());
    let end = children
        .iter()
        .rposition(is_child_layer)
        .map_or(first_layer, |last| last + 1);
    debug_assert!(
        children[first_layer..end].iter().all(is_child_layer),
        "a layer's own primitives never sit between its child layers"
    );
    // A container that draws nothing of its own has nothing to recycle.
    if first_layer == 0 && end == children.len() {
        return;
    }
    crate::thread_state::with(|state| {
        let mut pool = state.layer_pool.borrow_mut();
        let mut keep = |child| match child {
            RenderNode::Primitive(PrimitiveEntry {
                node: PrimitiveNode::Text(text),
                ..
            }) => pool.texts.push(text),
            RenderNode::Layer(group) => pool.recycle_layer(group),
            RenderNode::Primitive(_) | RenderNode::DrawRun(_) => {}
        };
        children.drain(end..).for_each(&mut keep);
        children.drain(..first_layer).for_each(keep);
    });
}

/// Moves `layer`'s box, and every layer box beneath it, into the pool.
pub(crate) fn recycle(layer: Box<LayerNode>) {
    crate::thread_state::with(|state| state.layer_pool.borrow_mut().recycle_layer(layer));
}

/// Moves an emptied child list into the pool.
pub(crate) fn recycle_list(list: Vec<RenderNode>) {
    debug_assert!(list.is_empty(), "only an emptied list is recycled");
    if list.capacity() > 0 {
        crate::thread_state::with(|state| state.layer_pool.borrow_mut().lists.push(list));
    }
}

/// A child list with room for `capacity` children, recycled when the pool
/// has one; see [`reserve_nodes`].
pub(crate) fn child_list(capacity: usize) -> Vec<RenderNode> {
    let mut list = crate::thread_state::with(|state| state.layer_pool.borrow_mut().lists.pop())
        .unwrap_or_default();
    reserve_nodes(&mut list, capacity);
    list
}

/// Makes room in `list` for `additional` more nodes: exactly that many in
/// a list that has none yet, as most layers are leaves of one draw run or
/// text where growth would make room for four; a list that has room grows
/// as usual.
pub(crate) fn reserve_nodes(list: &mut Vec<RenderNode>, additional: usize) {
    if list.capacity() == 0 {
        list.reserve_exact(additional);
    } else {
        list.reserve(additional);
    }
}

pub(crate) fn layer_box() -> Box<LayerNode> {
    crate::thread_state::with(|state| state.layer_pool.borrow_mut().layers.pop())
        .unwrap_or_default()
}

/// `text` in a recycled box when the pool has one.
pub(crate) fn boxed_text(text: TextPrimitiveNode) -> Box<TextPrimitiveNode> {
    match crate::thread_state::with(|state| state.layer_pool.borrow_mut().texts.pop()) {
        Some(mut recycled) => {
            *recycled = text;
            recycled
        }
        None => Box::new(text),
    }
}

/// Frees what the update did not take.
pub(crate) fn release() {
    crate::thread_state::with(|state| {
        let mut pool = state.layer_pool.borrow_mut();
        pool.layers.clear();
        pool.texts.clear();
        pool.lists.clear();
    });
}
