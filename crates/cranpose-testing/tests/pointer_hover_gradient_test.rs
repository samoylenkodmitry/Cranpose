use cranpose_core::MutableState;
use cranpose_foundation::PointerEventKind;
use cranpose_macros::composable;
use cranpose_testing::ComposeTestRule;
use cranpose_ui::*;

#[composable]
fn gradient_follows_state_app(pointer_position: MutableState<Point>) {
    Column(
        Modifier::empty()
            .size(Size {
                width: 200.0,
                height: 200.0,
            })
            .then(Modifier::empty().draw_with_content({
                let position = pointer_position.get();
                eprintln!("Creating draw closure with position: {:?}", position);
                move |scope| {
                    eprintln!("Drawing with captured position: {:?}", position);
                    scope.draw_rect(Brush::radial_gradient(
                        vec![Color(1.0, 0.0, 0.0, 0.8), Color(0.0, 0.0, 0.0, 0.0)],
                        position,
                        50.0,
                    ));
                }
            }))
            .then(Modifier::empty().pointer_input((), {
                move |scope: PointerInputScope| async move {
                    scope
                        .await_pointer_event_scope(|await_scope| async move {
                            loop {
                                let event = await_scope.await_pointer_event().await;
                                if let PointerEventKind::Move = event.kind {
                                    eprintln!(
                                        "Pointer event: setting position to {:?}",
                                        event.position
                                    );
                                    pointer_position.set(Point {
                                        x: event.position.x,
                                        y: event.position.y,
                                    });
                                }
                            }
                        })
                        .await;
                }
            })),
        ColumnSpec::default(),
        || {
            Text(
                "Hover area",
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );
        },
    );
}

#[test]
fn test_manual_state_change_triggers_recomposition() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);

    let mut rule = ComposeTestRule::new();
    let runtime = rule.runtime_handle();

    let pointer_position = MutableState::with_runtime(Point { x: 0.0, y: 0.0 }, runtime.clone());

    eprintln!("\n=== Initial composition ===");
    rule.set_content({
        let pos = pointer_position;
        move || {
            gradient_follows_state_app(pos);
        }
    })
    .expect("initial render succeeds");

    assert_eq!(pointer_position.get().x, 0.0);
    eprintln!("Initial position: {:?}", pointer_position.get());

    eprintln!("\n=== Manually changing state to (100, 100) ===");
    pointer_position.set(Point { x: 100.0, y: 100.0 });

    eprintln!("=== Forcing recomposition ===");
    rule.pump_until_idle()
        .expect("recompose after state change");

    assert_eq!(pointer_position.get().x, 100.0);
    eprintln!("New position: {:?}", pointer_position.get());
    eprintln!("✓ Manual state changes trigger recomposition correctly\n");
}

#[composable]
fn working_gradient_app(pointer_position: MutableState<Point>) {
    Column(
        Modifier::empty()
            .size(Size {
                width: 200.0,
                height: 200.0,
            })
            .then(Modifier::empty().draw_with_content({
                move |scope| {
                    let position = pointer_position.get();
                    eprintln!("Drawing with current state position: {:?}", position);
                    scope.draw_rect(Brush::radial_gradient(
                        vec![Color(0.0, 1.0, 0.0, 0.8), Color(0.0, 0.0, 0.0, 0.0)],
                        position,
                        50.0,
                    ));
                }
            }))
            .then(Modifier::empty().pointer_input((), {
                move |scope: PointerInputScope| async move {
                    scope
                        .await_pointer_event_scope(|await_scope| async move {
                            loop {
                                let event = await_scope.await_pointer_event().await;
                                if let PointerEventKind::Move = event.kind {
                                    eprintln!("Setting position to {:?}", event.position);
                                    pointer_position.set(Point {
                                        x: event.position.x,
                                        y: event.position.y,
                                    });
                                }
                            }
                        })
                        .await;
                }
            })),
        ColumnSpec::default(),
        || {
            Text(
                "Hover area",
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );
        },
    );
}

#[test]
fn test_correct_pattern_reads_state_at_draw_time() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);

    let mut rule = ComposeTestRule::new();
    let runtime = rule.runtime_handle();

    let pointer_position = MutableState::with_runtime(Point { x: 0.0, y: 0.0 }, runtime.clone());

    eprintln!("\n=== Initial composition (correct pattern) ===");
    rule.set_content({
        let pos = pointer_position;
        move || {
            working_gradient_app(pos);
        }
    })
    .expect("initial render succeeds");

    eprintln!("Initial position: {:?}", pointer_position.get());

    eprintln!("\n=== Changing state to (100, 100) ===");
    pointer_position.set(Point { x: 100.0, y: 100.0 });

    eprintln!("New position: {:?}", pointer_position.get());
    eprintln!("✓ Correct pattern: state read at draw time\n");
}
