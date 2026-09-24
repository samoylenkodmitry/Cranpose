use super::*;

struct MockHandler;
impl FocusedTextFieldHandler for MockHandler {
    fn handle_key(&self, _: &KeyEvent) -> bool {
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

fn mock_handler() -> Rc<dyn FocusedTextFieldHandler> {
    Rc::new(MockHandler)
}

struct NodeBackedHandler(cranpose_core::NodeId);
impl FocusedTextFieldHandler for NodeBackedHandler {
    fn node_id(&self) -> Option<cranpose_core::NodeId> {
        Some(self.0)
    }
    fn handle_key(&self, _: &KeyEvent) -> bool {
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

#[test]
fn focus_transitions_schedule_scoped_draw_repasses_on_both_fields() {
    let _app_context = crate::render_state::app_context_test_scope();
    let _ = crate::render_state::take_draw_repass_nodes();

    let first = Rc::new(RefCell::new(false));
    request_focus(first, Rc::new(NodeBackedHandler(7)), 0);
    assert!(
        crate::render_state::take_draw_repass_nodes().contains(&7),
        "gaining focus must re-record the gaining field's draws"
    );

    let second = Rc::new(RefCell::new(false));
    request_focus(second, Rc::new(NodeBackedHandler(9)), 0);
    let repasses = crate::render_state::take_draw_repass_nodes();
    assert!(
        repasses.contains(&7) && repasses.contains(&9),
        "a focus hand-off must re-record both fields, got {repasses:?}"
    );

    clear_focus();
    assert!(
        crate::render_state::take_draw_repass_nodes().contains(&9),
        "losing focus must re-record the field that had the caret"
    );
}

#[test]
fn a_blink_transition_schedules_a_scoped_repass_on_the_focused_field() {
    let _app_context = crate::render_state::app_context_test_scope();
    let focus = Rc::new(RefCell::new(false));
    request_focus(focus, Rc::new(NodeBackedHandler(21)), 0);
    let _ = crate::render_state::take_draw_repass_nodes();

    let past_interval = web_time::Instant::now()
        + crate::cursor_animation::CursorAnimationState::BLINK_INTERVAL
        + std::time::Duration::from_millis(1);
    assert!(
        crate::cursor_animation::tick_cursor_blink_at(past_interval),
        "the tick past the interval must flip visibility"
    );
    assert!(
        crate::render_state::take_draw_repass_nodes().contains(&21),
        "the flip must re-record the focused field's draws"
    );
    clear_focus();
}

#[test]
fn request_focus_sets_flag() {
    let _app_context = crate::render_state::app_context_test_scope();
    let focus = Rc::new(RefCell::new(false));
    request_focus(focus.clone(), mock_handler(), 0);
    assert!(*focus.borrow());
    clear_focus();
}

#[test]
fn request_focus_clears_previous() {
    let _app_context = crate::render_state::app_context_test_scope();
    let focus1 = Rc::new(RefCell::new(false));
    let focus2 = Rc::new(RefCell::new(false));

    request_focus(focus1.clone(), mock_handler(), 0);
    assert!(*focus1.borrow());

    request_focus(focus2.clone(), mock_handler(), 0);
    assert!(!*focus1.borrow());
    assert!(*focus2.borrow());
    clear_focus();
}

#[test]
fn clear_focus_unfocuses_current() {
    let _app_context = crate::render_state::app_context_test_scope();
    let focus = Rc::new(RefCell::new(false));
    request_focus(focus.clone(), mock_handler(), 0);
    assert!(*focus.borrow());

    clear_focus();
    assert!(!*focus.borrow());
}

#[derive(Default)]
struct DispatchRecordingHandler {
    key_count: Cell<usize>,
    insert_count: Cell<usize>,
    delete_count: Cell<usize>,
    copy_count: Cell<usize>,
    cut_count: Cell<usize>,
    preedit_count: Cell<usize>,
    last_delete: Cell<Option<(usize, usize)>>,
}

impl DispatchRecordingHandler {
    fn bump(cell: &Cell<usize>) {
        cell.set(cell.get() + 1);
    }

    fn total_calls(&self) -> usize {
        self.key_count.get()
            + self.insert_count.get()
            + self.delete_count.get()
            + self.copy_count.get()
            + self.cut_count.get()
            + self.preedit_count.get()
    }
}

impl FocusedTextFieldHandler for DispatchRecordingHandler {
    fn handle_key(&self, _: &KeyEvent) -> bool {
        Self::bump(&self.key_count);
        true
    }

    fn insert_text(&self, _: &str) {
        Self::bump(&self.insert_count);
    }

    fn delete_surrounding(&self, before_bytes: usize, after_bytes: usize) {
        Self::bump(&self.delete_count);
        self.last_delete.set(Some((before_bytes, after_bytes)));
    }

    fn copy_selection(&self) -> Option<String> {
        Self::bump(&self.copy_count);
        Some("copy".to_string())
    }

    fn cut_selection(&self) -> Option<String> {
        Self::bump(&self.cut_count);
        Some("cut".to_string())
    }

    fn set_composition(&self, _: &str, _: Option<(usize, usize)>) {
        Self::bump(&self.preedit_count);
    }
}

#[test]
fn dispatch_delete_surrounding_calls_handler() {
    let _app_context = crate::render_state::app_context_test_scope();
    let focus = Rc::new(RefCell::new(false));
    let handler = Rc::new(DispatchRecordingHandler::default());

    request_focus(Rc::clone(&focus), handler.clone(), 0);
    assert!(dispatch_delete_surrounding(3, 1));
    assert_eq!(handler.last_delete.get(), Some((3, 1)));

    clear_focus();
}

#[test]
fn dispatch_clears_stale_focus_owner_before_invoking_handler() {
    let _app_context = crate::render_state::app_context_test_scope();
    let handler = Rc::new(DispatchRecordingHandler::default());

    {
        let focus = Rc::new(RefCell::new(false));
        request_focus(Rc::clone(&focus), handler.clone(), 0);
        assert!(has_focused_field());
    }

    let key_event = KeyEvent::key_down(crate::key_event::KeyCode::A, "a");

    assert!(!dispatch_key_event(&key_event));
    assert!(!dispatch_paste("stale paste"));
    assert!(!dispatch_delete_surrounding(2, 1));
    assert_eq!(dispatch_copy(), None);
    assert_eq!(dispatch_cut(), None);
    assert!(!dispatch_ime_preedit("preedit", Some((1, 1))));
    assert!(!has_focused_field());
    assert_eq!(
        handler.total_calls(),
        0,
        "stale focused-field handlers must not receive input"
    );
}

#[test]
fn stale_focus_cleanup_after_hidden_blink_does_not_reenter_the_focus_registry_borrow() {
    let _app_context = crate::render_state::app_context_test_scope();

    let focus = Rc::new(RefCell::new(false));
    request_focus(focus.clone(), Rc::new(NodeBackedHandler(3)), 0);

    let past_interval = web_time::Instant::now()
        + crate::cursor_animation::CursorAnimationState::BLINK_INTERVAL
        + std::time::Duration::from_millis(1);
    assert!(
        crate::cursor_animation::tick_cursor_blink_at(past_interval),
        "the blink must already have toggled to hidden, matching the real \
         timing where the bug's stop_cursor_blink() call is a visibility \
         change and therefore reaches invalidate_focused_caret()"
    );

    drop(focus);

    assert!(
        !has_focused_field(),
        "a field dropped without clear_focus must read back as unfocused \
         instead of panicking on a reentrant borrow of focused_field"
    );
}

#[test]
fn stale_focus_cleanup_repasses_the_node_that_lost_its_caret() {
    let _app_context = crate::render_state::app_context_test_scope();
    let _ = crate::render_state::take_draw_repass_nodes();

    let focus = Rc::new(RefCell::new(false));
    request_focus(focus.clone(), Rc::new(NodeBackedHandler(11)), 0);
    let _ = crate::render_state::take_draw_repass_nodes();

    drop(focus);

    assert!(!has_focused_field());
    assert!(
        crate::render_state::take_draw_repass_nodes().contains(&11),
        "discovering a stale field lazily must repass its node just like \
         an explicit clear_focus does, or its caret is left stale on screen"
    );
}

#[derive(Default)]
struct KeyboardProbe {
    calls: RefCell<Vec<&'static str>>,
}

impl crate::text_input_session::PlatformTextInputHandler for KeyboardProbe {
    fn show_keyboard(&self) {
        self.calls.borrow_mut().push("show");
    }

    fn hide_keyboard(&self) {
        self.calls.borrow_mut().push("hide");
    }
}

#[test]
fn focus_transitions_drive_platform_keyboard() {
    let _app_context = crate::render_state::app_context_test_scope();
    let keyboard = Rc::new(KeyboardProbe::default());
    crate::text_input_session::set_platform_text_input_handler(keyboard.clone());

    let focus = Rc::new(RefCell::new(false));
    request_focus(focus.clone(), mock_handler(), 0);
    assert_eq!(*keyboard.calls.borrow(), vec!["show"]);

    request_focus(focus, mock_handler(), 0);
    assert_eq!(*keyboard.calls.borrow(), vec!["show", "show"]);

    clear_focus();
    assert_eq!(*keyboard.calls.borrow(), vec!["show", "show", "hide"]);
}

#[test]
fn stale_focus_detection_hides_platform_keyboard() {
    let _app_context = crate::render_state::app_context_test_scope();
    let keyboard = Rc::new(KeyboardProbe::default());
    crate::text_input_session::set_platform_text_input_handler(keyboard.clone());

    {
        let focus = Rc::new(RefCell::new(false));
        request_focus(focus, mock_handler(), 0);
    }

    assert!(!has_focused_field());
    assert_eq!(*keyboard.calls.borrow(), vec!["show", "hide"]);

    assert!(!has_focused_field());
    assert_eq!(*keyboard.calls.borrow(), vec!["show", "hide"]);
}

#[test]
fn text_field_focus_is_scoped_by_app_context() {
    let _app_context = crate::render_state::app_context_test_scope();
    let first = crate::render_state::AppContext::new_with_density(1.0);
    let second = crate::render_state::AppContext::new_with_density(1.0);
    let first_focus = Rc::new(RefCell::new(false));
    let second_focus = Rc::new(RefCell::new(false));

    first.enter(|| {
        request_focus(first_focus.clone(), mock_handler(), 0);
        assert!(has_focused_field());
        assert!(*first_focus.borrow());
    });

    second.enter(|| {
        assert!(!has_focused_field());
        request_focus(second_focus.clone(), mock_handler(), 0);
        assert!(has_focused_field());
        assert!(*second_focus.borrow());
    });

    first.enter(|| {
        assert!(has_focused_field());
        assert!(*first_focus.borrow());
        clear_focus();
        assert!(!has_focused_field());
        assert!(!*first_focus.borrow());
    });

    second.enter(|| {
        assert!(has_focused_field());
        assert!(*second_focus.borrow());
        clear_focus();
    });
}
