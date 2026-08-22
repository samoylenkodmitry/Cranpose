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
    // Same effect as losing focus, but semantically "the app went away": the
    // field may still be focused, we simply must not leave a shown-keyboard
    // request dangling across the pause.
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
/// (which re-requests it through [`notify_text_input_focus_gained`]).
///
/// Always returns `false` so the platform runtime force-hides the OS-restored
/// keyboard. Pruning stale focus here keeps the keyboard-request bookkeeping
/// consistent (a focused-but-detached field is dropped and its outstanding
/// request withdrawn) without ever calling `show`.
pub fn notify_app_resumed() -> bool {
    // Prune stale focus (a detached field withdraws its keyboard request), but
    // never re-request the keyboard: resume must leave it hidden.
    let _ = crate::text_field_focus::has_focused_field();
    false
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

    struct NoopFocusHandler;
    impl crate::text_field_focus::FocusedTextFieldHandler for NoopFocusHandler {
        fn handle_key(&self, _: &crate::key_event::KeyEvent) -> bool {
            false
        }
        fn insert_text(&self, _: &str) {}
        fn delete_surrounding(&self, _: usize, _: usize) {}
        fn copy_selection(&self) -> Option<String> {
            None
        }
        fn cut_selection(&self) -> Option<String> {
            None
        }
        fn set_composition(&self, _: &str, _: Option<(usize, usize)>) {}
    }

    fn focus_a_field() -> Rc<std::cell::RefCell<bool>> {
        let focus = Rc::new(std::cell::RefCell::new(false));
        crate::text_field_focus::request_focus(Rc::clone(&focus), Rc::new(NoopFocusHandler), 0);
        focus
    }

    #[test]
    fn resume_without_a_focused_field_does_not_show_the_keyboard() {
        let _app_context = crate::render_state::app_context_test_scope();
        let handler = install_recording_handler();

        // The soft keyboard was shown earlier and the app was paused (hidden).
        // Coming back to the foreground with nothing focused must NOT re-show
        // it — this is the reported "keyboard re-opens on resume" bug.
        notify_text_input_focus_gained();
        notify_app_paused();
        assert_eq!(*handler.calls.borrow(), vec!["show", "hide"]);

        assert!(!notify_app_resumed());
        assert_eq!(
            *handler.calls.borrow(),
            vec!["show", "hide"],
            "resume with no focused field must not re-show the keyboard"
        );
    }

    #[test]
    fn resume_never_reshows_the_keyboard_even_for_a_focused_field() {
        let _app_context = crate::render_state::app_context_test_scope();
        let handler = install_recording_handler();

        // A focused field shows the keyboard.
        let focus = focus_a_field();
        assert_eq!(*handler.calls.borrow(), vec!["show"]);

        // Pause withdraws it.
        notify_app_paused();
        assert_eq!(*handler.calls.borrow(), vec!["show", "hide"]);

        // Resume must NOT bring it back even though the field is still focused
        // (the reported warm-resume bug): the keyboard stays hidden until the
        // user taps the field again. Resume reports `false` so the platform
        // force-hides the OS-restored keyboard.
        assert!(!notify_app_resumed());
        assert_eq!(
            *handler.calls.borrow(),
            vec!["show", "hide"],
            "resume must leave the keyboard hidden"
        );

        // Tapping the still-focused field re-requests the keyboard.
        crate::text_field_focus::request_focus(Rc::clone(&focus), Rc::new(NoopFocusHandler), 0);
        assert_eq!(
            *handler.calls.borrow(),
            vec!["show", "hide", "show"],
            "tapping the field after resume re-shows the keyboard"
        );

        crate::text_field_focus::clear_focus();
    }

    #[test]
    fn cold_start_with_no_focus_does_not_show_the_keyboard() {
        // Bug 5: on a fresh launch the platform runtime calls `notify_app_resumed`
        // once and only re-shows the keyboard when a field is actually focused.
        // With nothing focused (a brand-new app context, e.g. a cold restart) it
        // must return false and never call `show`, so the runtime knows to force
        // the OS-restored keyboard hidden instead.
        let _app_context = crate::render_state::app_context_test_scope();
        let handler = install_recording_handler();

        assert!(
            !notify_app_resumed(),
            "a launch with no focused field must not re-open the keyboard"
        );
        assert!(
            handler.calls.borrow().is_empty(),
            "no platform show/hide should be requested for an unfocused cold start"
        );
    }

    #[test]
    fn pause_is_a_noop_when_the_keyboard_was_not_showing() {
        let _app_context = crate::render_state::app_context_test_scope();
        let handler = install_recording_handler();

        notify_app_paused();
        assert!(
            handler.calls.borrow().is_empty(),
            "pausing without a shown keyboard must not call the platform"
        );
    }
}
