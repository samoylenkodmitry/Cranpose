//! The allocations of the layers a scene update replaces, which the layers
//! built in their place take instead of allocating their own. A graph update
//! frees whatever it did not take when it ends, so nothing is held between
//! frames; a walk that lowers nodes alone keeps as many as it asked for,
//! which the next walk takes.

use std::rc::Rc;

use cranpose_ui_graphics::CommandRecording;

use crate::{
    graph::{
        DrawCommandId, InlineNode, LayerNode, PrimitiveEntry, PrimitiveNode, RenderNode,
        TextPrimitiveNode,
    },
    scene_builder::Redraw,
};

#[derive(Default)]
#[expect(
    clippy::vec_box,
    reason = "the boxes are the allocations the pool keeps for reuse"
)]
pub(crate) struct Pool {
    layers: Vec<Box<LayerNode>>,
    inlines: Vec<Box<InlineNode>>,
    /// Text boxes still holding the text they drew, which the text that
    /// takes one replaces.
    texts: Vec<Box<TextPrimitiveNode>>,
    lists: Vec<Vec<RenderNode>>,
    // How many boxes and child lists of each kind were asked for since the
    // pool was last released or trimmed.
    asked: Asked,
}

#[derive(Default)]
struct Asked {
    layers: usize,
    inlines: usize,
    texts: usize,
    lists: usize,
}

impl Pool {
    fn recycle_children(&mut self, layer: &mut LayerNode) {
        self.recycle_nodes(&mut layer.children);
    }

    fn recycle_nodes(&mut self, nodes: &mut Vec<RenderNode>) {
        while let Some(child) = nodes.pop() {
            self.recycle_node(child);
        }
    }

    fn recycle_node(&mut self, node: RenderNode) {
        self.recycle_keeping(node, &mut |_, _| {});
    }

    // Moves the boxes of `node` and of the nodes beneath it into the pool,
    // and hands the recording of each draw run among them to `keep`.
    fn recycle_keeping(
        &mut self,
        node: RenderNode,
        keep: &mut impl FnMut(DrawCommandId, Rc<CommandRecording>),
    ) {
        match node {
            RenderNode::Layer(mut layer) => {
                while let Some(child) = layer.children.pop() {
                    self.recycle_keeping(child, keep);
                }
                layer.hit_test = None;
                self.layers.push(layer);
            }
            RenderNode::Inline(mut inline) => {
                while let Some(child) = inline.content.pop() {
                    self.recycle_keeping(child, keep);
                }
                self.inlines.push(inline);
            }
            RenderNode::Primitive(PrimitiveEntry {
                node: PrimitiveNode::Text(text),
                ..
            }) => self.texts.push(text),
            RenderNode::DrawRun(run) => {
                if let Some(id) = run.command {
                    keep(id, run.recording);
                }
            }
            RenderNode::Primitive(_) => {}
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
        children
            .drain(end..)
            .for_each(|child| pool.recycle_node(child));
        children
            .drain(..first_layer)
            .for_each(|child| pool.recycle_node(child));
    });
}

// Moves the boxes of `node`, a child of a layer, into the pool.
pub(crate) fn recycle_node(node: RenderNode) {
    crate::thread_state::with(|state| state.layer_pool.borrow_mut().recycle_node(node));
}

/// Moves the boxes of `node`, a node
/// [`lower_node_alone`](crate::scene_builder::lower_node_alone) gave whose
/// items a renderer took, into the pool the next lowered nodes take theirs
/// from, and keeps the recordings of its draws for their next draws.
pub fn recycle_lowered(node: RenderNode) {
    recycle_lowered_drawing(node, Redraw::AsBefore);
}

// Moves the boxes of `node`, the kept head of a node that lowers again in
// this update, into the pool, and keeps the recordings of its draws for its
// new draws.
pub(crate) fn recycle_to_redraw(node: RenderNode) {
    recycle_lowered_drawing(node, Redraw::Now);
}

// Moves the boxes of `node` into the pool and keeps the recordings of its
// draws, whose commands draw again as `redraw` says.
fn recycle_lowered_drawing(node: RenderNode, redraw: Redraw) {
    crate::thread_state::with(|state| {
        let mut redrawn = state.redrawn.borrow_mut();
        state
            .layer_pool
            .borrow_mut()
            .recycle_keeping(node, &mut |id, recording| {
                redrawn.keep(id, recording, redraw);
            });
    });
}

// Moves the text boxes of `inline`'s content into the pool, before the
// content is written again in its emptied list.
pub(crate) fn recycle_content(inline: &mut InlineNode) {
    crate::thread_state::with(|state| {
        state
            .layer_pool
            .borrow_mut()
            .recycle_nodes(&mut inline.content);
    });
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
    let mut list = crate::thread_state::with(|state| {
        let mut pool = state.layer_pool.borrow_mut();
        pool.asked.lists += 1;
        pool.lists.pop()
    })
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
    crate::thread_state::with(|state| {
        let mut pool = state.layer_pool.borrow_mut();
        pool.asked.layers += 1;
        pool.layers.pop()
    })
    .unwrap_or_default()
}

// An inline node's box with an empty content list, recycled when the pool
// has one.
pub(crate) fn inline_box() -> Option<Box<InlineNode>> {
    crate::thread_state::with(|state| {
        let mut pool = state.layer_pool.borrow_mut();
        pool.asked.inlines += 1;
        pool.inlines.pop()
    })
}

/// `text` in a recycled box when the pool has one.
pub(crate) fn boxed_text(text: TextPrimitiveNode) -> Box<TextPrimitiveNode> {
    let recycled = crate::thread_state::with(|state| {
        let mut pool = state.layer_pool.borrow_mut();
        pool.asked.texts += 1;
        pool.texts.pop()
    });
    match recycled {
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
        pool.inlines.clear();
        pool.texts.clear();
        pool.lists.clear();
        pool.asked = Asked::default();
    });
}

/// Keeps as many boxes and child lists of each kind as the walk that ends
/// asked for. The next walk asks for about as many. The rest goes. A kept
/// text box holds the text it drew until a text takes the box.
pub(crate) fn keep_for_next_walk() {
    crate::thread_state::with(|state| {
        let mut pool = state.layer_pool.borrow_mut();
        let asked = std::mem::take(&mut pool.asked);
        pool.layers.truncate(asked.layers);
        pool.inlines.truncate(asked.inlines);
        pool.texts.truncate(asked.texts);
        pool.lists.truncate(asked.lists);
    });
}
