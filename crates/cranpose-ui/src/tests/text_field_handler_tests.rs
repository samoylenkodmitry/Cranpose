use std::{cell::RefCell, sync::Arc};

use cranpose_core::{DefaultScheduler, Runtime};

use super::*;
use crate::{
    key_event::{KeyCode, KeyEvent, KeyEventType, Modifiers},
    text_field_focus,
};

fn with_test_runtime<T>(f: impl FnOnce() -> T) -> T {
    let _runtime = Runtime::new(Arc::new(DefaultScheduler));
    f()
}

fn key_down(key_code: KeyCode, text: &str) -> KeyEvent {
    KeyEvent::new(key_code, text, Modifiers::NONE, KeyEventType::KeyDown)
}

fn focused_state(
    initial: &str,
    line_limits: TextFieldLineLimits,
) -> (TextFieldState, Rc<RefCell<bool>>) {
    let state = TextFieldState::new(initial);
    let handler = TextFieldHandler::new(
        state,
        None,
        line_limits,
        CaretGeometryRefs {
            node_origin: Rc::new(Cell::new(Point { x: 0.0, y: 0.0 })),
            content_offset: Rc::new(Cell::new(0.0)),
            content_y_offset: Rc::new(Cell::new(0.0)),
            scroll_offset: Rc::new(Cell::new(0.0)),
            style: TextStyle::default(),
        },
    );
    let focus = Rc::new(RefCell::new(false));
    text_field_focus::request_focus(focus.clone(), handler, 0);
    (state, focus)
}

#[test]
fn caret_geometry_exposed_for_single_visual_line() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let (_state, focus) = focused_state(
            "abc",
            TextFieldLineLimits::MultiLine {
                min_lines: 1,
                max_lines: 3,
            },
        );
        let geom = text_field_focus::focused_caret_geometry().expect("caret geometry");
        assert_eq!(geom.caret_xs.len(), "abc".chars().count() + 1);
        drop(focus);
    });

    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let (_state, focus) = focused_state(
            "a\nb",
            TextFieldLineLimits::MultiLine {
                min_lines: 1,
                max_lines: 3,
            },
        );
        assert!(text_field_focus::focused_caret_geometry().is_none());
        drop(focus);
    });
}

#[test]
fn android_style_key_events_commit_text() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let (state, _focus) = focused_state(
            "",
            TextFieldLineLimits::MultiLine {
                min_lines: 1,
                max_lines: usize::MAX,
            },
        );

        assert!(text_field_focus::dispatch_key_event(&key_down(
            KeyCode::H,
            "h"
        )));
        assert!(text_field_focus::dispatch_key_event(&key_down(
            KeyCode::I,
            "i"
        )));
        assert!(text_field_focus::dispatch_key_event(&key_down(
            KeyCode::Unknown,
            "\u{00f6}"
        )));
        assert_eq!(state.text(), "hi\u{00f6}");

        let key_up = KeyEvent::new(KeyCode::H, "h", Modifiers::NONE, KeyEventType::KeyUp);
        assert!(!text_field_focus::dispatch_key_event(&key_up));
        assert_eq!(state.text(), "hi\u{00f6}");

        assert!(text_field_focus::dispatch_key_event(&key_down(
            KeyCode::Backspace,
            ""
        )));
        assert_eq!(state.text(), "hi");

        assert!(text_field_focus::dispatch_key_event(&key_down(
            KeyCode::Enter,
            ""
        )));
        assert_eq!(state.text(), "hi\n");

        text_field_focus::clear_focus();
    });
}

#[test]
fn select_all_dispatch_selects_entire_field() {
    use cranpose_foundation::text::TextRange;
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let (state, _focus) = focused_state("hello world", TextFieldLineLimits::SingleLine);
        state.edit(cranpose_foundation::text::TextFieldBuffer::place_cursor_at_end);

        assert!(text_field_focus::dispatch_select_all());
        assert_eq!(state.selection(), TextRange::new(0, "hello world".len()));

        text_field_focus::clear_focus();
        assert!(!text_field_focus::dispatch_select_all());
    });
}

#[test]
fn clipboard_fallback_round_trips() {
    use crate::clipboard_session::{clipboard_read_text, clipboard_write_text};
    let _app_context = crate::render_state::app_context_test_scope();
    assert_eq!(clipboard_read_text(), None);
    clipboard_write_text("copied text");
    assert_eq!(clipboard_read_text(), Some("copied text".to_string()));
}

#[test]
fn spacebar_swipe_arrow_keys_scrub_caret() {
    use cranpose_foundation::text::TextRange;
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let (state, _focus) = focused_state("hello", TextFieldLineLimits::SingleLine);
        state.edit(cranpose_foundation::text::TextFieldBuffer::place_cursor_at_end);
        assert_eq!(state.selection(), TextRange::new(5, 5));

        assert!(text_field_focus::dispatch_key_event(&key_down(
            KeyCode::ArrowLeft,
            ""
        )));
        assert_eq!(state.selection(), TextRange::new(4, 4));
        assert!(text_field_focus::dispatch_key_event(&key_down(
            KeyCode::ArrowLeft,
            ""
        )));
        assert_eq!(state.selection(), TextRange::new(3, 3));

        assert!(text_field_focus::dispatch_key_event(&key_down(
            KeyCode::ArrowRight,
            ""
        )));
        assert_eq!(state.selection(), TextRange::new(4, 4));

        assert_eq!(state.text(), "hello");
        text_field_focus::clear_focus();
    });
}

