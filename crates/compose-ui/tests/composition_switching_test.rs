//! Tests for composition switching bug
//!
//! This test reproduces a bug where switching between different composable views
//! and clicking buttons causes content to be appended multiple times.

use compose_core::{compositionLocalOf, CompositionLocalProvider, MutableState};
use compose_macros::composable;
use compose_testing::ComposeTestRule;
use compose_ui::*;

/// Helper to create a simple composable that tracks how many times it's rendered
#[composable]
fn counter_view(counter: MutableState<i32>, render_count: MutableState<i32>) {
    // Track render count
    render_count.set(render_count.get() + 1);

    Column(
        Modifier::empty().padding(20.0),
        ColumnSpec::default(),
        move || {
            Text(
                format!("Counter: {}", counter.get()),
                Modifier::empty().padding(8.0),
            );

            Button(
                Modifier::empty().padding(10.0),
                {
                    move || {
                        counter.set(counter.get() + 1);
                    }
                },
                || {
                    Text("Increment", Modifier::empty().padding(4.0));
                },
            );
        },
    );
}

/// Helper for an alternative view
#[composable]
fn alternative_view(counter: MutableState<i32>, render_count: MutableState<i32>) {
    // Track render count
    render_count.set(render_count.get() + 1);

    Column(
        Modifier::empty().padding(20.0),
        ColumnSpec::default(),
        move || {
            Text(
                format!("Alternative: {}", counter.get()),
                Modifier::empty().padding(8.0),
            );

            Button(
                Modifier::empty().padding(10.0),
                {
                    move || {
                        counter.set(counter.get() + 1);
                    }
                },
                || {
                    Text("Add", Modifier::empty().padding(4.0));
                },
            );
        },
    );
}

/// Combined app that switches between views
#[composable]
fn combined_switching_app(
    show_counter: MutableState<bool>,
    counter1: MutableState<i32>,
    counter2: MutableState<i32>,
    render_count1: MutableState<i32>,
    render_count2: MutableState<i32>,
) {
    Column(
        Modifier::empty().padding(20.0),
        ColumnSpec::default(),
        move || {
            let show_counter_inner = show_counter;
            let show_counter_for_button1 = show_counter;
            let show_counter_for_button2 = show_counter;
            let counter1_inner = counter1;
            let counter2_inner = counter2;
            let render_count1_inner = render_count1;
            let render_count2_inner = render_count2;

            // Switch buttons
            Row(
                Modifier::empty().padding(8.0),
                RowSpec::default(),
                move || {
                    Button(
                        Modifier::empty().padding(10.0),
                        {
                            let show_counter = show_counter_for_button1;
                            move || {
                                show_counter.set(true);
                            }
                        },
                        || {
                            Text("Counter View", Modifier::empty().padding(4.0));
                        },
                    );

                    Spacer(Size {
                        width: 8.0,
                        height: 0.0,
                    });

                    Button(
                        Modifier::empty().padding(10.0),
                        {
                            let show_counter = show_counter_for_button2;
                            move || {
                                show_counter.set(false);
                            }
                        },
                        || {
                            Text("Alternative View", Modifier::empty().padding(4.0));
                        },
                    );
                },
            );

            Spacer(Size {
                width: 0.0,
                height: 12.0,
            });

            // Conditionally show one view or the other
            if show_counter_inner.get() {
                counter_view(counter1_inner, render_count1_inner);
            } else {
                alternative_view(counter2_inner, render_count2_inner);
            }
        },
    );
}

