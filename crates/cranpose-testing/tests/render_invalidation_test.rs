use cranpose_core::MutableState;
use cranpose_macros::composable;
use cranpose_testing::ComposeTestRule;
use cranpose_ui::*;

#[composable]
fn conditional_text_app(counter: MutableState<i32>) {
    if counter.get() % 2 == 0 {
        Text("Even", Modifier::empty().padding(8.0), TextStyle::default());
    } else {
        Text("Odd", Modifier::empty().padding(8.0), TextStyle::default());
    }
}

#[test]
fn test_render_invalidation_on_conditional_change() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);

    let mut rule = ComposeTestRule::new();
    let runtime = rule.runtime_handle();

    let counter = MutableState::with_runtime(0, runtime.clone());

    eprintln!("\n╔═══════════════════════════════════════════════════════════════╗");
    eprintln!("║           RENDER INVALIDATION BUG TEST                        ║");
    eprintln!("╚═══════════════════════════════════════════════════════════════╝\n");

    eprintln!("=== Initial composition ===");
    rule.set_content({
        let c = counter;
        move || {
            conditional_text_app(c);
        }
    })
    .expect("initial render succeeds");

    rule.with_app_context(cranpose_ui::take_render_invalidation);

    let before_change = rule.with_app_context(cranpose_ui::peek_render_invalidation);
    eprintln!(
        "Before state change - render invalidated: {}",
        before_change
    );
    assert!(
        !before_change,
        "Should start with no pending render invalidation"
    );

    eprintln!("\n=== Changing counter from 0 to 1 ===");
    counter.set(1);

    eprintln!("=== Running recomposition ===");
    rule.pump_until_idle().expect("recompose");

    let after_change = rule.with_app_context(cranpose_ui::peek_render_invalidation);
    eprintln!(
        "\nAfter recomposition - render invalidated: {}",
        after_change
    );

    assert!(
        after_change,
        "\n\n\
        ╔═══════════════════════════════════════════════════════════════╗\n\
        ║                    ❌ BUG DETECTED! ❌                         ║\n\
        ╠═══════════════════════════════════════════════════════════════╣\n\
        ║ Render was NOT invalidated after composition change!         ║\n\
        ║                                                               ║\n\
        ║ What happened:                                                ║\n\
        ║  1. ✓ State changed (counter: 0 → 1)                        ║\n\
        ║  2. ✓ Recomposition ran successfully                         ║\n\
        ║  3. ✓ Node content updated (verified in other test)          ║\n\
        ║  4. ✗ request_render_invalidation() was NEVER called!        ║\n\
        ║                                                               ║\n\
        ║ Result:                                                       ║\n\
        ║  The visual display stays stale even though the underlying   ║\n\
        ║  data is correct.                                            ║\n\
        ║                                                               ║\n\
        ║ This explains the demo app bug:                              ║\n\
        ║  - 'Counter: X' updates (inside Row closure)                 ║\n\
        ║  - 'if counter % 2' text does NOT update (outside closure)   ║\n\
        ║                                                               ║\n\
        ║ Both read the same state, both cause recomposition, but      ║\n\
        ║ only the one inside a content closure triggers a redraw.     ║\n\
        ╚═══════════════════════════════════════════════════════════════╝\n"
    );

    eprintln!("\n╔═══════════════════════════════════════════════════════════════╗");
    eprintln!("║                    ✓ BUG IS FIXED! ✓                         ║");
    eprintln!("║  Render invalidation now works correctly after recomposition ║");
    eprintln!("╚═══════════════════════════════════════════════════════════════╝\n");
}
