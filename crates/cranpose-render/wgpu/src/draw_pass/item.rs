use cranpose_ui_graphics::Rect;

use super::ResolvedComposite;
use crate::scene::{RRectShadowDraw, RunDraw, TextDraw};

pub(super) enum Item<'a> {
    /// A run, the window of its records the segment draws, and its draw
    /// bounds: see [`op_bounds::run_draw_bounds`].
    Run(&'a RunDraw, Option<std::ops::Range<u32>>, Rect),
    Image(usize),
    /// A text with its logical geometry: see
    /// [`crate::render::text_logical_geometry_for_draw`].
    Text(&'a TextDraw, (Rect, f32)),
    RRectShadow(&'a RRectShadowDraw),
    Composite(&'a ResolvedComposite),
}
