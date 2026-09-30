use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{location_key, remember, rememberMutableStateOf};
use cranpose_testing::robot::TestRenderer;
use cranpose_ui::{
    BasicTextField, Column, ColumnSpec, FocusRequester, KeyCode, KeyEvent, Modifier, TextFieldState,
};

type EventLog = Rc<RefCell<Vec<&'static str>>>;

fn new_shell(content: impl FnMut() + 'static) -> AppShell<TestRenderer> {
    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        content,
    );
    shell.set_viewport(320.0, 200.0);
    shell.set_buffer_size(320, 200);
    shell.update();
    shell
}

fn record(
    log: &EventLog,
    name: &'static str,
    consume: bool,
) -> impl Fn(&KeyEvent) -> bool + 'static {
    let log = log.clone();
    move |_| {
        log.borrow_mut().push(name);
        consume
    }
}

fn logged_shell(
    content: impl Fn(FocusRequester, EventLog) + 'static,
) -> (AppShell<TestRenderer>, FocusRequester, EventLog) {
    let focus = FocusRequester::new();
    let events = EventLog::default();
    let content_focus = focus.clone();
    let content_events = events.clone();
    let shell = new_shell(move || content(content_focus.clone(), content_events.clone()));
    (shell, focus, events)
}

#[cranpose_ui::composable]
fn editor(focus: Modifier, initial: &'static str) {
    let state = remember(|| TextFieldState::new(initial)).with(|state| *state);
    BasicTextField(
        state,
        Modifier::empty().width(200.0).height(40.0).then(focus),
        Default::default(),
    );
}

#[test]
fn ancestor_preview_consumes_shortcuts_before_the_focused_editor() {
    let focus = FocusRequester::new();
    let content_focus = focus.clone();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let content_seen = seen.clone();
    let mut shell = new_shell(move || {
        let seen = content_seen.clone();
        let focus = content_focus.clone();
        Column(
            Modifier::empty().on_preview_key_event(move |event| {
                seen.borrow_mut().push(event.key_code);
                event.key_code == KeyCode::Q
            }),
            ColumnSpec::default(),
            move || {
                editor(Modifier::empty().focus_requester(&focus), "");
            },
        );
    });
    focus.request_focus().expect("focus editor");
    assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::Q, "q")));
    assert_eq!(
        shell.ime_editor_state().expect("focused editor").text,
        "",
        "consumed shortcut must not edit text"
    );
    assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::A, "a")));
    assert_eq!(
        shell.ime_editor_state().expect("focused editor").text,
        "a",
        "unconsumed key must reach editor"
    );
    assert_eq!(*seen.borrow(), [KeyCode::Q, KeyCode::A]);
}

#[test]
fn keys_follow_nested_and_same_component_order_without_visiting_siblings() {
    let (mut shell, focus, log) = logged_shell(move |focus, log| {
        Column(
            Modifier::empty()
                .on_preview_key_event(record(&log, "outer preview", false))
                .on_preview_key_event(record(&log, "inner preview", false))
                .on_key_event(record(&log, "outer bubble", true))
                .on_key_event(record(&log, "inner bubble", false)),
            ColumnSpec::default(),
            move || {
                cranpose_ui::Box(
                    Modifier::empty()
                        .size(cranpose_ui::Size::new(100.0, 40.0))
                        .on_preview_key_event(record(&log, "child preview", false))
                        .focus_requester(&focus)
                        .focus_target()
                        .on_key_event(record(&log, "child bubble", false)),
                    cranpose_ui::BoxSpec::default(),
                    || {},
                );
                cranpose_ui::Box(
                    Modifier::empty().on_preview_key_event(record(&log, "sibling", true)),
                    cranpose_ui::BoxSpec::default(),
                    || {},
                );
            },
        );
    });
    focus.request_focus().expect("focus child");
    assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::A, "a")));
    assert_eq!(
        *log.borrow(),
        [
            "outer preview",
            "inner preview",
            "child preview",
            "child bubble",
            "inner bubble",
            "outer bubble"
        ]
    );
}

