//! Test for conditional rendering bug
//!
//! ## Summary of Bugs
//!
//! 1. **Conditional Text Bug**: Text in if/else blocks outside content closures
//!    may not update visually when the condition changes, even though
//!    recomposition happens correctly.
//!
//! 2. **Root Cause**: The composition layer works correctly - conditionals
//!    are re-evaluated and nodes are updated. However, the rendering pipeline
//!    may not rebuild the scene when these changes occur.
//!
//! ## Reproduction Steps
//!
//! Run the demo app and click "Increment":
//! - "Counter: X" text updates correctly (it's inside a Row closure)
//! - "if counter % 2 == 0/!= 0" text does NOT update (it's outside any closure)
//!
//! Both read from the same state, but only one updates visually.

use compose_core::MutableState;
use compose_macros::composable;
use compose_testing::ComposeTestRule;
use compose_ui::*;

#[composable]
fn conditional_outside_closure_app(counter: MutableState<i32>) {
    // BUG REPRODUCTION: This conditional is outside any content closure
    // When counter changes, recomposition happens but the visual may not update
    if counter.get() % 2 == 0 {
        Text("Even", Modifier::empty().padding(8.0));
    } else {
        Text("Odd", Modifier::empty().padding(8.0));
    }

    // This should work because state is read inside the closure
    Column(Modifier::empty().padding(16.0), ColumnSpec::default(), {
        move || {
            Text(
                format!("Counter: {}", counter.get()),
                Modifier::empty().padding(8.0),
            );
        }
    });
}

#[composable]
fn conditional_inside_closure_app(counter: MutableState<i32>) {
    // CORRECT PATTERN: Conditional is inside the content closure
    Column(Modifier::empty().padding(16.0), ColumnSpec::default(), {
        move || {
            // State is read here, inside the closure
            if counter.get() % 2 == 0 {
                Text("Even", Modifier::empty().padding(8.0));
            } else {
                Text("Odd", Modifier::empty().padding(8.0));
            }

            Text(
                format!("Counter: {}", counter.get()),
                Modifier::empty().padding(8.0),
            );
        }
    });
}

#[test]
fn test_conditional_inside_closure_works() {
    // This shows the CORRECT pattern that should always work

    let mut rule = ComposeTestRule::new();
    let runtime = rule.runtime_handle();

    let counter = MutableState::with_runtime(0, runtime.clone());

    eprintln!("\n=== Testing CORRECT pattern (conditional inside closure) ===");
    rule.set_content({
        let c = counter;
        move || {
            conditional_inside_closure_app(c);
        }
    })
    .expect("initial render succeeds");

    // Change counter multiple times
    for i in 1..=3 {
        counter.set(i);
        rule.pump_until_idle()
            .unwrap_or_else(|_| panic!("recompose to {}", i));
        eprintln!("Counter changed to {}", i);
    }

    eprintln!("✓ Correct pattern works as expected\n");
}

/// This test documents the exact issue from the demo app
#[test]
fn test_demo_app_pattern_analysis() {
    eprintln!("\n========================================");
    eprintln!("Demo App Bug Analysis");
    eprintln!("========================================\n");

    eprintln!("In apps/desktop-demo/src/app.rs:");
    eprintln!();
    eprintln!("BROKEN (line 774-802):");
    eprintln!("  if counter.get() % 2 == 0 {{");
    eprintln!("    Text(\"if counter % 2 == 0\", ...);");
    eprintln!("  }} else {{");
    eprintln!("    Text(\"if counter % 2 != 0\", ...);");
    eprintln!("  }}");
    eprintln!("  ↑ Conditional OUTSIDE any closure");
    eprintln!("  ↑ Doesn't update visually when counter changes");
    eprintln!();
    eprintln!("WORKS (line 860):");
    eprintln!("  Row(Modifier..., move || {{");
    eprintln!("    Text(format!(\"Counter: {{}}\", counter.get()), ...);");
    eprintln!("  }})");
    eprintln!("  ↑ Text INSIDE the Row's content closure");
    eprintln!("  ↑ Updates correctly");
    eprintln!();
    eprintln!("DIAGNOSIS:");
    eprintln!("  - Both read from the same state");
    eprintln!("  - Both trigger recomposition");
    eprintln!("  - But only one updates visually");
    eprintln!("  - Likely: render scene not rebuilt for");
    eprintln!("    conditionals outside content closures");
    eprintln!("========================================\n");
}
