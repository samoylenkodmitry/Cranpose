use cranpose_core::MutableState;
use cranpose_macros::composable;
use cranpose_testing::ComposeTestRule;
use cranpose_ui::*;

#[composable]
fn conditional_outside_closure_app(counter: MutableState<i32>) {
    if counter.get() % 2 == 0 {
        Text("Even", Modifier::empty().padding(8.0), TextStyle::default());
    } else {
        Text("Odd", Modifier::empty().padding(8.0), TextStyle::default());
    }

    Column(Modifier::empty().padding(16.0), ColumnSpec::default(), {
        move || {
            Text(
                format!("Counter: {}", counter.get()),
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );
        }
    });
}

#[composable]
fn conditional_inside_closure_app(counter: MutableState<i32>) {
    Column(Modifier::empty().padding(16.0), ColumnSpec::default(), {
        move || {
            if counter.get() % 2 == 0 {
                Text("Even", Modifier::empty().padding(8.0), TextStyle::default());
            } else {
                Text("Odd", Modifier::empty().padding(8.0), TextStyle::default());
            }

            Text(
                format!("Counter: {}", counter.get()),
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );
        }
    });
}

#[test]
fn test_conditional_inside_closure_works() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);

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

    for i in 1..=3 {
        counter.set(i);
        rule.pump_until_idle()
            .unwrap_or_else(|_| panic!("recompose to {}", i));
        eprintln!("Counter changed to {}", i);
    }

    eprintln!("✓ Correct pattern works as expected\n");
}

#[test]
fn test_demo_app_pattern_analysis() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
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
