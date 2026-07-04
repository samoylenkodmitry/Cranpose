//! Platform text-input session: soft-keyboard visibility hooks.
//!
//! Platforms with an on-screen keyboard (Android, iOS, some Linux shells)
//! install a [`PlatformTextInputHandler`] so the framework can tell them when
//! editable text gains or loses focus. The text-field focus manager
//! ([`crate::text_field_focus`]) fires these notifications:
//!
//! - a text field acquired focus → [`notify_text_input_focus_gained`] →
//!   `show_keyboard`
//! - focus was explicitly cleared, or the focused field left the composition →
//!   [`notify_text_input_focus_lost`] → `hide_keyboard`
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

use std::cell::{Cell, RefCell};
use std::rc::Rc;

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

/// Per-app-context storage for the installed platform handler.
pub(crate) struct PlatformTextInputState {
    handler: RefCell<Option<Rc<dyn PlatformTextInputHandler>>>,
    /// Whether the framework has asked the platform to show the keyboard and
    /// not yet asked it to hide. Gates `hide_keyboard` so repeated
    /// "no field focused" checks forward at most one hide per shown keyboard.
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

/// Notifies the platform that a text field gained focus.
///
/// Called by the text-field focus manager after the focus transition has been
/// recorded, so the platform callback observes consistent focus state.
pub(crate) fn notify_text_input_focus_gained() {
    let handler = crate::render_state::with_text_input_session(|state| {
        let handler = state.handler();
        if handler.is_some() {
            state.keyboard_requested.set(true);
        }
        handler
    });
    // Invoke outside the state borrow: the platform callback may re-enter the
    // framework (e.g. logging hooks or JNI callbacks that pump events).
    if let Some(handler) = handler {
        handler.show_keyboard();
    }
}

/// Notifies the platform that no text field is focused anymore.
///
/// Forwarded to the platform only when a keyboard request is outstanding, so
/// this is safe to call repeatedly (the focus manager calls it from lazy
/// stale-focus detection on every key event without a focused field).
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell as StdRefCell;

    #[derive(Default)]
    struct RecordingHandler {
        calls: StdRefCell<Vec<&'static str>>,
    }

    impl PlatformTextInputHandler for RecordingHandler {
        fn show_keyboard(&self) {
            self.calls.borrow_mut().push("show");
        }

        fn hide_keyboard(&self) {
            self.calls.borrow_mut().push("hide");
        }
    }

    fn install_recording_handler() -> Rc<RecordingHandler> {
        let handler = Rc::new(RecordingHandler::default());
        set_platform_text_input_handler(handler.clone());
        handler
    }

    #[test]
    fn focus_gained_shows_keyboard() {
        let _app_context = crate::render_state::app_context_test_scope();
        let handler = install_recording_handler();

        notify_text_input_focus_gained();

        assert_eq!(*handler.calls.borrow(), vec!["show"]);
    }

    #[test]
    fn focus_lost_hides_keyboard_once() {
        let _app_context = crate::render_state::app_context_test_scope();
        let handler = install_recording_handler();

        notify_text_input_focus_gained();
        notify_text_input_focus_lost();
        // Stale-focus detection can fire "lost" repeatedly; only one hide
        // should reach the platform.
        notify_text_input_focus_lost();

        assert_eq!(*handler.calls.borrow(), vec!["show", "hide"]);
    }

    #[test]
    fn focus_lost_without_prior_show_is_not_forwarded() {
        let _app_context = crate::render_state::app_context_test_scope();
        let handler = install_recording_handler();

        notify_text_input_focus_lost();

        assert!(handler.calls.borrow().is_empty());
    }

    #[test]
    fn repeated_focus_gain_reshows_keyboard() {
        let _app_context = crate::render_state::app_context_test_scope();
        let handler = install_recording_handler();

        // Tapping an already-focused field must re-request the keyboard: the
        // user may have dismissed it without the framework knowing.
        notify_text_input_focus_gained();
        notify_text_input_focus_gained();

        assert_eq!(*handler.calls.borrow(), vec!["show", "show"]);
    }

    #[test]
    fn notifications_without_handler_are_noops() {
        let _app_context = crate::render_state::app_context_test_scope();
        notify_text_input_focus_gained();
        notify_text_input_focus_lost();
    }

    #[test]
    fn clearing_handler_stops_notifications() {
        let _app_context = crate::render_state::app_context_test_scope();
        let handler = install_recording_handler();

        notify_text_input_focus_gained();
        clear_platform_text_input_handler();
        notify_text_input_focus_lost();

        assert_eq!(*handler.calls.borrow(), vec!["show"]);
    }
}
