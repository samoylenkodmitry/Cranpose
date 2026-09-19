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
#[path = "tests/pointer_icon_session_tests.rs"]
mod tests;
