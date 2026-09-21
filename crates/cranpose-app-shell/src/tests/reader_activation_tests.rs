use super::*;

fn overlapping_shell() -> (AppShell<HitGraphRenderer>, Rc<Cell<i32>>) {
    let count = Rc::new(Cell::new(0));
    let recorded = Rc::clone(&count);
    let shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let recorded = Rc::clone(&recorded);
            Box(Modifier::empty(), BoxSpec::default(), move || {
                for (name, increment) in [("Rear", 1), ("Front", 10)] {
                    let recorded = Rc::clone(&recorded);
                    Box(
                        Modifier::empty()
                            .size(Size::new(100.0, 48.0))
                            .content_description(name)
                            .clickable(move |_| recorded.set(recorded.get() + increment)),
                        BoxSpec::default(),
                        || {},
                    );
                }
            });
        },
    );
    (shell, count)
}

#[test]
fn reader_activation_targets_identity_and_preserves_pointer_position() {
    let _guard = test_guard();
    let (mut shell, count) = overlapping_shell();
    shell.update();
    let rear = reader_control_id(&mut shell, "Rear");
    let front = reader_control_id(&mut shell, "Front");
    shell.set_cursor(350.0, 250.0);
    assert!(shell.accessibility_activate(rear, None));
    assert_eq!(count.get(), 1);
    assert!(shell.primary().accessibility_activate(front, None));
    assert_eq!(count.get(), 11);
    assert_eq!(shell.surfaces[0].cursor, (350.0, 250.0));
    assert!(!shell.accessibility_activate(usize::MAX, None));
    assert!(!shell.accessibility_activate(rear, Some(999)));
    assert_eq!(count.get(), 11);
}

#[test]
fn keyboard_activation_targets_focused_control_without_a_screen_reader() {
    let _guard = test_guard();
    let (mut shell, count) = overlapping_shell();
    shell.update();
    assert!(!shell.semantics_active());
    assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::Tab, "\t")));
    assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::Enter, "\r")));
    assert_eq!(count.get(), 1);
    assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::Tab, "\t")));
    assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::Space, " ")));
    assert_eq!(count.get(), 11);
    assert!(!shell.semantics_active());
}

#[test]
fn reader_activation_revalidates_disabled_hidden_and_removed_controls() {
    let _guard = test_guard();
    let count = Rc::new(Cell::new(0));
    let recorded = Rc::clone(&count);
    let mode = Rc::new(RefCell::new(None));
    let captured = Rc::clone(&mode);
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let state = rememberMutableStateOf(|| 0);
            *captured.borrow_mut() = Some(state);
            if state.get() == 3 {
                return;
            }
            let recorded = Rc::clone(&recorded);
            Box(
                Modifier::empty()
                    .size(Size::new(100.0, 48.0))
                    .content_description("Action")
                    .clickable(move |_| recorded.set(recorded.get() + 1))
                    .semantics(move |config| {
                        config.enabled = state.get() != 1;
                        config.hidden = state.get() == 2;
                    }),
                BoxSpec::default(),
                || {},
            );
        },
    );
    shell.update();
    let node_id = reader_control_id(&mut shell, "Action");
    assert!(shell.accessibility_activate(node_id, None));
    for value in 1..=3 {
        mode.borrow().expect("state").set(value);
        shell.update();
        assert!(
            !shell.accessibility_activate(node_id, None),
            "state {value}"
        );
    }
    assert_eq!(count.get(), 1);
}

#[test]
fn reader_activation_resolves_canvas_key_at_its_current_position() {
    let _guard = test_guard();
    let positions = Rc::new(RefCell::new(Vec::new()));
    let recorded = Rc::clone(&positions);
    let state = Rc::new(RefCell::new(None));
    let captured = Rc::clone(&state);
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let position = rememberMutableStateOf(|| 10.0f32);
            *captured.borrow_mut() = Some(position);
            let x = position.get();
            let recorded = Rc::clone(&recorded);
            Box(
                Modifier::empty()
                    .size(Size::new(200.0, 100.0))
                    .content_description("Drawing")
                    .clickable(move |point| recorded.borrow_mut().push(point))
                    .semantics(move |config| {
                        let bounds = Rect {
                            x,
                            y: 5.0,
                            width: 20.0,
                            height: 30.0,
                        };
                        let mut child = cranpose_foundation::CanvasSemanticsNode::control(
                            7,
                            bounds,
                            "Drawn button",
                        );
                        child.enabled = x < 100.0;
                        config.canvas_children = vec![child];
                    }),
                BoxSpec::default(),
                || {},
            );
        },
    );
    shell.update();
    let node_id = reader_control_id(&mut shell, "Drawing");
    assert!(shell.accessibility_activate(node_id, Some(7)));
    state.borrow().expect("position").set(50.0);
    shell.update();
    assert!(shell.accessibility_activate(node_id, Some(7)));
    assert_eq!(
        *positions.borrow(),
        vec![Point::new(20.0, 20.0), Point::new(60.0, 20.0)]
    );
    state.borrow().expect("position").set(110.0);
    shell.update();
    assert!(!shell.accessibility_activate(node_id, Some(7)));
    assert_eq!(positions.borrow().len(), 2);
}

#[test]
fn reader_activation_rejects_a_background_identity_after_a_modal_opens() {
    let _guard = test_guard();
    let count = Rc::new(Cell::new(0));
    let recorded = Rc::clone(&count);
    let state = Rc::new(RefCell::new(None));
    let captured = Rc::clone(&state);
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let open = rememberMutableStateOf(|| false);
            *captured.borrow_mut() = Some(open);
            let recorded = Rc::clone(&recorded);
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                let recorded = Rc::clone(&recorded);
                Text(
                    "Background",
                    Modifier::empty()
                        .size(Size::new(120.0, 48.0))
                        .clickable(move |_| recorded.set(recorded.get() + 1)),
                    TextStyle::default(),
                );
                if open.get() {
                    Box(
                        Modifier::empty()
                            .size(Size::new(120.0, 48.0))
                            .semantics(|config| config.is_modal = true),
                        BoxSpec::default(),
                        || {},
                    );
                }
            });
        },
    );
    shell.update();
    let node_id = reader_control_id(&mut shell, "Background");
    state.borrow().expect("modal").set(true);
    shell.update();
    assert!(!shell.accessibility_activate(node_id, None));
    assert_eq!(count.get(), 0);
    state.borrow().expect("modal").set(false);
    shell.update();
    assert!(shell.accessibility_activate(node_id, None));
    assert_eq!(count.get(), 1);
}
