//! What a tile is drawn on: the window, through the paint context of a view,
//! or a bitmap that a layer keeps for content that never changes.

use floem::{
    context::PaintCx,
    kurbo::{Circle, Point, RoundedRect},
    peniko::Color,
    text::TextLayout,
};
use floem_renderer::Renderer as _;

pub trait Surface {
    /// A rectangle `(left, top, right, bottom)` with rounded corners.
    fn fill_rounded(&mut self, rect: (f64, f64, f64, f64), radius: f64, color: Color);

    fn fill_circle(&mut self, center: Point, radius: f64, color: Color);

    /// `layout` with its top left corner at `at`.
    fn text(&mut self, layout: &TextLayout, at: Point);
}

impl Surface for PaintCx<'_> {
    fn fill_rounded(
        &mut self,
        (left, top, right, bottom): (f64, f64, f64, f64),
        radius: f64,
        color: Color,
    ) {
        self.fill(
            &RoundedRect::new(left, top, right, bottom, radius),
            color,
            0.0,
        );
    }

    fn fill_circle(&mut self, center: Point, radius: f64, color: Color) {
        self.fill(&Circle::new(center, radius), color, 0.0);
    }

    fn text(&mut self, layout: &TextLayout, at: Point) {
        self.draw_text(layout, at);
    }
}