#[test]
fn test_switching_between_views_doesnt_duplicate_content() {
    let mut rule = ComposeTestRule::new();
    let runtime = rule.runtime_handle();

    let show_counter = MutableState::with_runtime(true, runtime.clone());
    let counter1 = MutableState::with_runtime(0, runtime.clone());
    let counter2 = MutableState::with_runtime(0, runtime.clone());
    let render_count1 = MutableState::with_runtime(0, runtime.clone());
    let render_count2 = MutableState::with_runtime(0, runtime.clone());

    // Initial render - show counter view
    rule.set_content({
        move || {
            combined_switching_app(
                show_counter,
                counter1,
                counter2,
                render_count1,
                render_count2,
            );
        }
    })
    .expect("initial render succeeds");

    let initial_node_count = rule.applier_mut().len();
    println!("Initial node count: {}", initial_node_count);
    println!("Counter view render count: {}", render_count1.get());
    assert_eq!(
        render_count1.get(),
        1,
        "Counter view should render once initially"
    );
    assert_eq!(
        render_count2.get(),
        0,
        "Alternative view should not render initially"
    );

    // Switch to alternative view
    show_counter.set(false);
    rule.pump_until_idle().expect("recompose after switching");

    let after_switch_node_count = rule.applier_mut().len();
    println!("After switch node count: {}", after_switch_node_count);
    println!("Alternative view render count: {}", render_count2.get());
    assert_eq!(
        render_count1.get(),
        1,
        "Counter view should still have 1 render"
    );
    assert_eq!(
        render_count2.get(),
        1,
        "Alternative view should render once"
    );

    // Switch back to counter view
    show_counter.set(true);
    rule.pump_until_idle()
        .expect("recompose after switching back");

    let after_switch_back_node_count = rule.applier_mut().len();
    println!(
        "After switch back node count: {}",
        after_switch_back_node_count
    );
    println!(
        "Counter view render count after switch back: {}",
        render_count1.get()
    );

    // The bug: if content is being duplicated, node count will grow
    assert_eq!(
        after_switch_back_node_count, initial_node_count,
        "Node count should return to initial value after switching back"
    );
    assert_eq!(
        render_count1.get(),
        2,
        "Counter view should render twice total"
    );

    // Click increment button twice in counter view
    counter1.set(1);
    rule.pump_until_idle()
        .expect("recompose after first increment");
    let after_first_click = rule.applier_mut().len();
    println!("After first increment node count: {}", after_first_click);

    counter1.set(2);
    rule.pump_until_idle()
        .expect("recompose after second increment");
    let after_second_click = rule.applier_mut().len();
    println!("After second increment node count: {}", after_second_click);

    // Node count should remain stable
    assert_eq!(
        after_first_click, after_switch_back_node_count,
        "Node count should not change on first increment"
    );
    assert_eq!(
        after_second_click, after_switch_back_node_count,
        "Node count should not change on second increment"
    );

    // Switch to alternative view again
    show_counter.set(false);
    rule.pump_until_idle()
        .expect("recompose after switching to alternative");
    let after_second_switch = rule.applier_mut().len();
    println!("After second switch node count: {}", after_second_switch);

    // Click increment button twice in alternative view
    counter2.set(1);
    rule.pump_until_idle().expect("recompose after first add");
    let after_first_add = rule.applier_mut().len();
    println!("After first add node count: {}", after_first_add);

    counter2.set(2);
    rule.pump_until_idle().expect("recompose after second add");
    let after_second_add = rule.applier_mut().len();
    println!("After second add node count: {}", after_second_add);

    // Node count should remain stable
    assert_eq!(
        after_first_add, after_second_switch,
        "Node count should not change on first add"
    );
    assert_eq!(
        after_second_add, after_second_switch,
        "Node count should not change on second add"
    );

    // Final check: switch back and verify no duplication
    show_counter.set(true);
    rule.pump_until_idle().expect("final recompose");
    let final_node_count = rule.applier_mut().len();
    println!("Final node count: {}", final_node_count);

    assert_eq!(
        final_node_count, initial_node_count,
        "Final node count should match initial - no content duplication"
    );
}

#[test]
fn test_node_cleanup_on_view_switch() {
    let mut rule = ComposeTestRule::new();
    let runtime = rule.runtime_handle();

    let show_first = MutableState::with_runtime(true, runtime.clone());

    rule.set_content({
        move || {
            let show_first_inner = show_first;
            Column(
                Modifier::empty().padding(20.0),
                ColumnSpec::default(),
                move || {
                    if show_first_inner.get() {
                        // First view with 3 text nodes
                        Text("First A", Modifier::empty());
                        Text("First B", Modifier::empty());
                        Text("First C", Modifier::empty());
                    } else {
                        // Second view with 2 text nodes
                        Text("Second A", Modifier::empty());
                        Text("Second B", Modifier::empty());
                    }
                },
            );
        }
    })
    .expect("initial render succeeds");

    // Count nodes: 1 Column + 3 Text = 4
    let initial_count = rule.applier_mut().len();
    println!("Initial node count (first view): {}", initial_count);
    assert_eq!(initial_count, 4, "Should have Column + 3 Text nodes");

    // Switch to second view
    show_first.set(false);
    rule.pump_until_idle().expect("recompose after switch");

    // Count nodes: 1 Column + 2 Text = 3
    let after_switch = rule.applier_mut().len();
    println!("Node count after switch (second view): {}", after_switch);
    assert_eq!(after_switch, 3, "Should have Column + 2 Text nodes");

    // Switch back to first view
    show_first.set(true);
    rule.pump_until_idle().expect("recompose after switch back");

    let after_switch_back = rule.applier_mut().len();
    println!(
        "Node count after switch back (first view): {}",
        after_switch_back
    );
    assert_eq!(
        after_switch_back, initial_count,
        "Should return to initial node count"
    );

    // Multiple rapid switches
    for i in 0..5 {
        show_first.set(i % 2 == 0);
        rule.pump_until_idle()
            .unwrap_or_else(|_| panic!("recompose on switch {}", i));
        let count = rule.applier_mut().len();
        let expected = if i % 2 == 0 { 4 } else { 3 };
        assert_eq!(
            count, expected,
            "Node count should be correct after rapid switch {}",
            i
        );
    }
}

