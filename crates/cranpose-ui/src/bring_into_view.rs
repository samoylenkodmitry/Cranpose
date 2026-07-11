//! Bring-a-focused-field's-caret-into-view above the soft keyboard.
//!
//! A scroll container ([`crate::widgets::LazyColumn`], a `Modifier.vertical_scroll`
//! column) installs a [`BringIntoViewResponder`] into composition via
//! [`local_bring_into_view_responder`]. A focused [`crate::widgets::BasicTextField`]
//! reads that responder and, on focus / caret move / keyboard animation, asks it
//! to scroll the caret's window rect clear of the on-screen keyboard
//! ([`crate::safe_area::local_ime_insets`]).
//!
//! The geometry decision is the pure, unit-tested [`scroll_delta_to_reveal`]; the
//! responder only owns its viewport rect (reported into a cell by the layout pass
//! through [`Modifier::report_window_rect`]) and how to apply a scroll delta to
//! its own scroll state.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use cranpose_core::{compositionLocalOf, CompositionLocal};
use cranpose_ui_graphics::Rect;

use crate::modifier::Modifier;

/// Breathing room (px) kept between the caret and the edge of the visible region
/// when scrolling it into view, so the caret never sits flush against the
/// keyboard or the viewport edge.
pub const BRING_INTO_VIEW_MARGIN: f32 = 12.0;

/// The scroll-offset delta needed to bring `caret` fully inside the un-obscured
/// part of `viewport`.
///
/// All three arguments are in the same (window) coordinate space. `ime_bottom` is
/// how many pixels at the **bottom of the viewport** are covered by the on-screen
/// keyboard (0 when it is hidden); the usable region is therefore
/// `[viewport.y, viewport.y + viewport.height - ime_bottom]`.
///
/// The return value is a delta to **add to the container's scroll offset**:
/// * positive → scroll toward the content end (content moves up) so a caret
///   hidden below the fold / behind the keyboard rises into view;
/// * negative → scroll back toward the content start (content moves down) so a
///   caret above the viewport top drops into view;
/// * `0.0` → the caret already fits, or the keyboard leaves no usable space.
///
/// When the caret is taller than the usable region the top edge is prioritised so
/// the start of the caret line stays visible.
pub fn scroll_delta_to_reveal(caret: Rect, viewport: Rect, ime_bottom: f32) -> f32 {
    let ime_bottom = ime_bottom.max(0.0);
    let visible_top = viewport.y;
    let visible_bottom = viewport.y + viewport.height - ime_bottom;
    // Keyboard (or a degenerate viewport) leaves nothing usable: don't fight it.
    if visible_bottom <= visible_top {
        return 0.0;
    }

    let caret_top = caret.y;
    let caret_bottom = caret.y + caret.height.max(0.0);

    // Hidden below the fold (behind the keyboard or past the viewport bottom):
    // scroll just enough to reveal the caret bottom plus a margin, but never so
    // far that the caret top is pushed above the visible top.
    if caret_bottom + BRING_INTO_VIEW_MARGIN > visible_bottom {
        let needed = caret_bottom + BRING_INTO_VIEW_MARGIN - visible_bottom;
        // Never scroll so far that the caret top is pushed above the visible
        // top (matters only when the caret is taller than the usable region).
        let max_delta = (caret_top - visible_top).max(0.0);
        return needed.min(max_delta);
    }

    // Hidden above the top of the viewport: scroll back so the caret top clears
    // the top edge by a margin.
    if caret_top - BRING_INTO_VIEW_MARGIN < visible_top {
        return caret_top - BRING_INTO_VIEW_MARGIN - visible_top;
    }

    0.0
}

/// A scroll container's ability to scroll a target window rect into view.
///
/// Installed into composition by scroll containers via
/// [`local_bring_into_view_responder`] and read by a focused text field. Equality
/// is by identity so providing the same remembered responder does not thrash the
/// composition local.
#[derive(Clone)]
pub struct BringIntoViewResponder {
    inner: Rc<dyn Fn(Rect, f32)>,
}

impl BringIntoViewResponder {
    /// Builds a responder from a callback that receives the caret's window rect
    /// and the keyboard inset (px covering the viewport bottom).
    pub fn new(responder: impl Fn(Rect, f32) + 'static) -> Self {
        Self {
            inner: Rc::new(responder),
        }
    }

    /// Asks the container to scroll `caret_window_rect` clear of a keyboard that
    /// covers the bottom `ime_bottom` px of the viewport.
    pub fn bring_into_view(&self, caret_window_rect: Rect, ime_bottom: f32) {
        (self.inner)(caret_window_rect, ime_bottom);
    }
}

impl PartialEq for BringIntoViewResponder {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl std::fmt::Debug for BringIntoViewResponder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BringIntoViewResponder").finish()
    }
}

