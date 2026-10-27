//! Where each layout modifier of a node's chain put its content, which is
//! where the draw modifiers and the text after it draw.

use std::rc::{Rc, Weak};

use cranpose_ui_graphics::{EdgeInsets, Point, Rect, Size};

use crate::widgets::nodes::LayoutParts;

/// Where one draw modifier or text of a chain draws: in the rect of its
/// coordinator, the first layout modifier after it (its own when it lays out
/// too, as text does), or the node's content when none follows. Coordinators
/// are as Compose has them: for the k-th layout modifier of the chain, the
/// rect relative to the node that modifier was placed at and sized to, and
/// after the last one the rect of the node's own content. Until layout has
/// placed that coordinator it draws inside the padding declared before it,
/// which is also all a node laid out without coordinators knows.
#[derive(Clone, Default)]
pub(crate) struct CoordinatorRect {
    // The parts of the node whose layout places the coordinators, held weak:
    // slices a renderer or a dispatcher keeps after the node left must not
    // keep its layout alive. Empty for a chain no layout places.
    parts: Weak<LayoutParts>,
    ordinal: usize,
    padding: EdgeInsets,
}

impl CoordinatorRect {
    pub(crate) fn new(
        parts: Option<&Rc<LayoutParts>>,
        ordinal: usize,
        padding: EdgeInsets,
    ) -> Self {
        Self {
            parts: parts.map_or_else(Weak::new, Rc::downgrade),
            ordinal,
            padding,
        }
    }

    fn placed_rect(&self, node_size: Size) -> Option<Rect> {
        self.parts
            .upgrade()?
            .placed_coordinator_rect(self.ordinal, node_size)
    }

    /// The rect in a node of `node_size`.
    pub(crate) fn rect(&self, node_size: Size) -> Rect {
        self.placed_rect(node_size)
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
        self.placed_rect(Size::default()).map_or(
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
#[path = "tests/coordinator_rect_tests.rs"]
mod tests;
