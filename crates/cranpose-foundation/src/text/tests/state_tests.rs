use std::sync::Arc;

use cranpose_core::{DefaultScheduler, Runtime};

use super::*;

fn with_test_runtime<T>(f: impl FnOnce() -> T) -> T {
    let _runtime = Runtime::new(Arc::new(DefaultScheduler));
    f()
}

#[test]
fn flushing_an_undo_group_is_harmless_when_nothing_is_pending() {
    with_test_runtime(|| {
        let state = TextFieldState::new("Hello");
        state.flush_undo_group();
        state.flush_undo_group();
        assert_eq!(state.text(), "Hello");
    });
}

#[test]
fn flushing_an_undo_group_breaks_the_coalescing_between_two_edits() {
    with_test_runtime(|| {
        let state = TextFieldState::new("");
        state.edit(|buffer| buffer.insert("ab"));
        state.flush_undo_group();
        state.edit(|buffer| buffer.insert("cd"));
        assert_eq!(state.text(), "abcd");

        state.undo();
        assert_eq!(
            state.text(),
            "ab",
            "the flush did not break the two edits apart"
        );
    });
}

#[test]
fn undo_skips_cursor_moves_and_restores_the_text_before_the_typing() {
    with_test_runtime(|| {
        let state = TextFieldState::new("");
        state.edit(|buffer| buffer.insert("a"));
        state.edit(|buffer| buffer.insert("b"));
        state.edit(TextFieldBuffer::move_cursor_left);
        state.edit(|buffer| buffer.insert("c"));
        assert_eq!(state.text(), "acb");

        state.undo();
        assert_eq!(state.text(), "ab", "the first undo takes back the c");
        state.undo();
        assert_eq!(
            state.text(),
            "",
            "the second undo takes back the typing, not the cursor move"
        );
        assert!(!state.can_undo());
    });
}

#[test]
fn a_cursor_move_keeps_the_redo_history() {
    with_test_runtime(|| {
        let state = TextFieldState::new("");
        state.edit(|buffer| buffer.insert("ab"));
        state.undo();
        state.edit(TextFieldBuffer::place_cursor_at_start);
        assert!(state.redo(), "moving the cursor dropped the redo");
        assert_eq!(state.text(), "ab");
    });
}

#[test]
fn new_state_has_cursor_at_end() {
    with_test_runtime(|| {
        let state = TextFieldState::new("Hello");
        assert_eq!(state.text(), "Hello");
        assert_eq!(state.selection(), TextRange::cursor(5));
    });
}

#[test]
fn edit_updates_text() {
    with_test_runtime(|| {
        let state = TextFieldState::new("Hello");
        state.edit(|buffer| {
            buffer.place_cursor_at_end();
            buffer.insert(", World!");
        });
        assert_eq!(state.text(), "Hello, World!");
    });
}

#[test]
fn edit_updates_selection() {
    with_test_runtime(|| {
        let state = TextFieldState::new("Hello");
        state.edit(|buffer| {
            buffer.select_all();
        });
        assert_eq!(state.selection(), TextRange::new(0, 5));
    });
}

#[test]
fn set_text_replaces_content() {
    with_test_runtime(|| {
        let state = TextFieldState::new("Hello");
        state.set_text("Goodbye");
        assert_eq!(state.text(), "Goodbye");
        assert_eq!(state.selection(), TextRange::cursor(7));
    });
}

#[test]
fn nested_edit_is_rejected() {
    with_test_runtime(|| {
        use std::{cell::Cell, rc::Rc};

        let state = TextFieldState::new("Hello");
        let state_clone = state;
        let nested_result = Rc::new(Cell::new(true));
        let nested_result_for_edit = Rc::clone(&nested_result);
        let outer_result = state.edit(move |_buffer| {
            nested_result_for_edit.set(state_clone.edit(|_| {}));
        });
        assert!(outer_result);
        assert!(!nested_result.get());
    });
}

#[test]
fn listener_is_called_on_change() {
    with_test_runtime(|| {
        use std::{cell::Cell, rc::Rc};

        let state = TextFieldState::new("Hello");
        let called = Rc::new(Cell::new(false));
        let called_clone = Rc::clone(&called);

        state.add_listener(move |_value| {
            called_clone.set(true);
        });

        state.edit(|buffer| {
            buffer.insert("!");
        });

        assert!(called.get());
    });
}
