//! The ring a control shows while the keyboard holds focus on it, so a person
//! who works without a pointer sees where the next Enter lands.

use std::{cell::Cell, rc::Rc};

use cranpose_ui_graphics::{Brush, Color, DrawScope, Rect, Size, Stroke};

use super::Modifier;
use crate::focus_dispatch::keyboard_focus_visible;

/// The outer line of the ring: a blue that stands out on white and on black.
pub const FOCUS_RING_COLOR: Color = Color::from_rgba_u8(0, 95, 204, 255);
/// The inner line of the ring, so the ring stands out on a blue control too.
pub const FOCUS_RING_INNER_COLOR: Color = Color::from_rgba_u8(255, 255, 255, 255);
const OUTER_WIDTH: f32 = 2.0;
const INNER_WIDTH: f32 = 1.0;

/// The two lines of a ring around content of the given size, outer line
/// first, each with its stroke width. Both lines sit inside the content
/// bounds, so the ring never leaves the control's own area.
pub fn focus_ring_lines(size: Size) -> [(Rect, f32); 2] {
    [
        (inset(size, OUTER_WIDTH / 2.0), OUTER_WIDTH),
        (inset(size, OUTER_WIDTH + INNER_WIDTH / 2.0), INNER_WIDTH),
    ]
}

fn inset(size: Size, by: f32) -> Rect {
    Rect {
        x: by,
        y: by,
        width: (size.width - 2.0 * by).max(0.0),
        height: (size.height - 2.0 * by).max(0.0),
    }
}

fn draw_focus_ring(scope: &mut dyn DrawScope) {
    let [(outer, outer_width), (inner, inner_width)] = focus_ring_lines(scope.size());
    scope.draw_rect_at_stroked(
        outer,
        Brush::solid(FOCUS_RING_COLOR),
        Stroke::new(outer_width),
    );
    scope.draw_rect_at_stroked(
        inner,
        Brush::solid(FOCUS_RING_INNER_COLOR),
        Stroke::new(inner_width),
    );
}

impl Modifier {
    /// A focus target that draws a ring while the keyboard put focus on it.
    ///
    /// The ring shows after Tab or an arrow key moved focus here and goes
    /// away on the next pointer press anywhere, the way a web browser's
    /// `:focus-visible` works. A control that draws its own focus look uses
    /// [`focus_target`](Self::focus_target) instead. Compose's
    /// `Modifier.focusable()`.
    pub fn focusable(self) -> Self {
        let focused = Rc::new(Cell::new(false));
        let seen = Rc::clone(&focused);
        self.on_focus_changed(move |state| {
            seen.set(state.is_focused());
            crate::request_render_invalidation();
        })
        .draw_with_content(move |scope| {
            scope.draw_content();
            if focused.get() && keyboard_focus_visible() {
                draw_focus_ring(scope);
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_stays_inside_the_control() {
        let [(outer, outer_width), (inner, inner_width)] = focus_ring_lines(Size {
            width: 48.0,
            height: 20.0,
        });
        assert_eq!((outer_width, inner_width), (2.0, 1.0));
        assert_eq!(
            (outer.x, outer.y, outer.width, outer.height),
            (1.0, 1.0, 46.0, 18.0)
        );
        assert_eq!(
            (inner.x, inner.y, inner.width, inner.height),
            (2.5, 2.5, 43.0, 15.0)
        );
    }

    #[test]
    fn a_ring_around_nothing_has_no_width() {
        let [(outer, _), (inner, _)] = focus_ring_lines(Size {
            width: 1.0,
            height: 1.0,
        });
        assert_eq!(
            (outer.width, outer.height, inner.width, inner.height),
            (0.0, 0.0, 0.0, 0.0)
        );
    }
}
