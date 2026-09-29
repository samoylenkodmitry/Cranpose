//! The allocations of the layers a scene update replaces, which the layers
//! built in their place take instead of allocating their own. The update
//! frees whatever it did not take when it ends, so nothing is held between
//! frames.

use std::cell::RefCell;

use crate::graph::{LayerNode, PrimitiveEntry, PrimitiveNode, RenderNode, TextPrimitiveNode};

#[derive(Default)]
#[expect(
    clippy::vec_box,
    reason = "the boxes are the allocations the pool keeps for reuse"
)]
struct Pool {
    layers: Vec<Box<LayerNode>>,
    /// Text boxes still holding the text they drew, which the text that
    /// takes one replaces.
    texts: Vec<Box<TextPrimitiveNode>>,
    lists: Vec<Vec<RenderNode>>,
}

thread_local! {
    static POOL: RefCell<Pool> = RefCell::new(Pool::default());
}

impl Pool {
    fn recycle_list(&mut self, mut list: Vec<RenderNode>) {
        for child in list.drain(..) {
            match child {
                RenderNode::Layer(mut layer) => {
                    let replaced = std::mem::take(layer.as_mut());
                    self.recycle_list(replaced.children);
                    self.layers.push(layer);
                }
                RenderNode::Primitive(PrimitiveEntry {
                    node: PrimitiveNode::Text(text),
                    ..
                }) => self.texts.push(text),
                RenderNode::Primitive(_) | RenderNode::DrawRun(_) => {}
            }
        }
        if list.capacity() > 0 {
            self.lists.push(list);
        }
    }
}

/// Moves `layer`'s children, and every layer box and child list beneath
/// them, into the pool, before the layer is built again.
pub(crate) fn recycle_children(layer: &mut LayerNode) {
    let children = std::mem::take(&mut layer.children);
    POOL.with(|pool| pool.borrow_mut().recycle_list(children));
}

/// Moves an emptied child list into the pool.
pub(crate) fn recycle_list(list: Vec<RenderNode>) {
    debug_assert!(list.is_empty(), "only an emptied list is recycled");
    if list.capacity() > 0 {
        POOL.with(|pool| pool.borrow_mut().lists.push(list));
    }
}

/// A child list with room for `capacity` children, recycled when the pool
/// has one.
pub(crate) fn child_list(capacity: usize) -> Vec<RenderNode> {
    let mut list = POOL
        .with(|pool| pool.borrow_mut().lists.pop())
        .unwrap_or_default();
    list.reserve(capacity);
    list
}

/// `layer` in a recycled box when the pool has one.
pub(crate) fn boxed(layer: LayerNode) -> Box<LayerNode> {
    refill(POOL.with(|pool| pool.borrow_mut().layers.pop()), layer)
}

/// `text` in a recycled box when the pool has one.
pub(crate) fn boxed_text(text: TextPrimitiveNode) -> Box<TextPrimitiveNode> {
    refill(POOL.with(|pool| pool.borrow_mut().texts.pop()), text)
}

fn refill<T>(recycled: Option<Box<T>>, value: T) -> Box<T> {
    match recycled {
        Some(mut recycled) => {
            *recycled = value;
            recycled
        }
        None => Box::new(value),
    }
}

/// Frees what the update did not take.
pub(crate) fn release() {
    POOL.with(|pool| {
        let mut pool = pool.borrow_mut();
        pool.layers.clear();
        pool.texts.clear();
        pool.lists.clear();
    });
}
