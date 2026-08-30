use cranpose_core::MutableState;
use cranpose_foundation::PointerEventKind;
use cranpose_macros::composable;
use cranpose_testing::ComposeTestRule;
use cranpose_ui::*;

#[composable]
fn test_hover_app(position: MutableState<Point>, event_count: MutableState<i32>) {
    Column(
        Modifier::empty()
            .padding(20.0)
            .then(Modifier::empty().size(Size {
                width: 200.0,
                height: 200.0,
            }))
            .then(Modifier::empty().pointer_input((), {
                let pos = position;
                let count = event_count;
                move |scope: PointerInputScope| async move {
                    count.set(-1);

                    scope
                        .await_pointer_event_scope(|await_scope| async move {
                            loop {
                                let event = await_scope.await_pointer_event().await;
                                if event.kind == PointerEventKind::Move {
                                    pos.set(Point {
                                        x: event.position.x,
                                        y: event.position.y,
                                    });
                                    count.update(|c| {
                                        if *c == -1 {
                                            *c = 1;
                                        } else {
                                            *c += 1;
                                        }
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
fn test_pointer_input_async_handler_lifecycle() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let mut rule = ComposeTestRule::new();
    let runtime = rule.runtime_handle();

    let position = MutableState::with_runtime(Point { x: 0.0, y: 0.0 }, runtime.clone());
    let event_count = MutableState::with_runtime(0, runtime.clone());

    rule.set_content({
        let pos = position;
        let count = event_count;
        move || {
            test_hover_app(pos, count);
        }
    })
    .expect("initial render succeeds");

    rule.pump_until_idle().expect("pump after initial render");

    assert_eq!(
        event_count.get(),
        -1,
        "async pointer handler should be attached and waiting for input"
    );
}

#[composable]
fn pause_button_app(is_running: MutableState<bool>, click_count: MutableState<i32>) {
    let running = is_running.get();
    let button_color = if running {
        Color(0.5, 0.2, 0.35, 1.0)
    } else {
        Color(0.2, 0.45, 0.9, 1.0)
    };

    Column(
        Modifier::empty().padding(20.0),
        ColumnSpec::default(),
        move || {
            Text(
                format!(
                    "Running: {}, Clicks: {}",
                    is_running.get(),
                    click_count.get()
                ),
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );

            Button(
                Modifier::empty()
                    .rounded_corners(16.0)
                    .then(Modifier::empty().draw_behind({
                        let color = button_color;
                        move |scope| {
                            scope.draw_round_rect(Brush::solid(color), CornerRadii::uniform(16.0));
                        }
                    })),
                ButtonSpec::default(),
                {
                    move || {
                        is_running.set(!is_running.get());
                        click_count.set(click_count.get() + 1);
                    }
                },
                {
                    let label = if running { "Pause" } else { "Resume" };
                    move || {
                        Text(label, Modifier::empty().padding(6.0), TextStyle::default());
                    }
                },
            );
        },
    );
}

#[test]
fn test_pause_button_with_dynamic_content() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let mut rule = ComposeTestRule::new();
    let runtime = rule.runtime_handle();

    let is_running = MutableState::with_runtime(true, runtime.clone());
    let click_count = MutableState::with_runtime(0, runtime.clone());

    rule.set_content({
        let running = is_running;
        let count = click_count;
        move || {
            pause_button_app(running, count);
        }
    })
    .expect("initial render succeeds");

    assert!(is_running.get());
    assert_eq!(click_count.get(), 0);

    is_running.set(false);
    click_count.set(1);

    rule.pump_until_idle()
        .expect("recompose after state change");

    assert!(!is_running.get());
    assert_eq!(click_count.get(), 1);

    let node_count_after_first_toggle = rule.applier_mut().len();

    is_running.set(true);
    click_count.set(2);

    rule.pump_until_idle()
        .expect("recompose after second toggle");

    assert!(is_running.get());
    assert_eq!(click_count.get(), 2);

    let node_count_after_second_toggle = rule.applier_mut().len();

    assert_eq!(
        node_count_after_first_toggle, node_count_after_second_toggle,
        "Node count should not change when toggling button state"
    );

    println!("✓ Pause button maintains stable structure through state changes");
}
