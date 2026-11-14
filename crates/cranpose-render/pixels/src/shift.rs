// A move of items by whole device pixels: what a scroll moves a frame of
// items by.

use cranpose_ui_graphics::{Point, Rect, whole};

// The most pixels a shift moves by along an axis: a float holds whole numbers
// up to far past it, so a shift adds to a position exactly.
const FARTHEST: f32 = 1_048_576.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Shift {
    pub(crate) x: i32,
    pub(crate) y: i32,
}

impl Shift {
    pub(crate) const NONE: Self = Self { x: 0, y: 0 };

    pub(crate) fn is_none(self) -> bool {
        self == Self::NONE
    }

    // The shift that takes items snapped to the anchor `from` to where they
    // lie snapped to the anchor `to`, with the anchor `from` moved by it.
    // `None` when an anchor is not finite, the snaps lie too far apart, or
    // the moved anchor snaps elsewhere than `to`: a half rounds away from
    // zero, so a whole move may take it across zero to the other side.
    pub(crate) fn snapping(from: Point, to: Point) -> Option<(Self, Point)> {
        let axis = |from: f32, to: f32| {
            let moved = whole::round(to) - whole::round(from);
            (moved.abs() <= FARTHEST).then(|| whole::to_i32(moved))
        };
        let shift = Self {
            x: axis(from.x, to.x)?,
            y: axis(from.y, to.y)?,
        };
        let moved = shift.point(from);
        let snap = |point: Point| {
            (
                whole::to_i32(whole::round(point.x)),
                whole::to_i32(whole::round(point.y)),
            )
        };
        (snap(moved) == snap(to)).then_some((shift, moved))
    }

    pub(crate) fn offset(self) -> (f32, f32) {
        (self.x as f32, self.y as f32)
    }

    pub(crate) fn point(self, point: Point) -> Point {
        if self.is_none() {
            return point;
        }
        let (dx, dy) = self.offset();
        Point::new(point.x + dx, point.y + dy)
    }

    pub(crate) fn rect(self, rect: Rect) -> Rect {
        if self.is_none() {
            return rect;
        }
        let (dx, dy) = self.offset();
        rect.translate(dx, dy)
    }
}