#[test]
fn updated_and_removed_handlers_take_effect_on_the_next_key() {
    let focus = FocusRequester::new();
    let content_focus = focus.clone();
    let log = EventLog::default();
    let content_log = log.clone();
    let version = Rc::new(RefCell::new(None));
    let content_version = version.clone();
    let mut shell = new_shell(move || {
        let current = rememberMutableStateOf(|| 0);
        *content_version.borrow_mut() = Some(current);
        let value = current.get();
        let modifier = if value < 2 {
            Modifier::empty().on_preview_key_event(record(
                &content_log,
                if value == 0 { "old" } else { "new" },
                true,
            ))
        } else if value == 2 {
            Modifier::empty().on_key_event(record(&content_log, "bubble", true))
        } else {
            Modifier::empty()
        };
        let focus = content_focus.clone();
        Column(modifier, ColumnSpec::default(), move || {
            editor(Modifier::empty().focus_requester(&focus), "");
        });
    });
    focus.request_focus().expect("focus editor");
    for value in 0..4 {
        version.borrow().expect("composed version").set(value);
        shell.update();
        assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::A, "a")));
    }
    assert_eq!(*log.borrow(), ["old", "new"]);
    assert_eq!(shell.ime_editor_state().expect("focused editor").text, "aa");
}

#[test]
fn moving_focus_during_preview_does_not_type_into_the_new_editor() {
    let first = FocusRequester::new();
    let second = FocusRequester::new();
    let content_first = first.clone();
    let content_second = second;
    let mut shell = new_shell(move || {
        let first = content_first.clone();
        let second = content_second.clone();
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            let state = remember(|| TextFieldState::new("first")).with(|state| *state);
            let target = second.clone();
            BasicTextField(
                state,
                Modifier::empty()
                    .width(200.0)
                    .height(40.0)
                    .focus_requester(&first)
                    .on_preview_key_event(move |_| {
                        target.request_focus().expect("move focus from callback");
                        false
                    }),
                Default::default(),
            );
            editor(Modifier::empty().focus_requester(&second), "second");
        });
    });
    first.request_focus().expect("focus first editor");
    shell.on_key_event(&KeyEvent::key_down(KeyCode::A, "a"));
    assert_eq!(
        shell.ime_editor_state().expect("newly focused editor").text,
        "second"
    );
    first.request_focus().expect("return to first editor");
    assert_eq!(
        shell.ime_editor_state().expect("first editor").text,
        "first"
    );
}

#[test]
fn focused_control_activation_runs_between_preview_and_bubble() {
    let (mut shell, focus, log) = logged_shell(move |focus, log| {
        Column(
            Modifier::empty()
                .on_preview_key_event(record(&log, "preview", false))
                .on_key_event(record(&log, "bubble", true)),
            ColumnSpec::default(),
            move || {
                let log = log.clone();
                cranpose_ui::Button(
                    Modifier::empty()
                        .width(100.0)
                        .height(40.0)
                        .focus_requester(&focus),
                    cranpose_ui::ButtonSpec::default(),
                    move || log.borrow_mut().push("activate"),
                    || {},
                );
            },
        );
    });
    focus.request_focus().expect("focus button");
    assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::Space, " ")));
    assert_eq!(
        *log.borrow(),
        ["preview", "activate"],
        "button consumes activation before bubbling"
    );
    log.borrow_mut().clear();
    assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::A, "a")));
    assert_eq!(*log.borrow(), ["preview", "bubble"]);
}

#[test]
fn preview_can_remove_its_focused_editor() {
    let focus = FocusRequester::new();
    let content_focus = focus.clone();
    let mut shell = new_shell(move || {
        let shown = rememberMutableStateOf(|| true);
        let focus = content_focus.clone();
        if shown.get() {
            Column(
                Modifier::empty().on_preview_key_event(move |_| {
                    shown.set(false);
                    true
                }),
                ColumnSpec::default(),
                move || {
                    editor(Modifier::empty().focus_requester(&focus), "kept");
                },
            );
        }
    });
    focus.request_focus().expect("focus editor");
    assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::Q, "q")));
    shell.update();
    assert!(shell.ime_editor_state().is_none());
    assert!(!shell.on_key_event(&KeyEvent::key_down(KeyCode::A, "a")));
}

