//! The pointer icon the platform should be drawing right now.
//!
//! The shell resolves the hovered region's [`PointerIcon`] on every pointer
//! move and records it here; the platform layer reads the pending change back
//! and applies it to the window it owns (winit's `Window::set_cursor` on
//! desktop, the canvas's CSS `cursor` on the web). The indirection exists
//! because neither side can call the other directly: `cranpose-ui` cannot reach
//! a window handle, and the platform backend is not on the stack when a
//! modifier declares its icon.
//!
//! Reading is a poll rather than a callback because a windowing system wants
//! the cursor set from its own event loop, holding the handles only that loop
//! has: the platform asks for the change at the point where it can act on it.
//!
//! The session is stored per [`AppContext`](crate::render_state::AppContext),
//! like the clipboard and text-input sessions, so each window in a multi-window
//! app carries its own pointer icon.

use std::cell::RefCell;

use cranpose_ui_graphics::PointerIcon;

/// The icon one window's pointer shows, with the change its platform has not
/// applied yet. The app context keeps one for callers of the free functions
/// below; a shell keeps one per surface, because the OS owns cursors per
/// window.
pub struct PointerIconState {
    current: RefCell<PointerIcon>,
    pending: RefCell<Option<PointerIcon>>,
}

impl Default for PointerIconState {
    fn default() -> Self {
        Self::new()
    }
}

impl PointerIconState {
    /// A session showing the platform default with no change pending.
    pub fn new() -> Self {
        Self {
            current: RefCell::new(PointerIcon::DEFAULT),
            pending: RefCell::new(None),
        }
    }

    /// Requests `icon`; a request for the icon already held changes nothing.
    pub fn set(&self, icon: PointerIcon) {
        if *self.current.borrow() == icon {
            return;
        }
        *self.current.borrow_mut() = icon.clone();
        *self.pending.borrow_mut() = Some(icon);
    }

    /// The change the platform has not applied yet, taken.
    pub fn take_change(&self) -> Option<PointerIcon> {
        self.pending.borrow_mut().take()
    }

    /// Offers the held icon to the platform again, as a pending change.
    pub fn refresh(&self) {
        *self.pending.borrow_mut() = Some(self.current.borrow().clone());
    }

    /// The icon currently requested, applied or not.
    pub fn current(&self) -> PointerIcon {
        self.current.borrow().clone()
    }
}

/// Requests `icon` as the pointer's appearance.
///
/// Called by the shell once per pointer move with whatever the hovered region
/// asks for. Setting the icon the session already holds does nothing, so a
/// pointer travelling across one region does not re-upload its cursor.
pub fn set_pointer_icon(icon: PointerIcon) {
    crate::render_state::with_pointer_icon_session(|state| state.set(icon));
}

/// Takes the pointer icon change the platform has not applied yet, leaving
/// nothing behind.
///
/// Returns `None` when the icon has not changed since the last call, which is
/// the common case: a platform backend calls this after every batch of input
/// and touches its window only when something comes back.
pub fn take_pointer_icon_change() -> Option<PointerIcon> {
    crate::render_state::with_pointer_icon_session(|state| state.take_change())
}

/// Offers the icon the session already holds to the platform again.
///
/// A windowing system resets the cursor to its own default on the way back
/// into a window — when the application is activated, or when the pointer
/// crosses in — without telling the application what it drew. Nothing in the
/// hovered region has changed, so no change would be reported and the platform
/// default would stay on screen over a region that names its own cursor. The
/// platform layer calls this at those moments so the next poll re-applies what
/// the region already asked for.
pub fn refresh_pointer_icon() {
    crate::render_state::with_pointer_icon_session(|state| state.refresh());
}

/// The pointer icon currently requested, whether or not the platform has
/// applied it yet.
pub fn current_pointer_icon() -> PointerIcon {
    crate::render_state::with_pointer_icon_session(|state| state.current())
}

#[cfg(test)]
mod tests {
    use cranpose_ui_graphics::{CursorIcon, ImageBitmap};

    use super::*;
    use crate::render_state::AppContext;

    fn custom_icon() -> PointerIcon {
        PointerIcon::custom(
            ImageBitmap::from_rgba8(4, 4, vec![255; 64]).expect("bitmap"),
            1,
            2,
        )
        .expect("icon")
    }

    #[test]
    fn a_fresh_session_holds_the_default_icon_and_no_change() {
        let context = AppContext::new();
        context.enter(|| {
            assert_eq!(current_pointer_icon(), PointerIcon::DEFAULT);
            assert_eq!(take_pointer_icon_change(), None);
        });
    }

    #[test]
    fn setting_a_new_icon_yields_one_change() {
        let context = AppContext::new();
        context.enter(|| {
            set_pointer_icon(PointerIcon::POINTER);
            assert_eq!(current_pointer_icon(), PointerIcon::POINTER);
            assert_eq!(take_pointer_icon_change(), Some(PointerIcon::POINTER));
            assert_eq!(take_pointer_icon_change(), None);
        });
    }

    #[test]
    fn re_setting_the_same_icon_reports_no_change() {
        let context = AppContext::new();
        context.enter(|| {
            set_pointer_icon(PointerIcon::TEXT);
            assert_eq!(take_pointer_icon_change(), Some(PointerIcon::TEXT));
            set_pointer_icon(PointerIcon::TEXT);
            assert_eq!(take_pointer_icon_change(), None);
        });
    }

    #[test]
    fn the_latest_icon_wins_when_the_platform_has_not_polled() {
        let context = AppContext::new();
        context.enter(|| {
            set_pointer_icon(PointerIcon::POINTER);
            set_pointer_icon(PointerIcon::System(CursorIcon::Crosshair));
            assert_eq!(
                take_pointer_icon_change(),
                Some(PointerIcon::System(CursorIcon::Crosshair))
            );
            assert_eq!(take_pointer_icon_change(), None);
        });
    }

    #[test]
    fn custom_icons_round_trip_through_the_session() {
        let context = AppContext::new();
        context.enter(|| {
            let icon = custom_icon();
            set_pointer_icon(icon.clone());
            assert_eq!(take_pointer_icon_change(), Some(icon.clone()));
            set_pointer_icon(icon);
            assert_eq!(take_pointer_icon_change(), None);
        });
    }

    #[test]
    fn a_refresh_offers_the_icon_the_region_already_asked_for() {
        let context = AppContext::new();
        context.enter(|| {
            set_pointer_icon(PointerIcon::POINTER);
            assert_eq!(take_pointer_icon_change(), Some(PointerIcon::POINTER));
            assert_eq!(take_pointer_icon_change(), None);

            refresh_pointer_icon();
            assert_eq!(
                take_pointer_icon_change(),
                Some(PointerIcon::POINTER),
                "coming back to the window re-applies the region's own cursor"
            );
        });
    }

    #[test]
    fn a_refresh_on_a_window_that_asked_for_nothing_restores_the_default() {
        let context = AppContext::new();
        context.enter(|| {
            refresh_pointer_icon();
            assert_eq!(take_pointer_icon_change(), Some(PointerIcon::DEFAULT));
        });
    }

    #[test]
    fn each_app_context_carries_its_own_icon() {
        let first = AppContext::new();
        let second = AppContext::new();
        first.enter(|| set_pointer_icon(PointerIcon::POINTER));
        second.enter(|| {
            assert_eq!(current_pointer_icon(), PointerIcon::DEFAULT);
            assert_eq!(take_pointer_icon_change(), None);
        });
        first.enter(|| {
            assert_eq!(take_pointer_icon_change(), Some(PointerIcon::POINTER));
        });
    }
}