#[test]
fn test_multiple_switches_with_state_changes() {
    let mut rule = ComposeTestRule::new();
    let runtime = rule.runtime_handle();

    let show_view_a = MutableState::with_runtime(true, runtime.clone());
    let counter_a = MutableState::with_runtime(0, runtime.clone());
    let counter_b = MutableState::with_runtime(0, runtime.clone());

    rule.set_content({
        move || {
            let show_view_a_inner = show_view_a;
            let counter_a_inner = counter_a;
            let counter_b_inner = counter_b;
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                if show_view_a_inner.get() {
                    Text(
                        format!("View A: {}", counter_a_inner.get()),
                        Modifier::empty(),
                    );
                    Button(
                        Modifier::empty(),
                        {
                            let counter_a = counter_a_inner;
                            move || counter_a.set(counter_a.get() + 1)
                        },
                        || {
                            Text("Increment A", Modifier::empty());
                        },
                    );
                } else {
                    Text(
                        format!("View B: {}", counter_b_inner.get()),
                        Modifier::empty(),
                    );
                    Button(
                        Modifier::empty(),
                        {
                            let counter_b = counter_b_inner;
                            move || counter_b.set(counter_b.get() + 1)
                        },
                        || {
                            Text("Increment B", Modifier::empty());
                        },
                    );
                }
            });
        }
    })
    .expect("initial render succeeds");

    let baseline_count = rule.applier_mut().len();
    println!("Baseline node count: {}", baseline_count);

    // Reproduce the exact scenario from the bug report:
    // 1. Start in View A (Counter APP)
    // 2. Switch to View B (CompositionLocal Test)
    // 3. Click button in View B twice
    // 4. Switch back to View A
    // 5. Click button in View A twice
    // Expected: No node duplication at any point

    // Step 1: We're already in View A
    assert_eq!(counter_a.get(), 0);

    // Step 2: Switch to View B
    show_view_a.set(false);
    rule.pump_until_idle().expect("switch to view B");
    let after_switch_to_b = rule.applier_mut().len();
    println!("After switch to View B: {}", after_switch_to_b);
    assert_eq!(
        after_switch_to_b, baseline_count,
        "Node count should be same (both views have same structure)"
    );

    // Step 3: Click button twice in View B
    counter_b.set(counter_b.get() + 1);
    rule.pump_until_idle().expect("first click in view B");
    let after_first_click_b = rule.applier_mut().len();
    println!("After first click in View B: {}", after_first_click_b);
    assert_eq!(counter_b.get(), 1);

    counter_b.set(counter_b.get() + 1);
    rule.pump_until_idle().expect("second click in view B");
    let after_second_click_b = rule.applier_mut().len();
    println!("After second click in View B: {}", after_second_click_b);
    assert_eq!(counter_b.get(), 2);

    // This is where the bug might manifest - content appended twice
    assert_eq!(
        after_second_click_b, baseline_count,
        "BUG: Content should not be duplicated after two clicks in View B"
    );

    // Step 4: Switch back to View A
    show_view_a.set(true);
    rule.pump_until_idle().expect("switch back to view A");
    let after_switch_to_a = rule.applier_mut().len();
    println!("After switch back to View A: {}", after_switch_to_a);
    assert_eq!(
        after_switch_to_a, baseline_count,
        "Node count should return to baseline after switching back"
    );

    // Step 5: Click button twice in View A
    counter_a.set(counter_a.get() + 1);
    rule.pump_until_idle().expect("first click in view A");
    let after_first_click_a = rule.applier_mut().len();
    println!("After first click in View A: {}", after_first_click_a);
    assert_eq!(counter_a.get(), 1);

    counter_a.set(counter_a.get() + 1);
    rule.pump_until_idle().expect("second click in view A");
    let after_second_click_a = rule.applier_mut().len();
    println!("After second click in View A: {}", after_second_click_a);
    assert_eq!(counter_a.get(), 2);

    // Final verification: no duplication
    assert_eq!(
        after_second_click_a, baseline_count,
        "BUG: Content should not be duplicated after two clicks in View A"
    );
}

