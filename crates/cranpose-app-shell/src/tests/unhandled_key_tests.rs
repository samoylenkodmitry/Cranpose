use super::*;

fn key_logging_shell(
    log: Rc<RefCell<Vec<String>>>,
    with_field: bool,
) -> AppShell<HitGraphRenderer> {
    AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let outer = Rc::clone(&log);
            cranpose_ui::UnhandledKeyEvents(move |event| {
                outer
                    .borrow_mut()
                    .push(format!("outer {:?}", event.key_code));
                true
            });
            let inner = Rc::clone(&log);
            cranpose_ui::UnhandledKeyEvents(move |event| {
                inner
                    .borrow_mut()
                    .push(format!("inner {:?}", event.key_code));
                event.key_code == KeyCode::X
            });
            if with_field {
                let field = cranpose_core::remember(|| {
                    cranpose_foundation::text::TextFieldState::new("Receipt")
                })
                .with(|field| *field);
                cranpose_ui::BasicTextField(
                    field,
                    Modifier::empty()
                        .size(Size::new(200.0, 48.0))
                        .content_description("Merchant"),
                    TextStyle::default(),
                );
            }
        },
    )
}

#[test]
fn a_key_no_control_takes_goes_to_the_handler_composed_last_first() {
    let _guard = test_guard();
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut shell = key_logging_shell(Rc::clone(&log), false);
    shell.update();

    assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::X, "x")));
    assert_eq!(*log.borrow(), vec!["inner X".to_string()]);

    log.borrow_mut().clear();
    assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::Z, "z")));
    assert_eq!(
        *log.borrow(),
        vec!["inner Z".to_string(), "outer Z".to_string()],
        "a key the newest handler lets pass reaches the one composed before it"
    );
}

#[test]
fn a_key_typed_into_a_text_field_never_reaches_the_handlers() {
    let _guard = test_guard();
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut shell = key_logging_shell(Rc::clone(&log), true);
    shell.set_semantics_enabled(true);
    shell.update();
    let placed =
        crate::placed_semantics::placed_semantics_from_shell(&mut shell).expect("placed field");
    let field = placed
        .flatten()
        .into_iter()
        .find(|node| node.editable_text && node.label.as_deref() == Some("Merchant"))
        .expect("named editable field");
    assert!(shell.accessibility_activate(field.node_id, None));
    shell.update();

    shell.on_key_event(&KeyEvent::key_down(KeyCode::X, "x"));
    assert!(
        log.borrow().is_empty(),
        "typing an x into a field must not press the play key: {:?}",
        log.borrow()
    );
}

#[test]
fn a_handler_that_left_composition_is_no_longer_asked() {
    let _guard = test_guard();
    let log = Rc::new(RefCell::new(Vec::new()));
    let shown = Rc::new(RefCell::new(None::<MutableState<bool>>));
    let content_log = Rc::clone(&log);
    let content_shown = Rc::clone(&shown);
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let visible = rememberMutableStateOf(|| true);
            *content_shown.borrow_mut() = Some(visible);
            if visible.get() {
                let log = Rc::clone(&content_log);
                cranpose_ui::UnhandledKeyEvents(move |event| {
                    log.borrow_mut().push(format!("{:?}", event.key_code));
                    true
                });
            }
        },
    );
    shell.update();
    assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::B, "b")));

    shown.borrow().expect("visibility state").set(false);
    shell.update();
    assert!(!shell.on_key_event(&KeyEvent::key_down(KeyCode::B, "b")));
    assert_eq!(*log.borrow(), vec!["B".to_string()]);
}
