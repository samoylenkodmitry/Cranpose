//! Platform text-input session: soft-keyboard visibility hooks.
//!
//! Platforms with an on-screen keyboard (Android, iOS, some Linux shells)
//! install a [`PlatformTextInputHandler`] so the framework can tell them when
//! editable text gains or loses focus. The text-field focus manager
//! ([`crate::text_field_focus`]) fires these notifications:
//!
//! - a text field acquired focus → `notify_text_input_focus_gained` →
//!   `show_keyboard`
//! - focus was explicitly cleared, or the focused field left the composition →
//!   `notify_text_input_focus_lost` → `hide_keyboard`
//!
//! `show_keyboard` fires on *every* focus request, including taps on an
//! already-focused field. This is intentional: the user may have dismissed the
//! keyboard (e.g. Android back gesture) without the framework knowing, and
//! tapping the field again must bring it back. Platform show/hide calls are
//! expected to be idempotent. `hide_keyboard` is only forwarded when the
//! framework previously requested the keyboard, so repeated stale-focus checks
//! do not spam the platform.
//!
//! The handler is stored per [`AppContext`](crate::render_state::AppContext),
//! like the focus state itself, so multiple app instances in one process do
//! not observe each other's keyboards.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

/// Callbacks a platform installs to control its on-screen keyboard.
///
/// Implementations must be idempotent: `show_keyboard` may be invoked while
/// the keyboard is already visible (every tap on a text field re-requests it)
/// and `hide_keyboard` may race a keyboard the user already dismissed.
pub trait PlatformTextInputHandler {
    /// A text field gained focus; the platform should show its soft keyboard.
    fn show_keyboard(&self);
    /// No text field is focused anymore; the platform should hide its soft
    /// keyboard.
    fn hide_keyboard(&self);
}

pub(crate) struct PlatformTextInputState {
    handler: RefCell<Option<Rc<dyn PlatformTextInputHandler>>>,
    keyboard_requested: Cell<bool>,
}

impl PlatformTextInputState {
    pub(crate) fn new() -> Self {
        Self {
            handler: RefCell::new(None),
            keyboard_requested: Cell::new(false),
        }
    }

    fn set_handler(&self, handler: Option<Rc<dyn PlatformTextInputHandler>>) {
        *self.handler.borrow_mut() = handler;
        self.keyboard_requested.set(false);
    }

    fn handler(&self) -> Option<Rc<dyn PlatformTextInputHandler>> {
        self.handler.borrow().clone()
    }
}

/// Installs the platform soft-keyboard handler for the current app context.
///
/// Replaces any previously installed handler. Must be called inside an app
/// context (platform runtimes go through
/// `AppShell::set_platform_text_input`).
pub fn set_platform_text_input_handler(handler: Rc<dyn PlatformTextInputHandler>) {
    crate::render_state::with_text_input_session(|state| state.set_handler(Some(handler)));
}

/// Removes the installed platform soft-keyboard handler, if any.
pub fn clear_platform_text_input_handler() {
    crate::render_state::with_text_input_session(|state| state.set_handler(None));
}

pub(crate) fn notify_text_input_focus_gained() {
    let handler = crate::render_state::with_text_input_session(|state| {
        let handler = state.handler();
        if handler.is_some() {
            state.keyboard_requested.set(true);
        }
        handler
    });
    if let Some(handler) = handler {
        handler.show_keyboard();
    }
}

pub(crate) fn notify_text_input_focus_lost() {
    let handler = crate::render_state::with_text_input_session(|state| {
        if !state.keyboard_requested.replace(false) {
            return None;
        }
        state.handler()
    });
    if let Some(handler) = handler {
        handler.hide_keyboard();
    }
}

/// Notifies the framework that the host app was paused (backgrounded — e.g.
/// Android `onPause`).
///
/// Any outstanding soft-keyboard request is withdrawn and the platform is told
/// to hide its keyboard, clearing the "keyboard shown" state so it cannot
/// survive into the next resume. Without this, a platform that remembers the
/// last editor view (Android's `InputMethodManager`) re-shows the keyboard when
/// the app returns to the foreground even though the framework no longer has a
/// focused field. Gated on an outstanding request, so it is a no-op when the
/// keyboard was not showing.
pub fn notify_app_paused() {
    notify_text_input_focus_lost();
}

/// Notifies the framework that the host app resumed (foregrounded — e.g.
/// Android `onResume`).
///
/// The soft keyboard is **never** auto-shown on resume, even when a text field
/// is still focused. A warm resume (return from HOME / task switch / back-exit
/// then relaunch) restores the process with the field's focus and caret intact,
/// but the framework must not resurrect the keyboard for it: the platform's
/// `InputMethodManager` remembers the last editor and would otherwise pop the
/// keyboard back open on its own. The user brings it back by tapping the field
/// (which re-requests it through `notify_text_input_focus_gained`).
///
/// Always returns `false` so the platform runtime force-hides the OS-restored
/// keyboard. Pruning stale focus here keeps the keyboard-request bookkeeping
/// consistent (a focused-but-detached field is dropped and its outstanding
/// request withdrawn) without ever calling `show`.
pub fn notify_app_resumed() -> bool {
    let _ = crate::text_field_focus::has_focused_field();
    false
}

#[cfg(test)]
#[path = "tests/text_input_session_tests.rs"]
mod tests;