#[test]
fn test_deeply_nested_conditional_switching() {
    let mut rule = ComposeTestRule::new();
    let runtime = rule.runtime_handle();

    let show_outer = MutableState::with_runtime(true, runtime.clone());
    let show_inner = MutableState::with_runtime(true, runtime.clone());

    rule.set_content({
        move || {
            let show_outer_inner = show_outer;
            let show_inner_inner = show_inner;
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                if show_outer_inner.get() {
                    let show_inner_for_column = show_inner_inner;
                    Column(Modifier::empty(), ColumnSpec::default(), move || {
                        if show_inner_for_column.get() {
                            Text("Outer A, Inner A", Modifier::empty());
                            Text("Nested content A", Modifier::empty());
                        } else {
                            Text("Outer A, Inner B", Modifier::empty());
                        }
                    });
                } else {
                    Text("Outer B", Modifier::empty());
                }
            });
        }
    })
    .expect("initial render succeeds");

    // Initial: outer=true, inner=true -> 1 root + 1 nested column + 2 texts = 4
    let initial = rule.applier_mut().len();
    assert_eq!(initial, 4, "Should have nested structure");

    // Switch inner: outer=true, inner=false -> 1 root + 1 nested column + 1 text = 3
    show_inner.set(false);
    rule.pump_until_idle().expect("switch inner");
    let after_inner_switch = rule.applier_mut().len();
    assert_eq!(
        after_inner_switch, 3,
        "Should have fewer nodes after inner switch"
    );

    // Switch outer: outer=false -> 1 root + 1 text = 2
    show_outer.set(false);
    rule.pump_until_idle().expect("switch outer");
    let after_outer_switch = rule.applier_mut().len();
    assert_eq!(after_outer_switch, 2, "Should have minimal nodes");

    // Switch back to initial state
    show_outer.set(true);
    show_inner.set(true);
    rule.pump_until_idle().expect("restore initial");
    let restored = rule.applier_mut().len();
    assert_eq!(restored, initial, "Should restore original node count");
}

#[test]
fn test_switching_with_different_node_counts() {
    let mut rule = ComposeTestRule::new();
    let runtime = rule.runtime_handle();

    let view_type = MutableState::with_runtime(0, runtime.clone());

    rule.set_content({
        move || {
            let view_type_inner = view_type;
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                match view_type_inner.get() {
                    0 => {
                        // View with 1 node
                        Text("Single", Modifier::empty());
                    }
                    1 => {
                        // View with 3 nodes
                        Text("Triple 1", Modifier::empty());
                        Text("Triple 2", Modifier::empty());
                        Text("Triple 3", Modifier::empty());
                    }
                    2 => {
                        // View with 5 nodes including nested structure
                        Column(Modifier::empty(), ColumnSpec::default(), || {
                            Text("Nested 1", Modifier::empty());
                            Text("Nested 2", Modifier::empty());
                        });
                        Text("Extra", Modifier::empty());
                    }
                    _ => {}
                }
            });
        }
    })
    .expect("initial render succeeds");

    // View 0: 1 root + 1 text = 2
    let count0 = rule.applier_mut().len();
    assert_eq!(count0, 2);

    // Switch to View 1: 1 root + 3 texts = 4
    view_type.set(1);
    rule.pump_until_idle().expect("switch to view 1");
    let count1 = rule.applier_mut().len();
    assert_eq!(count1, 4);

    // Switch to View 2: 1 root + 1 nested column + 2 texts + 1 extra text = 5
    view_type.set(2);
    rule.pump_until_idle().expect("switch to view 2");
    let count2 = rule.applier_mut().len();
    assert_eq!(count2, 5);

    // Switch back to View 0
    view_type.set(0);
    rule.pump_until_idle().expect("switch back to view 0");
    let count0_again = rule.applier_mut().len();
    assert_eq!(count0_again, count0, "Should return to original count");

    // Cycle through all views multiple times
    for _ in 0..3 {
        view_type.set(1);
        rule.pump_until_idle().expect("cycle: view 1");
        assert_eq!(rule.applier_mut().len(), 4);

        view_type.set(2);
        rule.pump_until_idle().expect("cycle: view 2");
        assert_eq!(rule.applier_mut().len(), 5);

        view_type.set(0);
        rule.pump_until_idle().expect("cycle: view 0");
        assert_eq!(rule.applier_mut().len(), 2);
    }
}