#[test]
fn modal_keys_do_not_reach_the_surrounding_page() {
    let (mut shell, focus, log) = logged_shell(move |focus, log| {
        Column(
            Modifier::empty().on_preview_key_event(record(&log, "page", true)),
            ColumnSpec::default(),
            move || {
                let focus = focus.clone();
                let log = log.clone();
                cranpose_ui::widgets::Dialog(
                    cranpose_ui::widgets::DialogSpec::default(),
                    |_| {},
                    move || {
                        let state = remember(|| TextFieldState::new("")).with(|state| *state);
                        BasicTextField(
                            state,
                            Modifier::empty()
                                .width(200.0)
                                .height(40.0)
                                .focus_requester(&focus)
                                .on_preview_key_event(record(&log, "modal", false)),
                            Default::default(),
                        );
                    },
                );
            },
        );
    });
    focus.request_focus().expect("focus modal editor");
    assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::A, "a")));
    assert_eq!(*log.borrow(), ["modal"]);
    assert_eq!(shell.ime_editor_state().expect("modal editor").text, "a");
}

struct TestWindow;

#[test]
fn focused_editor_applies_clipboard_and_ime_edits() {
    let focus = FocusRequester::new();
    let content_focus = focus.clone();
    let mut shell = new_shell(move || {
        editor(Modifier::empty().focus_requester(&content_focus), "");
    });
    focus.request_focus().expect("focus editor");
    assert!(shell.on_paste("hello"));
    assert_eq!(shell.ime_editor_state().expect("editor").text, "hello");
    assert!(shell.on_ime_set_selection(1, 4));
    assert_eq!(shell.on_copy().as_deref(), Some("ell"));
    assert_eq!(shell.on_cut().as_deref(), Some("ell"));
    assert_eq!(shell.ime_editor_state().expect("editor").text, "ho");
    assert!(shell.on_ime_preedit("abc", Some((1, 2))));
    let state = shell.ime_editor_state().expect("composing editor");
    assert_eq!(state.text, "habco");
    assert_eq!(state.composition, Some((1, 4)));
    assert!(shell.on_ime_finish_composing());
    assert!(
        shell
            .ime_editor_state()
            .expect("editor")
            .composition
            .is_none()
    );
    assert!(shell.on_ime_set_composing_region(1, 4));
    assert_eq!(
        shell.ime_editor_state().expect("editor").composition,
        Some((1, 4))
    );
    assert!(shell.on_ime_set_selection(2, 2));
    assert!(shell.on_ime_finish_composing());
    shell.update();
    assert!(shell.on_ime_delete_surrounding(1, 1));
    assert_eq!(shell.ime_editor_state().expect("editor").text, "hco");
    assert!(shell.needs_redraw());
    shell.clear_text_field_focus();
    assert!(shell.ime_editor_state().is_none());
}

#[test]
fn empty_secondary_surface_cannot_receive_primary_input() {
    let focus = FocusRequester::new();
    let content_focus = focus.clone();
    let mut shell = new_shell(move || {
        editor(Modifier::empty().focus_requester(&content_focus), "primary");
    });
    shell.add_window_surface(
        u64::MAX,
        TestRenderer::default(),
        (320, 200),
        (320.0, 200.0),
    );
    focus.request_focus().expect("focus primary editor");
    let mut empty = shell
        .surface(cranpose_app_shell::RootId::Window(u64::MAX))
        .expect("empty surface");
    assert!(!empty.on_key_event(&KeyEvent::key_down(KeyCode::A, "a")));
    assert!(!empty.on_paste("wrong"));
    assert!(empty.ime_editor_state().is_none());
    assert_eq!(
        shell.ime_editor_state().expect("primary editor").text,
        "primary"
    );
}

