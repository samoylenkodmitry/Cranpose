//! Test for render invalidation bug
//!
//! This test checks if the render scene is properly invalidated and rebuilt
//! when composition changes. The bug is that even though nodes update correctly,
//! the visual scene doesn't get rebuilt, so the display is stale.

use cranpose_core::MutableState;
use cranpose_macros::composable;
use cranpose_testing::ComposeTestRule;
use cranpose_ui::*;

#[composable]
fn conditional_text_app(counter: MutableState<i32>) {
    if counter.get() % 2 == 0 {
        Text("Even", Modifier::empty().padding(8.0));
    } else {
        Text("Odd", Modifier::empty().padding(8.0));
    }
}

#[test]
fn test_render_invalidation_on_conditional_change() {
    // This test FAILS because render invalidation doesn't happen

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

    // Clear any previous render invalidation
    cranpose_ui::take_render_invalidation();

    // Verify we're starting clean
    let before_change = cranpose_ui::peek_render_invalidation();
    eprintln!(
        "Before state change - render invalidated: {}",
        before_change
    );
    assert!(
        !before_change,
        "Should start with no pending render invalidation"
    );

    // Change state - this should trigger render invalidation
    eprintln!("\n=== Changing counter from 0 to 1 ===");
    counter.set(1);

    eprintln!("=== Running recomposition ===");
    rule.pump_until_idle().expect("recompose");

    // Check if render was invalidated after recomposition
    let after_change = cranpose_ui::peek_render_invalidation();
    eprintln!(
        "\nAfter recomposition - render invalidated: {}",
        after_change
    );

    // THIS ASSERTION SHOULD FAIL - exposing the bug
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