#[test]
fn test_conditional_with_complex_button_structure() {
    let mut rule = ComposeTestRule::new();
    let runtime = rule.runtime_handle();

    let show_first = MutableState::with_runtime(true, runtime.clone());
    let counter = MutableState::with_runtime(0, runtime.clone());

    rule.set_content({
        move || {
            let show_first_inner = show_first;
            let counter_inner = counter;
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                if show_first_inner.get() {
                    // Complex structure with nested buttons
                    Column(Modifier::empty(), ColumnSpec::default(), {
                        let counter = counter_inner;
                        move || {
                            Text("First View", Modifier::empty());
                            Button(
                                Modifier::empty(),
                                move || counter.set(counter.get() + 1),
                                || {
                                    Text("Button 1", Modifier::empty());
                                },
                            );
                            Button(
                                Modifier::empty(),
                                move || counter.set(counter.get() + 10),
                                || {
                                    Text("Button 2", Modifier::empty());
                                },
                            );
                        }
                    });
                } else {
                    // Different structure
                    Text("Second View", Modifier::empty());
                    Button(
                        Modifier::empty(),
                        {
                            let counter = counter_inner;
                            move || counter.set(counter.get() - 1)
                        },
                        || {
                            Text("Decrement", Modifier::empty());
                        },
                    );
                }
            });
        }
    })
    .expect("initial render succeeds");

    // First view: 1 root + 1 nested column + 1 text + 2 buttons (each with 1 text child) = 7
    // (root Column, nested Column, Text, Button1, Button1's Text, Button2, Button2's Text)
    let initial = rule.applier_mut().len();
    println!("Initial node count: {}", initial);
    assert_eq!(initial, 7);

    // Interact with first view
    counter.set(5);
    rule.pump_until_idle().expect("update counter");
    assert_eq!(
        rule.applier_mut().len(),
        initial,
        "Node count stable after state change"
    );

    // Switch to second view: 1 root + 1 text + 1 button (with 1 text child) = 4
    show_first.set(false);
    rule.pump_until_idle().expect("switch to second");
    let after_switch = rule.applier_mut().len();
    assert_eq!(after_switch, 4);

    // Interact with second view
    counter.set(3);
    rule.pump_until_idle()
        .expect("update counter in second view");
    assert_eq!(
        rule.applier_mut().len(),
        after_switch,
        "Node count stable in second view"
    );

    // Switch back to first view
    show_first.set(true);
    rule.pump_until_idle().expect("switch back to first");
    let restored = rule.applier_mut().len();
    assert_eq!(
        restored, initial,
        "Should restore original complex structure"
    );
}

