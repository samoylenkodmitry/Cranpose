// An app selects text in a `SelectionContainer` by a range of the
// container's text, through the public shell.

use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_testing::robot::TestRenderer;
use cranpose_ui::{
    Column, ColumnSpec, Modifier, Text, TextRange, TextStyle,
    selection_container::SelectionRegistrar,
    widgets::{SelectionContainer, local_selection_registrar},
};

type Seen = Rc<RefCell<Option<SelectionRegistrar>>>;

// A container showing `texts` one under another, which reports its
// registrar to `seen`.
fn container(texts: &'static [&'static str], seen: Seen) {
    SelectionContainer(Modifier::empty(), move || {
        *seen.borrow_mut() = local_selection_registrar().current();
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            for text in texts {
                Text(*text, Modifier::empty(), TextStyle::default());
            }
        });
    });
}

// Composes `content` and lays it out once.
fn shown(content: impl FnMut() + 'static) -> AppShell<TestRenderer> {
    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        content,
    );
    shell.set_viewport(400.0, 300.0);
    shell.set_buffer_size(400, 300);
    shell.update_at_frame_time_nanos(16_000_000);
    shell
}

// The registrar of a container showing `texts`, with the shell that keeps
// it composed.
fn registrar(texts: &'static [&'static str]) -> (AppShell<TestRenderer>, SelectionRegistrar) {
    let seen = Seen::default();
    let reported = Rc::clone(&seen);
    let shell = shown(move || container(texts, Rc::clone(&reported)));
    let registrar = seen
        .borrow()
        .clone()
        .expect("the container reported its registrar");
    (shell, registrar)
}

#[test]
fn a_range_of_the_container_text_selects_across_its_texts() {
    let (_shell, registrar) = registrar(&["First line", "Second"]);
    assert_eq!(registrar.text(), "First line\nSecond");
    registrar.select(TextRange::new(6, 14));
    assert_eq!(registrar.selected_text(), "line\nSec");
}

#[test]
fn a_backward_range_selects_the_same_text_from_its_end() {
    let (_shell, registrar) = registrar(&["First line", "Second"]);
    registrar.select(TextRange::new(14, 6));
    assert_eq!(registrar.selected_text(), "line\nSec");
    let selection = registrar.selection().expect("the range selected");
    assert_eq!(
        (selection.anchor.offset, selection.focus.offset),
        (3, 6),
        "the selection starts after \"Sec\" and ends before \"line\""
    );
}

#[test]
fn offsets_inside_characters_and_past_the_end_select_whole_characters() {
    let (_shell, registrar) = registrar(&["日本語", "мир"]);
    registrar.select(TextRange::new(1, 11));
    assert_eq!(
        registrar.selected_text(),
        "日本語",
        "both ends move to the start of the character they fall inside"
    );
    registrar.select(TextRange::new(4, 1_000));
    assert_eq!(registrar.selected_text(), "本語\nмир");
}

#[test]
fn a_selection_made_in_one_container_clears_the_other() {
    let (first, second) = (Seen::default(), Seen::default());
    let reported = (Rc::clone(&first), Rc::clone(&second));
    let _shell = shown(move || {
        let (first, second) = (Rc::clone(&reported.0), Rc::clone(&reported.1));
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            container(&["Upper words"], Rc::clone(&first));
            container(&["Lower words"], Rc::clone(&second));
        });
    });
    let (first, second) = (
        first
            .borrow()
            .clone()
            .expect("the upper container reported"),
        second
            .borrow()
            .clone()
            .expect("the lower container reported"),
    );
    first.select(TextRange::new(0, 5));
    assert_eq!(first.selected_text(), "Upper");
    second.select(TextRange::new(6, 11));
    assert_eq!(second.selected_text(), "words");
    assert_eq!(first.selection(), None, "the window holds one selection");
}