/// CompositionLocal carrying the nearest scroll container's
/// [`BringIntoViewResponder`], or `None` outside any scroll container.
///
/// Scroll containers provide it around their content; a focused text field reads
/// `current()` to request that its caret be scrolled above the keyboard. Like the
/// safe-area locals, the same instance is returned per thread.
pub fn local_bring_into_view_responder() -> CompositionLocal<Option<BringIntoViewResponder>> {
    thread_local! {
        static LOCAL: RefCell<Option<CompositionLocal<Option<BringIntoViewResponder>>>> =
            const { RefCell::new(None) };
    }
    LOCAL.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| compositionLocalOf(|| None))
            .clone()
    })
}

impl Modifier {
    /// Publishes this node's composited window rect (window coordinates,
    /// resolved through ancestor scroll placement + graphics-layer translation)
    /// into `sink` every layout pass.
    ///
    /// Scroll containers use it to expose their viewport bounds to a
    /// [`BringIntoViewResponder`]. Positioning-only: it draws nothing and does
    /// not affect layout.
    pub fn report_window_rect(self, sink: Rc<Cell<Rect>>) -> Self {
        self.then(Modifier::with_element(
            crate::modifier_nodes::WindowRectReporterElement::new(sink),
        ))
    }

    /// Publishes this node's measured size (logical px) into `sink` on every
    /// measure pass — the `onSizeChanged` seam for consumers that need the
    /// resolved size outside layout (shader morph geometry, lens overlays).
    pub fn report_size(self, sink: Rc<Cell<cranpose_ui_graphics::Size>>) -> Self {
        self.then(Modifier::with_element(
            crate::modifier_nodes::SizeReporterElement::new(sink),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(y: f32, height: f32) -> Rect {
        Rect {
            x: 0.0,
            y,
            width: 100.0,
            height,
        }
    }

    #[test]
    fn visible_caret_needs_no_scroll() {
        let viewport = rect(0.0, 1000.0);
        let caret = rect(400.0, 20.0);
        assert_eq!(scroll_delta_to_reveal(caret, viewport, 0.0), 0.0);
    }

    #[test]
    fn caret_behind_keyboard_scrolls_content_up() {
        // Viewport 0..1000, keyboard covers bottom 400 → usable 0..600.
        let viewport = rect(0.0, 1000.0);
        // Caret at y=700..720 is behind the keyboard.
        let caret = rect(700.0, 20.0);
        let delta = scroll_delta_to_reveal(caret, viewport, 400.0);
        // Must move content up so the caret bottom (720) + margin clears 600.
        assert!(
            delta > 0.0,
            "expected a positive (content-up) delta, got {delta}"
        );
        assert!(
            (delta - (720.0 + BRING_INTO_VIEW_MARGIN - 600.0)).abs() < 0.01,
            "delta {delta} should reveal the caret bottom plus a margin"
        );
        // After applying the delta the caret bottom sits at/above the fold.
        let revealed_bottom = caret.y + caret.height - delta;
        assert!(revealed_bottom <= 600.0 - BRING_INTO_VIEW_MARGIN + 0.01);
    }

    #[test]
    fn caret_just_below_fold_uses_margin() {
        let viewport = rect(0.0, 1000.0);
        // Usable region 0..600; caret bottom exactly at 600 still needs a nudge
        // for the margin.
        let caret = rect(580.0, 20.0);
        let delta = scroll_delta_to_reveal(caret, viewport, 400.0);
        assert!((delta - BRING_INTO_VIEW_MARGIN).abs() < 0.01, "got {delta}");
    }

    #[test]
    fn caret_above_viewport_top_scrolls_content_down() {
        // Container starts at y=200 (below a header); caret sits above it.
        let viewport = rect(200.0, 800.0);
        let caret = rect(150.0, 20.0);
        let delta = scroll_delta_to_reveal(caret, viewport, 0.0);
        assert!(
            delta < 0.0,
            "expected a negative (content-down) delta, got {delta}"
        );
        assert!(
            (delta - (150.0 - BRING_INTO_VIEW_MARGIN - 200.0)).abs() < 0.01,
            "delta {delta} should reveal the caret top with a margin"
        );
    }

    #[test]
    fn no_usable_space_does_not_scroll() {
        // Keyboard taller than the viewport: refuse rather than scroll wildly.
        let viewport = rect(0.0, 300.0);
        let caret = rect(280.0, 20.0);
        assert_eq!(scroll_delta_to_reveal(caret, viewport, 400.0), 0.0);
    }

    #[test]
    fn responder_identity_equality() {
        let r = BringIntoViewResponder::new(|_, _| {});
        assert_eq!(r, r.clone());
        let other = BringIntoViewResponder::new(|_, _| {});
        assert_ne!(r, other);
    }

    #[test]
    fn responder_forwards_request() {
        let seen: Rc<Cell<Option<(f32, f32)>>> = Rc::new(Cell::new(None));
        let seen2 = seen.clone();
        let r = BringIntoViewResponder::new(move |caret: Rect, ime: f32| {
            seen2.set(Some((caret.y, ime)));
        });
        r.bring_into_view(rect(42.0, 20.0), 300.0);
        assert_eq!(seen.get(), Some((42.0, 300.0)));
    }
}
