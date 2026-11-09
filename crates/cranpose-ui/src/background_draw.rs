//! What a background modifier draws, kept as plain data.

use cranpose_ui_graphics::{CornerRadii, DrawScope, Rect, RoundedCornerShape, Size};

use crate::modifier::{Brush, Color, CoordinatorRect};

/// What a background modifier draws: its color over the rect of its
/// coordinator, rounded when a corner shape of the chain shapes it. It is
/// plain data, so a renderer can keep it as one shape.
#[derive(Clone)]
pub struct BackgroundDraw {
    color: Color,
    shape: Option<RoundedCornerShape>,
    coordinator: CoordinatorRect,
}

impl BackgroundDraw {
    pub(crate) fn new(
        color: Color,
        shape: Option<RoundedCornerShape>,
        coordinator: CoordinatorRect,
    ) -> Self {
        Self {
            color,
            shape,
            coordinator,
        }
    }

    /// The color the background fills with.
    pub fn color(&self) -> Color {
        self.color
    }

    /// The rect the background fills in a node of `size`, with the radii of
    /// its corners when a corner shape rounds it.
    pub fn shape_in(&self, size: Size) -> (Rect, Option<CornerRadii>) {
        let rect = self.coordinator.rect(size);
        let radii = self
            .shape
            .map(|shape| shape.resolve(rect.width, rect.height));
        (rect, radii)
    }

    /// Records the background into `scope`, whose size is the node's.
    pub fn draw(&self, scope: &mut dyn DrawScope) {
        let brush = Brush::solid(self.color);
        match self.shape_in(scope.size()) {
            (rect, Some(radii)) => scope.draw_round_rect_at(rect, brush, radii),
            (rect, None) => scope.draw_rect_at(rect, brush),
        }
    }

    // Whether `other` draws the same in every node.
    pub(crate) fn same_as(&self, other: &Self) -> bool {
        self.color == other.color
            && self.shape == other.shape
            && self.coordinator.same_as(&other.coordinator)
    }
}
