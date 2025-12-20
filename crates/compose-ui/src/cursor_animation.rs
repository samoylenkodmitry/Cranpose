//! Cursor blink animation state.
//!
//! Provides timer-based cursor visibility for text fields,
//! avoiding continuous redraw when a field is focused.
//!
//! This follows the pattern from Jetpack Compose's `CursorAnimationState.kt`:
//! - Cursor visibility toggles on a fixed interval
//! - Only requests redraw at transition times, not continuously
//! - Uses platform timer scheduling (`WaitUntil`) instead of busy-polling

use std::cell::Cell;
use instant::{Duration, Instant};

/// Cursor blink interval in milliseconds.
pub const BLINK_INTERVAL_MS: u64 = 500;

thread_local! {
    /// Global cursor animation state.
    /// Shared by all text fields - only the focused one renders the cursor.
    static CURSOR_STATE: CursorAnimationState = const { CursorAnimationState::new() };
}

/// Cursor blink animation state.
///
/// Manages timed visibility transitions instead of continuous redraw.
/// The cursor alternates between visible (alpha=1.0) and hidden (alpha=0.0)
/// on a fixed interval.
pub struct CursorAnimationState {
    /// Current cursor alpha (0.0 = hidden, 1.0 = visible)
    cursor_alpha: Cell<f32>,
    /// Next scheduled blink transition time
    next_blink_time: Cell<Option<Instant>>,
}

impl CursorAnimationState {
    /// Blink interval duration
    pub const BLINK_INTERVAL: Duration = Duration::from_millis(BLINK_INTERVAL_MS);
    
    /// Creates a new cursor animation state (cursor initially visible, not blinking).
    pub const fn new() -> Self {
        Self {
            cursor_alpha: Cell::new(1.0),
            next_blink_time: Cell::new(None),
        }
    }
    
    /// Starts the blink animation (called when a text field gains focus).
    /// Resets cursor to visible and schedules the first transition.
    pub fn start(&self) {
        self.cursor_alpha.set(1.0);
        self.next_blink_time.set(Some(Instant::now() + Self::BLINK_INTERVAL));
    }
    
    /// Stops the blink animation (called when text field loses focus).
    /// Resets cursor to visible for next focus.
    pub fn stop(&self) {
        self.cursor_alpha.set(1.0); // Reset to visible for next focus
        self.next_blink_time.set(None);
    }
    
    /// Returns whether blinking is active.
    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        self.next_blink_time.get().is_some()
    }
    
    /// Returns the current cursor alpha (0.0 or 1.0).
    #[allow(dead_code)]
    pub fn alpha(&self) -> f32 {
        self.cursor_alpha.get()
    }
    
    /// Returns whether the cursor is currently visible.
    pub fn is_visible(&self) -> bool {
        self.cursor_alpha.get() > 0.5
    }
    
    /// Advances the blink state if the transition time has passed.
    /// Returns `true` if the state changed (redraw needed).
    pub fn tick(&self, now: Instant) -> bool {
        if let Some(next) = self.next_blink_time.get() {
            if now >= next {
                // Toggle visibility
                let new_alpha = if self.cursor_alpha.get() > 0.5 { 0.0 } else { 1.0 };
                self.cursor_alpha.set(new_alpha);
                // Schedule next transition
                self.next_blink_time.set(Some(now + Self::BLINK_INTERVAL));
                return true;
            }
        }
        false
    }
    
    /// Returns the next blink transition time, if blinking is active.
    /// Use this for `WaitUntil` scheduling.
    pub fn next_blink_time(&self) -> Option<Instant> {
        self.next_blink_time.get()
    }
}

// ============================================================================
// Global accessor functions (thread-local)
// ============================================================================

/// Starts the global cursor blink animation.
/// Called when a text field gains focus.
pub fn start_cursor_blink() {
    CURSOR_STATE.with(|state| state.start());
}

/// Stops the global cursor blink animation.
/// Called when no text field is focused.
pub fn stop_cursor_blink() {
    CURSOR_STATE.with(|state| state.stop());
}

/// Resets cursor to visible and restarts the blink timer.
/// Call this on any input (key press, paste) so cursor stays visible while typing.
#[inline]
pub fn reset_cursor_blink() {
    start_cursor_blink();
}

/// Returns whether the cursor should be visible right now.
pub fn is_cursor_visible() -> bool {
    CURSOR_STATE.with(|state| state.is_visible())
}

/// Advances the cursor blink state if needed.
/// Returns `true` if a redraw is needed.
pub fn tick_cursor_blink() -> bool {
    CURSOR_STATE.with(|state| state.tick(Instant::now()))
}

/// Returns the next cursor blink transition time, if any.
/// Use this for `WaitUntil` scheduling in the event loop.
pub fn next_cursor_blink_time() -> Option<Instant> {
    CURSOR_STATE.with(|state| state.next_blink_time())
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
        assert!(state.is_visible()); // Should be visible after stop
    }
    
    #[test]
    fn tick_toggles_visibility() {
        let state = CursorAnimationState::new();
        state.start();
        assert!(state.is_visible());
        
        // Simulate time passing beyond blink interval
        let future_time = Instant::now() + CursorAnimationState::BLINK_INTERVAL + Duration::from_millis(1);
        let changed = state.tick(future_time);
        
        assert!(changed);
        assert!(!state.is_visible()); // Should have toggled
        
        // Tick again after another interval
        let future_time2 = future_time + CursorAnimationState::BLINK_INTERVAL + Duration::from_millis(1);
        let changed2 = state.tick(future_time2);
        
        assert!(changed2);
        assert!(state.is_visible()); // Should toggle back
    }
}