#[test]
fn cursor_move_via_set_selection_resets_blink_to_solid() {
    use crate::cursor_animation::{BLINK_INTERVAL_MS, is_cursor_visible, start_cursor_blink};
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let (state, _focus) = focused_state("hello world", TextFieldLineLimits::SingleLine);
        state.edit(cranpose_foundation::text::TextFieldBuffer::place_cursor_at_end);

        start_cursor_blink();
        let hidden_at =
            web_time::Instant::now() + std::time::Duration::from_millis(BLINK_INTERVAL_MS + 1);
        crate::render_state::with_cursor_animation(|state| state.tick(hidden_at));
        assert!(!is_cursor_visible(), "caret should be hidden mid-blink");

        assert!(text_field_focus::dispatch_ime_set_selection(2, 2));
        assert!(
            is_cursor_visible(),
            "a cursor-move must reset the blink phase to solid (visible)"
        );

        text_field_focus::clear_focus();
    });
}

#[test]
fn ime_set_selection_scrubs_caret_without_editing() {
    use cranpose_foundation::text::TextRange;
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let (state, _focus) = focused_state("hello world", TextFieldLineLimits::SingleLine);
        state.edit(cranpose_foundation::text::TextFieldBuffer::place_cursor_at_end);

        assert!(text_field_focus::dispatch_ime_set_selection(2, 2));
        assert_eq!(state.selection(), TextRange::new(2, 2));
        assert_eq!(state.text(), "hello world");

        assert!(text_field_focus::dispatch_ime_set_selection(0, 5));
        assert_eq!(state.selection(), TextRange::new(0, 5));

        text_field_focus::clear_focus();
        assert!(!text_field_focus::dispatch_ime_set_selection(1, 1));
    });
}

#[test]
fn successive_preedit_updates_replace_previous_composition() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let (state, _focus) = focused_state("ab", TextFieldLineLimits::SingleLine);
        state.edit(cranpose_foundation::text::TextFieldBuffer::place_cursor_at_end);

        assert!(text_field_focus::dispatch_ime_preedit("k", Some((1, 1))));
        assert_eq!(state.text(), "abk");
        assert!(text_field_focus::dispatch_ime_preedit("ka", Some((2, 2))));
        assert_eq!(state.text(), "abka");
        assert!(text_field_focus::dispatch_ime_preedit(
            "\u{304b}",
            Some((3, 3))
        ));
        assert_eq!(state.text(), "ab\u{304b}");
        let comp = state.composition().expect("composition active");
        assert_eq!((comp.min(), comp.max()), (2, 2 + "\u{304b}".len()));

        assert!(text_field_focus::dispatch_ime_preedit("", None));
        assert_eq!(state.text(), "ab");
        assert!(text_field_focus::dispatch_paste("\u{304b}\u{306a}"));
        assert_eq!(state.text(), "ab\u{304b}\u{306a}");
        assert_eq!(state.composition(), None);

        text_field_focus::clear_focus();
    });
}

#[test]
fn finish_composition_keeps_text() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let (state, _focus) = focused_state("", TextFieldLineLimits::SingleLine);

        assert!(text_field_focus::dispatch_ime_preedit("hello", None));
        assert_eq!(state.text(), "hello");
        assert!(state.composition().is_some());

        assert!(text_field_focus::dispatch_ime_finish_composing());
        assert_eq!(state.text(), "hello");
        assert_eq!(state.composition(), None);

        assert!(text_field_focus::dispatch_ime_finish_composing());
        assert_eq!(state.text(), "hello");

        text_field_focus::clear_focus();
    });
}

#[test]
fn set_composing_region_marks_text_without_changing_it() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let (state, _focus) = focused_state("hello world", TextFieldLineLimits::SingleLine);

        assert!(text_field_focus::dispatch_ime_set_composing_region(0, 5));
        assert_eq!(state.text(), "hello world");
        let comp = state.composition().expect("composition active");
        assert_eq!((comp.min(), comp.max()), (0, 5));

        assert!(text_field_focus::dispatch_ime_preedit("Hello", None));
        assert_eq!(state.text(), "Hello world");

        assert!(text_field_focus::dispatch_ime_preedit("", None));
        state.edit(|buffer| {
            buffer.clear();
            buffer.insert("\u{00e9}x");
        });
        assert!(text_field_focus::dispatch_ime_set_composing_region(1, 3));
        let comp = state.composition().expect("composition active");
        assert_eq!((comp.min(), comp.max()), (0, 3));

        assert!(text_field_focus::dispatch_ime_set_composing_region(1, 1));
        assert_eq!(state.composition(), None);
        assert_eq!(state.text(), "\u{00e9}x");

        text_field_focus::clear_focus();
    });
}

#[test]
fn editor_state_reflects_text_selection_and_composition() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let (state, _focus) = focused_state("hi", TextFieldLineLimits::SingleLine);
        state.edit(cranpose_foundation::text::TextFieldBuffer::place_cursor_at_end);

        let snapshot = text_field_focus::focused_editor_state().expect("focused field");
        assert_eq!(snapshot.text, "hi");
        assert_eq!(
            (snapshot.selection_start, snapshot.selection_end),
            (2usize, 2usize)
        );
        assert_eq!(snapshot.composition, None);
        assert!(snapshot.single_line);

        assert!(text_field_focus::dispatch_ime_preedit("ab", None));
        let snapshot = text_field_focus::focused_editor_state().expect("focused field");
        assert_eq!(snapshot.text, "hiab");
        assert_eq!(snapshot.composition, Some((2, 4)));

        text_field_focus::clear_focus();
        assert_eq!(text_field_focus::focused_editor_state(), None);
    });
}

#[test]
fn enter_on_single_line_field_is_not_consumed() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let (state, _focus) = focused_state("abc", TextFieldLineLimits::SingleLine);

        assert!(!text_field_focus::dispatch_key_event(&key_down(
            KeyCode::Enter,
            ""
        )));
        assert_eq!(state.text(), "abc");

        text_field_focus::clear_focus();
    });
}
