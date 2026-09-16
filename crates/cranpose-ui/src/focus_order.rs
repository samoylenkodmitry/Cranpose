use std::cell::RefCell;

use cranpose_core::NodeId;
use cranpose_ui_graphics::Rect;

use crate::layout::{LayoutBox, LayoutTree};

/// One focus target, with the bounds layout gave it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FocusEntry {
    pub node_id: NodeId,
    pub rect: Rect,
}

impl FocusEntry {
    pub fn center(&self) -> (f32, f32) {
        (
            self.rect.x + self.rect.width * 0.5,
            self.rect.y + self.rect.height * 0.5,
        )
    }
}

thread_local! {
    static FOCUS_ORDER: RefCell<Vec<FocusEntry>> = const { RefCell::new(Vec::new()) };
}

/// Replaces the focus order a later [`crate::FocusManager`] move reads. The
/// app shell publishes it after a layout pass.
pub fn set_focus_order(entries: Vec<FocusEntry>) {
    FOCUS_ORDER.with(|cell| *cell.borrow_mut() = entries);
}

/// Reads the published focus order.
pub fn with_focus_order<T>(reader: impl FnOnce(&[FocusEntry]) -> T) -> T {
    FOCUS_ORDER.with(|cell| reader(&cell.borrow()))
}

/// How many focus targets the last published order holds.
pub fn focus_order_len() -> usize {
    FOCUS_ORDER.with(|cell| cell.borrow().len())
}

/// Walks `tree` in the order the layout pass placed it and keeps the nodes
/// that registered a focus target and take space on screen.
pub fn collect_focus_order(tree: &LayoutTree) -> Vec<FocusEntry> {
    let mut entries = Vec::new();
    collect_from_box(tree.root(), &mut entries);
    entries
}

fn collect_from_box(layout_box: &LayoutBox, entries: &mut Vec<FocusEntry>) {
    if crate::focus_dispatch::has_focus_target(layout_box.node_id) && takes_space(layout_box.rect) {
        entries.push(FocusEntry {
            node_id: layout_box.node_id,
            rect: layout_box.rect,
        });
    }
    for child in &layout_box.children {
        collect_from_box(child, entries);
    }
}

fn takes_space(rect: Rect) -> bool {
    rect.width > 0.0
        && rect.height > 0.0
        && rect.x.is_finite()
        && rect.y.is_finite()
        && rect.width.is_finite()
        && rect.height.is_finite()
}
