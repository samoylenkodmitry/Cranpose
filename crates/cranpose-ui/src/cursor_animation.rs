use std::cell::Cell;

use web_time::{Duration, Instant};

pub const BLINK_INTERVAL_MS: u64 = 500;

pub struct CursorAnimationState {
    cursor_alpha: Cell<f32>,
    next_blink_time: Cell<Option<Instant>>,
}

impl CursorAnimationState {
    pub const BLINK_INTERVAL: Duration = Duration::from_millis(BLINK_INTERVAL_MS);

    pub const fn new() -> Self {
        Self {
            cursor_alpha: Cell::new(1.0),
            next_blink_time: Cell::new(None),
        }
    }

    pub fn start(&self) -> bool {
        let flipped = !self.is_visible();
        self.cursor_alpha.set(1.0);
        self.next_blink_time
            .set(Some(Instant::now() + Self::BLINK_INTERVAL));
        flipped
    }

    pub fn stop(&self) -> bool {
        let flipped = !self.is_visible();
        self.cursor_alpha.set(1.0);
        self.next_blink_time.set(None);
        flipped
    }

    #[cfg(test)]
    pub fn is_active(&self) -> bool {
        self.next_blink_time.get().is_some()
    }

    pub fn is_visible(&self) -> bool {
        self.cursor_alpha.get() > 0.5
    }

    pub fn tick(&self, now: Instant) -> bool {
        if let Some(next) = self.next_blink_time.get()
            && now >= next
        {
            let new_alpha = if self.cursor_alpha.get() > 0.5 {
                0.0
            } else {
                1.0
            };
            self.cursor_alpha.set(new_alpha);
            self.next_blink_time.set(Some(now + Self::BLINK_INTERVAL));
            return true;
        }
        false
    }

    pub fn next_blink_time(&self) -> Option<Instant> {
        self.next_blink_time.get()
    }
}

/// Starts the active context's cursor blink animation.
/// Called when a text field gains focus.
pub fn start_cursor_blink() {
    if crate::render_state::with_cursor_animation(CursorAnimationState::start) {
        invalidate_focused_caret();
    }
}

/// Stops the active context's cursor blink animation.
/// Called when no text field is focused.
pub fn stop_cursor_blink() {
    if crate::render_state::with_cursor_animation(CursorAnimationState::stop) {
        invalidate_focused_caret();
    }
}

fn invalidate_focused_caret() {
    if let Some(node_id) = crate::text_field_focus::focused_field_node() {
        crate::schedule_draw_repass(node_id);
    }
    crate::request_render_invalidation();
}

/// Resets cursor to visible and restarts the blink timer.
/// Call this on any input (key press, paste) so cursor stays visible while typing.
#[inline]
pub fn reset_cursor_blink() {
    start_cursor_blink();
}

pub fn suspend_cursor_blink() {
    if crate::render_state::with_cursor_animation(CursorAnimationState::stop) {
        invalidate_focused_caret();
    }
}

/// Returns whether the cursor should be visible right now.
pub fn is_cursor_visible() -> bool {
    crate::render_state::with_cursor_animation(CursorAnimationState::is_visible)
}

/// Advances the cursor blink state if needed.
/// Returns `true` if a redraw is needed. A transition schedules a scoped
/// draw repass on the focused field, so the caller only has to run the
/// ordinary dirty-node paths — no whole-scene work.
pub fn tick_cursor_blink() -> bool {
    tick_cursor_blink_at(Instant::now())
}

pub(crate) fn tick_cursor_blink_at(now: Instant) -> bool {
    let flipped = crate::render_state::with_cursor_animation(|state| state.tick(now));
    if flipped {
        invalidate_focused_caret();
    }
    flipped
}

/// Returns the next cursor blink transition time, if any.
/// Use this for `WaitUntil` scheduling in the event loop.
pub fn next_cursor_blink_time() -> Option<Instant> {
    crate::render_state::with_cursor_animation(CursorAnimationState::next_blink_time)
}

#[cfg(test)]
#[path = "tests/cursor_animation_tests.rs"]
mod tests;
