//! Where each layout modifier of a node's chain put its content, which is
//! where the draw modifiers and the text after it draw.

use std::{cell::RefCell, rc::Rc};

use cranpose_ui_graphics::{EdgeInsets, Point, Rect, Size};
use smallvec::SmallVec;

/// A layout node's coordinators, as Compose has them: for the k-th layout
/// modifier of its chain, the rect relative to the node that modifier was
/// placed at and sized to, and after the last one the rect of the node's own
/// content. Layout writes them after every measure of a chain with layout
/// modifiers; until it has, or for a node laid out without coordinators,
/// there are none.
#[derive(Default)]
pub(crate) struct CoordinatorGeometry {
    rects: RefCell<SmallVec<[Rect; 4]>>,
}

impl CoordinatorGeometry {
    /// Replaces the rects with those of a new measure, outermost first.
    /// Stores where each coordinator ended up, and says whether that moved
    /// any of them: a layer bounded by an inner coordinator changes when that
    /// coordinator does, though the node's own size may not.
    pub(crate) fn replace(&self, rects: impl IntoIterator<Item = Rect>) -> bool {
        let mut stored = self.rects.borrow_mut();
        let mut len = 0;
        let mut moved = false;
        for rect in rects {
            match stored.get_mut(len) {
                Some(slot) if *slot == rect => {}
                Some(slot) => {
                    *slot = rect;
                    moved = true;
                }
                None => {
                    stored.push(rect);
                    moved = true;
                }
            }
            len += 1;
        }
        if stored.len() != len {
            stored.truncate(len);
            moved = true;
        }
        moved
    }

    fn rect(&self, ordinal: usize) -> Option<Rect> {
        self.rects.borrow().get(ordinal).copied()
    }
}

/// Where one draw modifier or text of a chain draws: in the rect of its
/// coordinator, the first layout modifier after it (its own when it lays out
/// too, as text does), or the node's content when none follows. Until layout
/// has placed that coordinator it draws inside the padding declared before
/// it, which is also all a node laid out without coordinators knows.
#[derive(Clone, Default)]
pub(crate) struct CoordinatorRect {
    geometry: Rc<CoordinatorGeometry>,
    ordinal: usize,
    padding: EdgeInsets,
}

impl CoordinatorRect {
    pub(crate) fn new(
        geometry: &Rc<CoordinatorGeometry>,
        ordinal: usize,
        padding: EdgeInsets,
    ) -> Self {
        Self {
            geometry: Rc::clone(geometry),
            ordinal,
            padding,
        }
    }

    /// The rect in a node of `node_size`.
    pub(crate) fn rect(&self, node_size: Size) -> Rect {
        self.geometry
            .rect(self.ordinal)
            .unwrap_or_else(|| self.padding.inset_rect(node_size))
    }

    /// The insets from the edges of a node of `node_size` to [`Self::rect`].
    pub(crate) fn insets(&self, node_size: Size) -> EdgeInsets {
        let rect = self.rect(node_size);
        EdgeInsets {
            left: rect.x,
            top: rect.y,
            right: node_size.width - rect.x - rect.width,
            bottom: node_size.height - rect.y - rect.height,
        }
    }

    /// The rect's top-left corner in its node.
    pub(crate) fn origin(&self) -> Point {
        self.geometry.rect(self.ordinal).map_or(
            Point {
                x: self.padding.left,
                y: self.padding.top,
            },
            |rect| Point {
                x: rect.x,
                y: rect.y,
            },
        )
    }
}

#[cfg(test)]
#[path = "tests/coordinator_geometry_tests.rs"]
mod tests;
