use std::cell::RefCell as StdRefCell;

use super::*;

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

    let focus = focus_a_field();
    assert_eq!(*handler.calls.borrow(), vec!["show"]);

    notify_app_paused();
    assert_eq!(*handler.calls.borrow(), vec!["show", "hide"]);

    assert!(!notify_app_resumed());
    assert_eq!(
        *handler.calls.borrow(),
        vec!["show", "hide"],
        "resume must leave the keyboard hidden"
    );

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
