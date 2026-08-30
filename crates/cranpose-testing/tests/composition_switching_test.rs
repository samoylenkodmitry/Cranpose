use cranpose_core::{CompositionLocalProvider, MutableState, compositionLocalOf};
use cranpose_macros::composable;
use cranpose_testing::ComposeTestRule;
use cranpose_ui::*;

#[composable]
fn counter_view(counter: MutableState<i32>, render_count: MutableState<i32>) {
    render_count.set(render_count.get() + 1);

    Column(
        Modifier::empty().padding(20.0),
        ColumnSpec::default(),
        move || {
            Text(
                format!("Counter: {}", counter.get()),
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );

            Button(
                Modifier::empty().padding(10.0),
                ButtonSpec::default(),
                {
                    move || {
                        counter.set(counter.get() + 1);
                    }
                },
                || {
                    Text(
                        "Increment",
                        Modifier::empty().padding(4.0),
                        TextStyle::default(),
                    );
                },
            );
        },
    );
}

#[composable]
fn alternative_view(counter: MutableState<i32>, render_count: MutableState<i32>) {
    render_count.set(render_count.get() + 1);

    Column(
        Modifier::empty().padding(20.0),
        ColumnSpec::default(),
        move || {
            Text(
                format!("Alternative: {}", counter.get()),
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );

            Button(
                Modifier::empty().padding(10.0),
                ButtonSpec::default(),
                {
                    move || {
                        counter.set(counter.get() + 1);
                    }
                },
                || {
                    Text("Add", Modifier::empty().padding(4.0), TextStyle::default());
                },
            );
        },
    );
}

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

            Row(
                Modifier::empty().padding(8.0),
                RowSpec::default(),
                move || {
                    Button(
                        Modifier::empty().padding(10.0),
                        ButtonSpec::default(),
                        {
                            let show_counter = show_counter_for_button1;
                            move || {
                                show_counter.set(true);
                            }
                        },
                        || {
                            Text(
                                "Counter View",
                                Modifier::empty().padding(4.0),
                                TextStyle::default(),
                            );
                        },
                    );

                    Spacer(Size {
                        width: 8.0,
                        height: 0.0,
                    });

                    Button(
                        Modifier::empty().padding(10.0),
                        ButtonSpec::default(),
                        {
                            let show_counter = show_counter_for_button2;
                            move || {
                                show_counter.set(false);
                            }
                        },
                        || {
                            Text(
                                "Alternative View",
                                Modifier::empty().padding(4.0),
                                TextStyle::default(),
                            );
                        },
                    );
                },
            );

            Spacer(Size {
                width: 0.0,
                height: 12.0,
            });

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
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let mut rule = ComposeTestRule::new();
    let runtime = rule.runtime_handle();

    let show_counter = MutableState::with_runtime(true, runtime.clone());
    let counter1 = MutableState::with_runtime(0, runtime.clone());
    let counter2 = MutableState::with_runtime(0, runtime.clone());
    let render_count1 = MutableState::with_runtime(0, runtime.clone());
    let render_count2 = MutableState::with_runtime(0, runtime.clone());

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

    assert_eq!(
        after_switch_back_node_count, initial_node_count,
        "Node count should return to initial value after switching back"
    );
    assert_eq!(
        render_count2.get(),
        1,
        "Switching back should not re-render the alternative view"
    );

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

    assert_eq!(
        after_first_click, after_switch_back_node_count,
        "Node count should not change on first increment"
    );
    assert_eq!(
        after_second_click, after_switch_back_node_count,
        "Node count should not change on second increment"
    );

    show_counter.set(false);
    rule.pump_until_idle()
        .expect("recompose after switching to alternative");
    let after_second_switch = rule.applier_mut().len();
    println!("After second switch node count: {}", after_second_switch);

    counter2.set(1);
    rule.pump_until_idle().expect("recompose after first add");
    let after_first_add = rule.applier_mut().len();
    println!("After first add node count: {}", after_first_add);

    counter2.set(2);
    rule.pump_until_idle().expect("recompose after second add");
    let after_second_add = rule.applier_mut().len();
    println!("After second add node count: {}", after_second_add);

    assert_eq!(
        after_first_add, after_second_switch,
        "Node count should not change on first add"
    );
    assert_eq!(
        after_second_add, after_second_switch,
        "Node count should not change on second add"
    );

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
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
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
                        Text("First A", Modifier::empty(), TextStyle::default());
                        Text("First B", Modifier::empty(), TextStyle::default());
                        Text("First C", Modifier::empty(), TextStyle::default());
                    } else {
                        Text("Second A", Modifier::empty(), TextStyle::default());
                        Text("Second B", Modifier::empty(), TextStyle::default());
                    }
                },
            );
        }
    })
    .expect("initial render succeeds");

    let initial_count = rule.applier_mut().len();
    println!("Initial node count (first view): {}", initial_count);
    assert_eq!(initial_count, 4, "Should have Column + 3 Text nodes");

    show_first.set(false);
    rule.pump_until_idle().expect("recompose after switch");

    let after_switch = rule.applier_mut().len();
    println!("Node count after switch (second view): {}", after_switch);
    assert_eq!(after_switch, 3, "Should have Column + 2 Text nodes");

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
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
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
                        TextStyle::default(),
                    );
                    Button(
                        Modifier::empty(),
                        ButtonSpec::default(),
                        {
                            let counter_a = counter_a_inner;
                            move || counter_a.set(counter_a.get() + 1)
                        },
                        || {
                            Text("Increment A", Modifier::empty(), TextStyle::default());
                        },
                    );
                } else {
                    Text(
                        format!("View B: {}", counter_b_inner.get()),
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                    Button(
                        Modifier::empty(),
                        ButtonSpec::default(),
                        {
                            let counter_b = counter_b_inner;
                            move || counter_b.set(counter_b.get() + 1)
                        },
                        || {
                            Text("Increment B", Modifier::empty(), TextStyle::default());
                        },
                    );
                }
            });
        }
    })
    .expect("initial render succeeds");

    let baseline_count = rule.applier_mut().len();
    println!("Baseline node count: {}", baseline_count);

    assert_eq!(counter_a.get(), 0);

    show_view_a.set(false);
    rule.pump_until_idle().expect("switch to view B");
    let after_switch_to_b = rule.applier_mut().len();
    println!("After switch to View B: {}", after_switch_to_b);
    assert_eq!(
        after_switch_to_b, baseline_count,
        "Node count should be same (both views have same structure)"
    );

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

    assert_eq!(
        after_second_click_b, baseline_count,
        "BUG: Content should not be duplicated after two clicks in View B"
    );

    show_view_a.set(true);
    rule.pump_until_idle().expect("switch back to view A");
    let after_switch_to_a = rule.applier_mut().len();
    println!("After switch back to View A: {}", after_switch_to_a);
    assert_eq!(
        after_switch_to_a, baseline_count,
        "Node count should return to baseline after switching back"
    );

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

    assert_eq!(
        after_second_click_a, baseline_count,
        "BUG: Content should not be duplicated after two clicks in View A"
    );
}