#[test]
fn test_clicking_same_switch_button_twice_no_duplication() {
    // This test reproduces the bug where clicking the "CompositionLocal Test" button
    // twice causes content duplication
    let mut rule = ComposeTestRule::new();
    let runtime = rule.runtime_handle();

    let show_counter = MutableState::with_runtime(true, runtime.clone());

    rule.set_content({
        move || {
            let show_counter_copy = show_counter;
            let show_counter_for_button = show_counter;
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                // Row with switching buttons
                Row(Modifier::empty(), RowSpec::default(), {
                    let show_counter = show_counter_for_button;
                    move || {
                        let show_counter_for_btn1 = show_counter;
                        let show_counter_for_btn2 = show_counter;

                        Button(
                            Modifier::empty(),
                            move || show_counter_for_btn1.set(true),
                            || {
                                Text("Counter App", Modifier::empty());
                            },
                        );

                        Button(
                            Modifier::empty(),
                            move || show_counter_for_btn2.set(false),
                            || {
                                Text("CompositionLocal Test", Modifier::empty());
                            },
                        );
                    }
                });

                // Conditional content
                if show_counter_copy.get() {
                    Column(Modifier::empty(), ColumnSpec::default(), || {
                        Text("Counter View", Modifier::empty());
                        Text("Line 2", Modifier::empty());
                    });
                } else {
                    Column(Modifier::empty(), ColumnSpec::default(), || {
                        Text("CompositionLocal Subscription Test", Modifier::empty());
                        Text("Counter: 0", Modifier::empty());
                        Text("Extra content", Modifier::empty());
                    });
                }
            });
        }
    })
    .expect("initial render succeeds");

    // Initial: root Column + Row with 2 Buttons (each with 1 Text) + nested Column + 2 Texts
    // = 1 + 1 + 2 + 2 + 1 + 2 = 9
    let initial = rule.applier_mut().len();
    println!("Initial node count (Counter View): {}", initial);

    // Click "CompositionLocal Test" button once
    show_counter.set(false);
    rule.pump_until_idle()
        .expect("first switch to composition local");
    let after_first_click = rule.applier_mut().len();
    println!(
        "After first click to CompositionLocal: {}",
        after_first_click
    );
    // Should be: 1 + 1 + 2 + 2 + 1 + 3 = 10
    assert_eq!(
        after_first_click, 10,
        "Node count changes due to different content"
    );

    // Click "CompositionLocal Test" button AGAIN (should be no-op since already showing that view)
    show_counter.set(false);
    rule.pump_until_idle()
        .expect("second click on composition local");
    let after_second_click = rule.applier_mut().len();
    println!(
        "After second click to CompositionLocal: {}",
        after_second_click
    );
    println!("\n=== Tree structure after second click ===");
    println!("{}", rule.dump_tree());

    // BUG: This is where duplication might happen
    assert_eq!(
        after_second_click, after_first_click,
        "BUG: Clicking the same button twice should not duplicate content"
    );

    // Switch to Counter App
    show_counter.set(true);
    rule.pump_until_idle().expect("switch to counter app");
    let after_switch_to_counter = rule.applier_mut().len();
    println!("After switch to Counter App: {}", after_switch_to_counter);
    assert_eq!(
        after_switch_to_counter, initial,
        "Should return to initial count"
    );

    // Click Counter App button again
    show_counter.set(true);
    rule.pump_until_idle().expect("second click on counter app");
    let after_second_counter_click = rule.applier_mut().len();
    println!(
        "After second click to Counter App: {}",
        after_second_counter_click
    );
    assert_eq!(
        after_second_counter_click, after_switch_to_counter,
        "Clicking Counter App button twice should not duplicate"
    );
}

#[composable]
fn test_composition_local_content_inner(local_holder: compose_core::CompositionLocal<i32>) {
    let value = local_holder.current();
    Text(
        format!("READING local: count={}", value),
        Modifier::empty().padding(8.0),
    );
}

#[composable]
fn test_composition_local_content(local_holder: compose_core::CompositionLocal<i32>) {
    Text(
        "Outside provider (NOT reading)",
        Modifier::empty().padding(8.0),
    );

    Spacer(Size {
        width: 0.0,
        height: 8.0,
    });

    test_composition_local_content_inner(local_holder.clone());

    Spacer(Size {
        width: 0.0,
        height: 8.0,
    });

    Text("NOT reading local", Modifier::empty().padding(8.0));
}

#[composable]
fn test_composition_local_demo(
    counter: MutableState<i32>,
    local_holder: compose_core::CompositionLocal<i32>,
) {
    Column(
        Modifier::empty().padding(20.0),
        ColumnSpec::default(),
        move || {
            Text(
                "CompositionLocal Subscription Test",
                Modifier::empty().padding(8.0),
            );

            Spacer(Size {
                width: 0.0,
                height: 12.0,
            });

            Text(
                format!("Counter: {}", counter.get()),
                Modifier::empty().padding(8.0),
            );

            Spacer(Size {
                width: 0.0,
                height: 12.0,
            });

            Button(
                Modifier::empty().padding(10.0),
                {
                    move || {
                        counter.set(counter.get() + 1);
                    }
                },
                || {
                    Text("Increment", Modifier::empty().padding(4.0));
                },
            );

            Spacer(Size {
                width: 0.0,
                height: 12.0,
            });

            let current_count = counter.get();
            CompositionLocalProvider(vec![local_holder.provides(current_count)], {
                let local_holder = local_holder.clone();
                move || {
                    test_composition_local_content(local_holder.clone());
                }
            });
        },
    );
}

