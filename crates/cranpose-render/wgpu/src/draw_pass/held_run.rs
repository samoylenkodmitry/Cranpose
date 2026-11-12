use cranpose_ui_graphics::Rect;

use super::SegmentRun;
use crate::scene::RunDraw;

// A run of shapes held behind the glyphs and images it overlaps: the item
// and how its segment places it, kept until the held draws are drawn and the
// run joins the chunk after them.
pub(super) struct HeldRun<'s> {
    pub(super) draw: &'s RunDraw,
    pub(super) window: Option<std::ops::Range<u32>>,
    pub(super) bounds: Rect,
    pub(super) run: SegmentRun<'s, 's>,
}

impl<'s> HeldRun<'s> {
    pub(super) fn new(
        (draw, window, bounds): (&'s RunDraw, Option<std::ops::Range<u32>>, Rect),
        run: &SegmentRun<'s, 's>,
    ) -> Self {
        Self {
            draw,
            window,
            bounds,
            run: run.clone(),
        }
    }
}