#[test]
fn test_deeply_nested_conditional_switching() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
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
                            Text("Outer A, Inner A", Modifier::empty(), TextStyle::default());
                            Text("Nested content A", Modifier::empty(), TextStyle::default());
                        } else {
                            Text("Outer A, Inner B", Modifier::empty(), TextStyle::default());
                        }
                    });
                } else {
                    Text("Outer B", Modifier::empty(), TextStyle::default());
                }
            });
        }
    })
    .expect("initial render succeeds");

    let initial = rule.applier_mut().len();
    assert_eq!(initial, 4, "Should have nested structure");

    show_inner.set(false);
    rule.pump_until_idle().expect("switch inner");
    let after_inner_switch = rule.applier_mut().len();
    assert_eq!(
        after_inner_switch, 3,
        "Should have fewer nodes after inner switch"
    );

    show_outer.set(false);
    rule.pump_until_idle().expect("switch outer");
    let after_outer_switch = rule.applier_mut().len();
    assert_eq!(after_outer_switch, 2, "Should have minimal nodes");

    show_outer.set(true);
    show_inner.set(true);
    rule.pump_until_idle().expect("restore initial");
    let restored = rule.applier_mut().len();
    assert_eq!(restored, initial, "Should restore original node count");
}

#[test]
fn test_switching_with_different_node_counts() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let mut rule = ComposeTestRule::new();
    let runtime = rule.runtime_handle();

    let view_type = MutableState::with_runtime(0, runtime.clone());

    rule.set_content({
        move || {
            let view_type_inner = view_type;
            Column(
                Modifier::empty(),
                ColumnSpec::default(),
                move || match view_type_inner.get() {
                    0 => {
                        Text("Single", Modifier::empty(), TextStyle::default());
                    }
                    1 => {
                        Text("Triple 1", Modifier::empty(), TextStyle::default());
                        Text("Triple 2", Modifier::empty(), TextStyle::default());
                        Text("Triple 3", Modifier::empty(), TextStyle::default());
                    }
                    2 => {
                        Column(Modifier::empty(), ColumnSpec::default(), || {
                            Text("Nested 1", Modifier::empty(), TextStyle::default());
                            Text("Nested 2", Modifier::empty(), TextStyle::default());
                        });
                        Text("Extra", Modifier::empty(), TextStyle::default());
                    }
                    _ => {}
                },
            );
        }
    })
    .expect("initial render succeeds");

    let count0 = rule.applier_mut().len();
    assert_eq!(count0, 2);

    view_type.set(1);
    rule.pump_until_idle().expect("switch to view 1");
    let count1 = rule.applier_mut().len();
    assert_eq!(count1, 4);

    view_type.set(2);
    rule.pump_until_idle().expect("switch to view 2");
    let count2 = rule.applier_mut().len();
    assert_eq!(count2, 5);

    view_type.set(0);
    rule.pump_until_idle().expect("switch back to view 0");
    let count0_again = rule.applier_mut().len();
    assert_eq!(count0_again, count0, "Should return to original count");

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
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
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
                    Column(Modifier::empty(), ColumnSpec::default(), {
                        let counter = counter_inner;
                        move || {
                            Text("First View", Modifier::empty(), TextStyle::default());
                            Button(
                                Modifier::empty(),
                                ButtonSpec::default(),
                                move || counter.set(counter.get() + 1),
                                || {
                                    Text("Button 1", Modifier::empty(), TextStyle::default());
                                },
                            );
                            Button(
                                Modifier::empty(),
                                ButtonSpec::default(),
                                move || counter.set(counter.get() + 10),
                                || {
                                    Text("Button 2", Modifier::empty(), TextStyle::default());
                                },
                            );
                        }
                    });
                } else {
                    Text("Second View", Modifier::empty(), TextStyle::default());
                    Button(
                        Modifier::empty(),
                        ButtonSpec::default(),
                        {
                            let counter = counter_inner;
                            move || counter.set(counter.get() - 1)
                        },
                        || {
                            Text("Decrement", Modifier::empty(), TextStyle::default());
                        },
                    );
                }
            });
        }
    })
    .expect("initial render succeeds");

    let initial = rule.applier_mut().len();
    println!("Initial node count: {}", initial);
    assert_eq!(initial, 7);

    counter.set(5);
    rule.pump_until_idle().expect("update counter");
    assert_eq!(
        rule.applier_mut().len(),
        initial,
        "Node count stable after state change"
    );

    show_first.set(false);
    rule.pump_until_idle().expect("switch to second");
    let after_switch = rule.applier_mut().len();
    assert_eq!(after_switch, 4);

    counter.set(3);
    rule.pump_until_idle()
        .expect("update counter in second view");
    assert_eq!(
        rule.applier_mut().len(),
        after_switch,
        "Node count stable in second view"
    );

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
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let mut rule = ComposeTestRule::new();
    let runtime = rule.runtime_handle();

    let show_counter = MutableState::with_runtime(true, runtime.clone());

    rule.set_content({
        move || {
            let show_counter_copy = show_counter;
            let show_counter_for_button = show_counter;
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                Row(Modifier::empty(), RowSpec::default(), {
                    let show_counter = show_counter_for_button;
                    move || {
                        let show_counter_for_btn1 = show_counter;
                        let show_counter_for_btn2 = show_counter;

                        Button(
                            Modifier::empty(),
                            ButtonSpec::default(),
                            move || show_counter_for_btn1.set(true),
                            || {
                                Text("Counter App", Modifier::empty(), TextStyle::default());
                            },
                        );

                        Button(
                            Modifier::empty(),
                            ButtonSpec::default(),
                            move || show_counter_for_btn2.set(false),
                            || {
                                Text(
                                    "CompositionLocal Test",
                                    Modifier::empty(),
                                    TextStyle::default(),
                                );
                            },
                        );
                    }
                });

                if show_counter_copy.get() {
                    Column(Modifier::empty(), ColumnSpec::default(), || {
                        Text("Counter View", Modifier::empty(), TextStyle::default());
                        Text("Line 2", Modifier::empty(), TextStyle::default());
                    });
                } else {
                    Column(Modifier::empty(), ColumnSpec::default(), || {
                        Text(
                            "CompositionLocal Subscription Test",
                            Modifier::empty(),
                            TextStyle::default(),
                        );
                        Text("Counter: 0", Modifier::empty(), TextStyle::default());
                        Text("Extra content", Modifier::empty(), TextStyle::default());
                    });
                }
            });
        }
    })
    .expect("initial render succeeds");

    let initial = rule.applier_mut().len();
    println!("Initial node count (Counter View): {}", initial);

    show_counter.set(false);
    rule.pump_until_idle()
        .expect("first switch to composition local");
    let after_first_click = rule.applier_mut().len();
    println!(
        "After first click to CompositionLocal: {}",
        after_first_click
    );
    assert_eq!(
        after_first_click, 10,
        "Node count changes due to different content"
    );

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

    assert_eq!(
        after_second_click, after_first_click,
        "BUG: Clicking the same button twice should not duplicate content"
    );

    show_counter.set(true);
    rule.pump_until_idle().expect("switch to counter app");
    let after_switch_to_counter = rule.applier_mut().len();
    println!("After switch to Counter App: {}", after_switch_to_counter);
    assert_eq!(
        after_switch_to_counter, initial,
        "Should return to initial count"
    );

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
fn test_composition_local_content_inner(local_holder: cranpose_core::CompositionLocal<i32>) {
    let value = local_holder.current();
    Text(
        format!("READING local: count={}", value),
        Modifier::empty().padding(8.0),
        TextStyle::default(),
    );
}