impl cranpose_ui::WindowRootDescriptor for TestWindow {
    fn layout_size(&self) -> cranpose_ui::Size {
        cranpose_ui::Size::new(320.0, 200.0)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn window_editor() -> (AppShell<TestRenderer>, u64, EventLog) {
    let (mut shell, focus, log) = logged_shell(move |focus, log| {
        Column(
            Modifier::empty().on_preview_key_event(record(&log, "primary", true)),
            ColumnSpec::default(),
            move || {
                let focus = focus.clone();
                Column(
                    Modifier::empty()
                        .window_root(Rc::new(TestWindow))
                        .on_preview_key_event(record(&log, "window", false)),
                    ColumnSpec::default(),
                    move || {
                        editor(Modifier::empty().focus_requester(&focus), "");
                    },
                );
            },
        );
    });
    let window = shell.window_roots().first().expect("composed window").node as u64;
    shell.add_window_surface(window, TestRenderer::default(), (320, 200), (320.0, 200.0));
    shell.update();
    focus.request_focus().expect("focus secondary editor");
    (shell, window, log)
}

#[test]
fn window_keys_stay_on_the_receiving_surface() {
    let (mut shell, window, log) = window_editor();
    assert!(
        !shell
            .primary()
            .on_key_event(&KeyEvent::key_down(KeyCode::Q, "q")),
        "primary must not edit a secondary window"
    );
    assert!(
        shell
            .surface(cranpose_app_shell::RootId::Window(window))
            .expect("secondary window")
            .on_key_event(&KeyEvent::key_down(KeyCode::A, "a"))
    );
    assert_eq!(*log.borrow(), ["window"]);
    assert_eq!(
        shell
            .surface(cranpose_app_shell::RootId::Window(window))
            .expect("secondary window")
            .ime_editor_state()
            .expect("secondary editor")
            .text,
        "a"
    );
}

#[test]
fn clipboard_and_ime_input_stay_on_the_receiving_surface() {
    let (mut shell, window, _) = window_editor();
    let secondary = cranpose_app_shell::RootId::Window(window);
    assert!(
        shell
            .surface(secondary)
            .expect("secondary window")
            .on_paste("hello")
    );
    assert!(
        shell
            .surface(secondary)
            .expect("secondary window")
            .on_ime_set_selection(0, 5)
    );
    assert!(shell.primary().ime_editor_state().is_none());
    assert!(shell.primary().ime_caret_geometry().is_none());
    assert!(shell.primary().on_copy().is_none());
    assert!(shell.primary().on_cut().is_none());
    assert!(!shell.primary().on_paste("wrong"));
    assert!(!shell.primary().on_ime_preedit("wrong", Some((0, 5))));
    assert!(!shell.primary().on_ime_set_composing_region(0, 1));
    assert!(!shell.primary().on_ime_set_selection(0, 0));
    assert!(!shell.primary().on_ime_delete_surrounding(1, 1));
    assert!(!shell.primary().on_ime_finish_composing());
    shell.primary().clear_text_field_focus();
    let state = shell
        .surface(secondary)
        .expect("secondary window")
        .ime_editor_state()
        .expect("secondary editor remains focused");
    assert_eq!(state.text, "hello");
    assert_eq!(
        shell
            .surface(secondary)
            .expect("secondary window")
            .on_copy()
            .as_deref(),
        Some("hello")
    );
    assert!(
        shell
            .surface(secondary)
            .expect("secondary window")
            .on_ime_preedit("ok", Some((2, 2)))
    );
    assert!(
        shell
            .surface(secondary)
            .expect("secondary window")
            .on_ime_finish_composing()
    );
    assert_eq!(
        shell
            .surface(secondary)
            .expect("secondary window")
            .ime_editor_state()
            .expect("secondary editor")
            .text,
        "ok"
    );
}