#[test]
fn composition_local_increment_keeps_node_count_stable() {
    let mut rule = ComposeTestRule::new();
    let runtime = rule.runtime_handle();

    let counter = MutableState::with_runtime(0, runtime.clone());
    let local_holder = compositionLocalOf(|| 0);

    rule.set_content({
        let local_holder = local_holder.clone();
        move || {
            test_composition_local_demo(counter, local_holder.clone());
        }
    })
    .expect("initial render succeeds");

    let initial_nodes = rule.applier_mut().len();

    for step in 1..=2 {
        counter.set(counter.get() + 1);
        rule.pump_until_idle()
            .unwrap_or_else(|_| panic!("pump after increment {}", step));
        let nodes = rule.applier_mut().len();
        assert_eq!(
            nodes, initial_nodes,
            "node count should stay stable after increment {}",
            step
        );
    }
}

#[composable]
fn composable_view_a() {
    Column(
        Modifier::empty().padding(20.0),
        ColumnSpec::default(),
        || {
            Text("View A - Line 1", Modifier::empty());
            Text("View A - Line 2", Modifier::empty());
            Button(
                Modifier::empty(),
                || {},
                || {
                    Text("Button A", Modifier::empty());
                },
            );
        },
    );
}

#[composable]
fn composable_view_b() {
    Column(
        Modifier::empty().padding(20.0),
        ColumnSpec::default(),
        || {
            Text("View B - Line 1", Modifier::empty());
            Text("View B - Line 2", Modifier::empty());
            Text("View B - Line 3", Modifier::empty());
            Button(
                Modifier::empty(),
                || {},
                || {
                    Text("Button B", Modifier::empty());
                },
            );
        },
    );
}

#[test]
fn test_switching_between_composable_functions() {
    // This test specifically checks switching between @composable functions
    // which is the pattern used in the desktop app
    let mut rule = ComposeTestRule::new();
    let runtime = rule.runtime_handle();

    let show_a = MutableState::with_runtime(true, runtime.clone());

    rule.set_content({
        move || {
            let show_a_inner = show_a;
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                if show_a_inner.get() {
                    composable_view_a();
                } else {
                    composable_view_b();
                }
            });
        }
    })
    .expect("initial render succeeds");

    // View A: 1 root + 1 Column + 2 Texts + 1 Button + 1 Text = 6
    let initial = rule.applier_mut().len();
    println!("Initial (View A): {}", initial);
    assert_eq!(initial, 6);

    // Switch to View B
    show_a.set(false);
    rule.pump_until_idle().expect("switch to B");
    let after_first_switch = rule.applier_mut().len();
    println!("After switch to View B: {}", after_first_switch);
    // View B: 1 root + 1 Column + 3 Texts + 1 Button + 1 Text = 7
    assert_eq!(after_first_switch, 7);

    // Click to switch to B again (should be no-op)
    show_a.set(false);
    rule.pump_until_idle().expect("click B again");
    let after_second_b = rule.applier_mut().len();
    println!("After clicking B again: {}", after_second_b);
    assert_eq!(
        after_second_b, after_first_switch,
        "Clicking to switch to current view should not duplicate"
    );

    // Switch back to A
    show_a.set(true);
    rule.pump_until_idle().expect("switch back to A");
    let after_back_to_a = rule.applier_mut().len();
    println!("After switch back to A: {}", after_back_to_a);
    assert_eq!(after_back_to_a, initial);

    // Click A again
    show_a.set(true);
    rule.pump_until_idle().expect("click A again");
    let after_second_a = rule.applier_mut().len();
    println!("After clicking A again: {}", after_second_a);
    assert_eq!(
        after_second_a, after_back_to_a,
        "Clicking to switch to current view should not duplicate"
    );

    // Rapid switching
    for i in 0..5 {
        show_a.set(i % 2 == 0);
        rule.pump_until_idle()
            .unwrap_or_else(|_| panic!("rapid switch {}", i));
        let count = rule.applier_mut().len();
        let expected = if i % 2 == 0 { 6 } else { 7 };
        assert_eq!(
            count, expected,
            "Rapid switch {} should have correct count",
            i
        );
    }
}