#[composable]
fn test_composition_local_content(local_holder: cranpose_core::CompositionLocal<i32>) {
    Text(
        "Outside provider (NOT reading)",
        Modifier::empty().padding(8.0),
        TextStyle::default(),
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

    Text(
        "NOT reading local",
        Modifier::empty().padding(8.0),
        TextStyle::default(),
    );
}

#[composable]
fn test_composition_local_demo(
    counter: MutableState<i32>,
    local_holder: cranpose_core::CompositionLocal<i32>,
) {
    Column(
        Modifier::empty().padding(20.0),
        ColumnSpec::default(),
        move || {
            Text(
                "CompositionLocal Subscription Test",
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );

            Spacer(Size {
                width: 0.0,
                height: 12.0,
            });

            Text(
                format!("Counter: {}", counter.get()),
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );

            Spacer(Size {
                width: 0.0,
                height: 12.0,
            });

            Button(
                Modifier::empty().padding(10.0),
                ButtonSpec::default(),
                {
                    move || {
                        counter.set(counter.get() + 1);
                    }
                },
                || {
                    Text(
                        "Increment",
                        Modifier::empty().padding(4.0),
                        TextStyle::default(),
                    );
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
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
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
            Text("View A - Line 1", Modifier::empty(), TextStyle::default());
            Text("View A - Line 2", Modifier::empty(), TextStyle::default());
            Button(
                Modifier::empty(),
                ButtonSpec::default(),
                || {},
                || {
                    Text("Button A", Modifier::empty(), TextStyle::default());
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
            Text("View B - Line 1", Modifier::empty(), TextStyle::default());
            Text("View B - Line 2", Modifier::empty(), TextStyle::default());
            Text("View B - Line 3", Modifier::empty(), TextStyle::default());
            Button(
                Modifier::empty(),
                ButtonSpec::default(),
                || {},
                || {
                    Text("Button B", Modifier::empty(), TextStyle::default());
                },
            );
        },
    );
}
