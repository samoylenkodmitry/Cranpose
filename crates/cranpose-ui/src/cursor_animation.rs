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
    if crate::render_state::with_cursor_animation(|state| state.start()) {
        invalidate_focused_caret();
    }
}

/// Stops the active context's cursor blink animation.
/// Called when no text field is focused.
pub fn stop_cursor_blink() {
    if crate::render_state::with_cursor_animation(|state| state.stop()) {
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
    if crate::render_state::with_cursor_animation(|state| state.stop()) {
        invalidate_focused_caret();
    }
}

/// Returns whether the cursor should be visible right now.
pub fn is_cursor_visible() -> bool {
    crate::render_state::with_cursor_animation(|state| state.is_visible())
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
    crate::render_state::with_cursor_animation(|state| state.next_blink_time())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_starts_visible() {
        let state = CursorAnimationState::new();
        assert!(state.is_visible());
        assert!(!state.is_active());
    }

    #[test]
    fn start_schedules_blink() {
        let state = CursorAnimationState::new();
        state.start();
        assert!(state.is_active());
        assert!(state.next_blink_time().is_some());
    }

    #[test]
    fn stop_clears_blink() {
        let state = CursorAnimationState::new();
        state.start();
        state.stop();
        assert!(!state.is_active());
        assert!(state.next_blink_time().is_none());
        assert!(state.is_visible());
    }

    #[test]
    fn tick_toggles_visibility() {
        let state = CursorAnimationState::new();
        state.start();
        assert!(state.is_visible());

        let future_time =
            Instant::now() + CursorAnimationState::BLINK_INTERVAL + Duration::from_millis(1);
        let changed = state.tick(future_time);

        assert!(changed);
        assert!(!state.is_visible());

        let future_time2 =
            future_time + CursorAnimationState::BLINK_INTERVAL + Duration::from_millis(1);
        let changed2 = state.tick(future_time2);

        assert!(changed2);
        assert!(state.is_visible());
    }

    #[test]
    fn cursor_blink_is_scoped_by_app_context() {
        let first = crate::render_state::AppContext::new_with_density(1.0);
        let second = crate::render_state::AppContext::new_with_density(1.0);

        first.enter(|| {
            stop_cursor_blink();
            start_cursor_blink();
            assert!(next_cursor_blink_time().is_some());
        });

        second.enter(|| {
            stop_cursor_blink();
            assert!(next_cursor_blink_time().is_none());
        });

        first.enter(|| {
            assert!(next_cursor_blink_time().is_some());
            stop_cursor_blink();
        });
    }
}
