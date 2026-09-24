use std::sync::Arc;

use cranpose_core::{DefaultScheduler, Runtime};

use super::*;

fn with_test_runtime<T>(f: impl FnOnce() -> T) -> T {
    let _runtime = Runtime::new(Arc::new(DefaultScheduler));
    f()
}

#[test]
fn a_desired_column_is_remembered_until_it_is_cleared() {
    with_test_runtime(|| {
        let state = TextFieldState::new("Hello");
        assert_eq!(
            state.desired_column(),
            None,
            "a field nobody navigated vertically had a remembered column"
        );

        state.set_desired_column(Some(7));
        assert_eq!(state.desired_column(), Some(7));

        state.set_desired_column(None);
        assert_eq!(state.desired_column(), None);
    });
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
        let nested_result_for_edit = nested_result.clone();
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
        let called_clone = called.clone();

        state.add_listener(move |_value| {
            called_clone.set(true);
        });

        state.edit(|buffer| {
            buffer.insert("!");
        });

        assert!(called.get());
    });
}
