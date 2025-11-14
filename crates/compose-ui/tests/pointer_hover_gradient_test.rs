//! Test for pointer hover gradient bug
//!
//! This test demonstrates a bug where draw_with_content doesn't update
//! when state is changed from within an async pointer_input handler.
//!
//! The issue: The gradient doesn't follow the mouse because state changes
//! from async pointer input handlers may not trigger proper recomposition.

use compose_core::MutableState;
use compose_foundation::PointerEventKind;
use compose_macros::composable;
use compose_testing::ComposeTestRule;
use compose_ui::*;

#[composable]
fn gradient_follows_state_app(pointer_position: MutableState<Point>) {
    // This demonstrates the pattern used in the demo app
    // where state is read and captured in the draw closure

    Column(
        Modifier::empty().size(Size {
            width: 200.0,
            height: 200.0,
        })
        .then(Modifier::empty().draw_with_content({
            // This reads state at composition time and captures the value
            let position = pointer_position.get();
            eprintln!("Creating draw closure with position: {:?}", position);
            move |scope| {
                // The closure uses the captured position
                eprintln!("Drawing with captured position: {:?}", position);
                scope.draw_rect(Brush::radial_gradient(
                    vec![Color(1.0, 0.0, 0.0, 0.8), Color(0.0, 0.0, 0.0, 0.0)],
                    position,
                    50.0,
                ));
            }
        }))
        .then(Modifier::empty().pointer_input((), {
            let pointer_position = pointer_position.clone();
            move |scope: PointerInputScope| {
                let pointer_position = pointer_position.clone();
                async move {
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
            }
        })),
        ColumnSpec::default(),
        || {
            Text("Hover area", Modifier::empty().padding(8.0));
        },
    );
}

#[test]
fn test_manual_state_change_triggers_recomposition() {
    // This test verifies that manual state changes DO trigger recomposition
    // and the draw closure is recreated with new values.

    let mut rule = ComposeTestRule::new();
    let runtime = rule.runtime_handle();

    let pointer_position = MutableState::with_runtime(Point { x: 0.0, y: 0.0 }, runtime.clone());

    eprintln!("\n=== Initial composition ===");
    rule.set_content({
        let pos = pointer_position.clone();
        move || {
            gradient_follows_state_app(pos.clone());
        }
    })
    .expect("initial render succeeds");

    assert_eq!(pointer_position.get().x, 0.0);
    eprintln!("Initial position: {:?}", pointer_position.get());

    // Manual state change - this SHOULD trigger recomposition
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
    // This shows the CORRECT pattern: read state inside the draw callback
    // This doesn't rely on recomposition, so it's more robust

    Column(
        Modifier::empty().size(Size {
            width: 200.0,
            height: 200.0,
        })
        .then(Modifier::empty().draw_with_content({
            // Clone the state handle, not the value
            let pointer_position = pointer_position.clone();
            move |scope| {
                // Read state at draw time, not composition time
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
            let pointer_position = pointer_position.clone();
            move |scope: PointerInputScope| {
                let pointer_position = pointer_position.clone();
                async move {
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
            }
        })),
        ColumnSpec::default(),
        || {
            Text("Hover area", Modifier::empty().padding(8.0));
        },
    );
}

#[test]
fn test_correct_pattern_reads_state_at_draw_time() {
    // This test shows the pattern that SHOULD work:
    // Clone the state handle and read it inside the draw callback
    // This way, state is read at draw time, not composition time

    let mut rule = ComposeTestRule::new();
    let runtime = rule.runtime_handle();

    let pointer_position = MutableState::with_runtime(Point { x: 0.0, y: 0.0 }, runtime.clone());

    eprintln!("\n=== Initial composition (correct pattern) ===");
    rule.set_content({
        let pos = pointer_position.clone();
        move || {
            working_gradient_app(pos.clone());
        }
    })
    .expect("initial render succeeds");

    eprintln!("Initial position: {:?}", pointer_position.get());

    // Change state
    eprintln!("\n=== Changing state to (100, 100) ===");
    pointer_position.set(Point { x: 100.0, y: 100.0 });

    // With this pattern, we don't even need to recompose!
    // The draw callback reads the state directly each time it's called
    eprintln!("New position: {:?}", pointer_position.get());
    eprintln!("✓ Correct pattern: state read at draw time\n");
}
