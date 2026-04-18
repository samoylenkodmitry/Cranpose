use super::*;

// ============================================================================
// Slot Table Unit Tests - Explicit Hidden Entries
// ============================================================================

#[test]
fn slot_table_marks_values_hidden() {
    let mut slots = test_slot_table();

    // Create initial composition with 3 value slots
    let _idx1 = slots.use_value_slot(|| 1i32);
    let _idx2 = slots.use_value_slot(|| 2i32);
    let _idx3 = slots.use_value_slot(|| 3i32);

    // Hide the middle slot
    hide_test_range(&mut slots, 1, 2, None);

    // Verify we can still read first and last slots
    assert_eq!(slots.read_value::<i32>(0), &1);
    assert_eq!(slots.read_value::<i32>(2), &3);
}

#[test]
fn slot_table_restores_hidden_values() {
    let mut slots = test_slot_table();

    // Create initial value
    let idx1 = slots.use_value_slot(|| 1i32);
    assert_eq!(slots.read_value::<i32>(idx1), &1);

    // Hide the slot.
    slots.reset();
    hide_test_range(&mut slots, 0, 1, None);

    // Reset cursor to restore.
    slots.reset();

    let initialized = Cell::new(false);
    let idx2 = slots.use_value_slot(|| {
        initialized.set(true);
        42i32
    });

    assert_eq!(idx2, 0, "hidden value should restore in place");
    assert!(
        !initialized.get(),
        "restoring a hidden value must not reinitialize it"
    );
    assert_eq!(slots.read_value::<i32>(idx2), &1);
}

#[test]
fn slot_table_replaces_mismatched_value_types() {
    let mut slots = test_slot_table();

    // Create initial value of type i32
    let idx = slots.use_value_slot(|| 1i32);
    assert_eq!(slots.read_value::<i32>(idx), &1);

    // Reset and try to use with different type
    slots.reset();
    let idx2 = slots.use_value_slot(|| "hello");

    // Should replace at same position
    assert_eq!(idx, idx2);
    assert_eq!(slots.read_value::<&str>(idx2), &"hello");
}

#[test]
fn slot_table_handles_nested_hidden_groups() {
    let mut slots = test_slot_table();

    // Create a parent group
    let parent_idx = begin_test_group(&mut slots, 100).0;

    // Create child group
    let child_idx = begin_test_group(&mut slots, 200).0;
    let _val_idx = slots.use_value_slot(|| 42i32);
    slots.end_group(); // End child

    slots.end_group(); // End parent

    // Verify parent group was created
    let groups = slots.debug_dump_groups();
    assert!(groups.iter().any(|(idx, _, _, _)| *idx == parent_idx));

    // Hide the parent group range. The child subtree should become hidden too.
    hide_test_range(&mut slots, parent_idx, child_idx + 2, None);

    // Structure remains preserved for reuse through hidden entries.
}

#[test]
fn slot_table_preserves_sibling_groups_when_hiding_ranges() {
    let mut slots = test_slot_table();

    // Create first group with a value
    let g1 = begin_test_group(&mut slots, 1).0;
    let _v1 = slots.use_value_slot(|| "first");
    slots.end_group();

    // Create second group with a value
    let g2 = begin_test_group(&mut slots, 2).0;
    let _v2 = slots.use_value_slot(|| "second");
    slots.end_group();

    // Create third group with a value
    let _g3 = begin_test_group(&mut slots, 3);
    let v3_idx = slots.use_value_slot(|| "third");
    slots.end_group();

    // Capture initial group count
    let initial_groups = slots.debug_dump_groups();
    assert_eq!(initial_groups.len(), 3, "should have 3 groups initially");

    // Hide only the first group's range.
    hide_test_range(&mut slots, g1, g2, None);

    // Third group should still be accessible (the value should remain)
    assert_eq!(slots.read_value::<&str>(v3_idx), &"third");

    // Groups outside the hidden range remain live.
    let remaining_groups = slots.debug_dump_groups();
    assert!(
        remaining_groups.len() >= 2,
        "groups outside marked range should be preserved, found {} groups",
        remaining_groups.len()
    );
}

#[test]
fn slot_table_tab_switching_preserves_scopes() {
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());
    let tab1_counter = MutableState::with_runtime(0i32, runtime.clone());
    let tab2_counter = MutableState::with_runtime(0i32, runtime.clone());

    thread_local! {
        static TAB1_RENDERS: Cell<usize> = const { Cell::new(0) };
        static TAB2_RENDERS: Cell<usize> = const { Cell::new(0) };
    }

    #[composable]
    fn tab_content_1(counter: MutableState<i32>) {
        TAB1_RENDERS.with(|c| c.set(c.get() + 1));
        let count = counter.value();
        let id = cranpose_test_node(TestTextNode::default);
        with_node_mut(id, |node: &mut TestTextNode| {
            node.text = format!("Tab 1: {}", count);
        })
        .expect("update tab1 node");
    }

    #[composable]
    fn tab_content_2(counter: MutableState<i32>) {
        TAB2_RENDERS.with(|c| c.set(c.get() + 1));
        let count = counter.value();
        let id = cranpose_test_node(TestTextNode::default);
        with_node_mut(id, |node: &mut TestTextNode| {
            node.text = format!("Tab 2: {}", count);
        })
        .expect("update tab2 node");
    }

    let mut render = {
        move || {
            let tab = active_tab.value();
            match tab {
                0 => tab_content_1(tab1_counter),
                1 => tab_content_2(tab2_counter),
                _ => {}
            }
        }
    };

    // Initial render - Tab 1
    TAB1_RENDERS.with(|c| c.set(0));
    TAB2_RENDERS.with(|c| c.set(0));

    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, &mut render)
        .expect("initial render");

    assert_eq!(
        TAB1_RENDERS.with(|c| c.get()),
        1,
        "tab1 should render initially"
    );
    assert_eq!(
        TAB2_RENDERS.with(|c| c.get()),
        0,
        "tab2 should not render initially"
    );

    // Switch to Tab 2
    active_tab.set_value(1);
    composition
        .render(key, &mut render)
        .expect("switch to tab2");

    assert_eq!(
        TAB1_RENDERS.with(|c| c.get()),
        1,
        "tab1 render count unchanged"
    );
    assert_eq!(
        TAB2_RENDERS.with(|c| c.get()),
        1,
        "tab2 should render after switch"
    );

    // Update tab2 counter - should trigger recomposition
    TAB1_RENDERS.with(|c| c.set(0));
    TAB2_RENDERS.with(|c| c.set(0));
    tab2_counter.set_value(5);

    let _ = composition
        .process_invalid_scopes()
        .expect("recompose tab2");

    assert_eq!(
        TAB1_RENDERS.with(|c| c.get()),
        0,
        "tab1 should not recompose"
    );
    assert!(
        TAB2_RENDERS.with(|c| c.get()) > 0,
        "tab2 should recompose on counter change"
    );

    // Switch back to Tab 1
    TAB1_RENDERS.with(|c| c.set(0));
    TAB2_RENDERS.with(|c| c.set(0));
    active_tab.set_value(0);
    composition
        .render(key, &mut render)
        .expect("switch back to tab1");

    assert!(
        TAB1_RENDERS.with(|c| c.get()) > 0,
        "tab1 should render after switch back"
    );
    assert_eq!(TAB2_RENDERS.with(|c| c.get()), 0, "tab2 should not render");

    // Update tab1 counter - should trigger recomposition even after tab switch cycle
    TAB1_RENDERS.with(|c| c.set(0));
    TAB2_RENDERS.with(|c| c.set(0));
    tab1_counter.set_value(10);

    let _ = composition
        .process_invalid_scopes()
        .expect("recompose tab1 after cycle");

    assert!(
        TAB1_RENDERS.with(|c| c.get()) > 0,
        "tab1 scope should work after tab cycle"
    );
    assert_eq!(
        TAB2_RENDERS.with(|c| c.get()),
        0,
        "tab2 should not recompose"
    );
}

#[test]
fn slot_table_conditional_rendering_preserves_sibling_scopes() {
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let show_middle = MutableState::with_runtime(true, runtime.clone());
    let top_counter = MutableState::with_runtime(0i32, runtime.clone());
    let middle_counter = MutableState::with_runtime(0i32, runtime.clone());
    let bottom_counter = MutableState::with_runtime(0i32, runtime.clone());

    thread_local! {
        static TOP_RENDERS: Cell<usize> = const { Cell::new(0) };
        static MIDDLE_RENDERS: Cell<usize> = const { Cell::new(0) };
        static BOTTOM_RENDERS: Cell<usize> = const { Cell::new(0) };
    }

    #[composable]
    fn top_component(counter: MutableState<i32>) {
        TOP_RENDERS.with(|c| c.set(c.get() + 1));
        let count = counter.value();
        let id = cranpose_test_node(TestTextNode::default);
        with_node_mut(id, |node: &mut TestTextNode| {
            node.text = format!("Top: {}", count);
        })
        .expect("update top node");
    }

    #[composable]
    fn middle_component(counter: MutableState<i32>) {
        MIDDLE_RENDERS.with(|c| c.set(c.get() + 1));
        let count = counter.value();
        let id = cranpose_test_node(TestTextNode::default);
        with_node_mut(id, |node: &mut TestTextNode| {
            node.text = format!("Middle: {}", count);
        })
        .expect("update middle node");
    }

    #[composable]
    fn bottom_component(counter: MutableState<i32>) {
        BOTTOM_RENDERS.with(|c| c.set(c.get() + 1));
        let count = counter.value();
        let id = cranpose_test_node(TestTextNode::default);
        with_node_mut(id, |node: &mut TestTextNode| {
            node.text = format!("Bottom: {}", count);
        })
        .expect("update bottom node");
    }

    let mut render = {
        move || {
            top_component(top_counter);

            if show_middle.value() {
                middle_component(middle_counter);
            }

            bottom_component(bottom_counter);
        }
    };

    // Initial render with all components
    TOP_RENDERS.with(|c| c.set(0));
    MIDDLE_RENDERS.with(|c| c.set(0));
    BOTTOM_RENDERS.with(|c| c.set(0));

    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, &mut render)
        .expect("initial render");

    assert_eq!(TOP_RENDERS.with(|c| c.get()), 1);
    assert_eq!(MIDDLE_RENDERS.with(|c| c.get()), 1);
    assert_eq!(BOTTOM_RENDERS.with(|c| c.get()), 1);

    // Hide middle component
    show_middle.set_value(false);
    composition.render(key, &mut render).expect("hide middle");

    // Update bottom counter - should still work
    TOP_RENDERS.with(|c| c.set(0));
    MIDDLE_RENDERS.with(|c| c.set(0));
    BOTTOM_RENDERS.with(|c| c.set(0));
    bottom_counter.set_value(5);

    let _ = composition
        .process_invalid_scopes()
        .expect("recompose bottom");

    assert_eq!(TOP_RENDERS.with(|c| c.get()), 0, "top should not recompose");
    assert_eq!(
        MIDDLE_RENDERS.with(|c| c.get()),
        0,
        "middle should not recompose"
    );
    assert!(
        BOTTOM_RENDERS.with(|c| c.get()) > 0,
        "bottom scope should work after middle removed"
    );

    // Update top counter - should still work
    TOP_RENDERS.with(|c| c.set(0));
    MIDDLE_RENDERS.with(|c| c.set(0));
    BOTTOM_RENDERS.with(|c| c.set(0));
    top_counter.set_value(3);

    let _ = composition.process_invalid_scopes().expect("recompose top");

    assert!(
        TOP_RENDERS.with(|c| c.get()) > 0,
        "top scope should work after middle removed"
    );
    assert_eq!(
        MIDDLE_RENDERS.with(|c| c.get()),
        0,
        "middle should not recompose"
    );
    assert_eq!(
        BOTTOM_RENDERS.with(|c| c.get()),
        0,
        "bottom should not recompose"
    );

    // Show middle again
    show_middle.set_value(true);
    composition
        .render(key, &mut render)
        .expect("show middle again");

    // Update middle counter - should work after restoration
    TOP_RENDERS.with(|c| c.set(0));
    MIDDLE_RENDERS.with(|c| c.set(0));
    BOTTOM_RENDERS.with(|c| c.set(0));
    middle_counter.set_value(7);

    let _ = composition
        .process_invalid_scopes()
        .expect("recompose middle after restore");

    assert_eq!(TOP_RENDERS.with(|c| c.get()), 0, "top should not recompose");
    assert!(
        MIDDLE_RENDERS.with(|c| c.get()) > 0,
        "middle scope should work after restoration"
    );
    assert_eq!(
        BOTTOM_RENDERS.with(|c| c.get()),
        0,
        "bottom should not recompose"
    );
}

#[test]
fn slot_table_gaps_work_with_nested_conditionals() {
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let outer_visible = MutableState::with_runtime(true, runtime.clone());
    let inner_visible = MutableState::with_runtime(true, runtime.clone());
    let outer_counter = MutableState::with_runtime(0i32, runtime.clone());
    let inner_counter = MutableState::with_runtime(0i32, runtime.clone());
    let after_counter = MutableState::with_runtime(0i32, runtime.clone());

    thread_local! {
        static OUTER_RENDERS: Cell<usize> = const { Cell::new(0) };
        static INNER_RENDERS: Cell<usize> = const { Cell::new(0) };
        static AFTER_RENDERS: Cell<usize> = const { Cell::new(0) };
    }

    #[composable]
    fn inner_content(counter: MutableState<i32>) {
        INNER_RENDERS.with(|c| c.set(c.get() + 1));
        let _count = counter.value();
    }

    #[composable]
    fn outer_content(
        inner_visible: MutableState<bool>,
        outer_counter: MutableState<i32>,
        inner_counter: MutableState<i32>,
    ) {
        OUTER_RENDERS.with(|c| c.set(c.get() + 1));
        let _count = outer_counter.value();

        if inner_visible.value() {
            inner_content(inner_counter);
        }
    }

    #[composable]
    fn after_content(counter: MutableState<i32>) {
        AFTER_RENDERS.with(|c| c.set(c.get() + 1));
        let _count = counter.value();
    }

    let mut render = {
        move || {
            if outer_visible.value() {
                outer_content(inner_visible, outer_counter, inner_counter);
            }
            after_content(after_counter);
        }
    };

    let key = location_key(file!(), line!(), column!());

    // Initial render - all visible
    composition
        .render(key, &mut render)
        .expect("initial render");

    // Hide inner
    inner_visible.set_value(false);
    composition.render(key, &mut render).expect("hide inner");

    // Verify after component scope still works
    AFTER_RENDERS.with(|c| c.set(0));
    after_counter.set_value(1);
    let _ = composition
        .process_invalid_scopes()
        .expect("recompose after");
    assert!(
        AFTER_RENDERS.with(|c| c.get()) > 0,
        "after scope should work with inner hidden"
    );

    // Hide outer (and inner is already hidden)
    outer_visible.set_value(false);
    composition.render(key, &mut render).expect("hide outer");

    // Verify after component scope still works
    AFTER_RENDERS.with(|c| c.set(0));
    after_counter.set_value(2);
    let _ = composition
        .process_invalid_scopes()
        .expect("recompose after with outer hidden");
    assert!(
        AFTER_RENDERS.with(|c| c.get()) > 0,
        "after scope should work with outer hidden"
    );

    // Show outer but keep inner hidden
    outer_visible.set_value(true);
    composition.render(key, &mut render).expect("show outer");

    // Verify outer scope works
    OUTER_RENDERS.with(|c| c.set(0));
    outer_counter.set_value(1);
    let _ = composition
        .process_invalid_scopes()
        .expect("recompose outer");
    assert!(
        OUTER_RENDERS.with(|c| c.get()) > 0,
        "outer scope should work after restoration"
    );

    // Show inner too
    inner_visible.set_value(true);
    composition.render(key, &mut render).expect("show inner");

    // Verify inner scope works after full restoration
    INNER_RENDERS.with(|c| c.set(0));
    inner_counter.set_value(1);
    let _ = composition
        .process_invalid_scopes()
        .expect("recompose inner");
    assert!(
        INNER_RENDERS.with(|c| c.get()) > 0,
        "inner scope should work after full restoration"
    );
}

#[test]
fn slot_table_multiple_rapid_tab_switches() {
    // Simulates rapid tab switching that could cause UI corruption
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());

    thread_local! {
        static RENDER_LOG: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    #[composable]
    fn tab_with_multiple_elements(tab_id: i32, counter: MutableState<i32>) {
        RENDER_LOG.with(|log| log.borrow_mut().push(format!("tab{}_start", tab_id)));

        // Multiple nodes to make the slot table more complex
        let count = counter.value();
        for i in 0..3 {
            let id = cranpose_test_node(TestTextNode::default);
            with_node_mut(id, |node: &mut TestTextNode| {
                node.text = format!("Tab {} Item {} ({})", tab_id, i, count);
            })
            .expect("update node");
        }

        RENDER_LOG.with(|log| log.borrow_mut().push(format!("tab{}_end", tab_id)));
    }

    let tab_counters: Vec<_> = (0..4)
        .map(|_| MutableState::with_runtime(0i32, runtime.clone()))
        .collect();

    let mut render = {
        let tab_counters = tab_counters.clone();
        move || {
            let tab = active_tab.value();
            if tab >= 0 && (tab as usize) < tab_counters.len() {
                tab_with_multiple_elements(tab, tab_counters[tab as usize]);
            }
        }
    };

    let key = location_key(file!(), line!(), column!());

    // Rapidly switch between tabs multiple times
    for cycle in 0..3 {
        for tab in 0..4 {
            RENDER_LOG.with(|log| log.borrow_mut().clear());
            active_tab.set_value(tab);
            composition
                .render(key, &mut render)
                .unwrap_or_else(|_| panic!("render cycle {} tab {}", cycle, tab));

            let log = RENDER_LOG.with(|log| log.borrow().clone());
            assert!(
                log.len() >= 2,
                "cycle {} tab {} should render start and end markers, got {:?}",
                cycle,
                tab,
                log
            );
            assert!(
                log[0].starts_with(&format!("tab{}_start", tab)),
                "cycle {} tab {} should start correctly, got {:?}",
                cycle,
                tab,
                log
            );
        }
    }

    // After all tab switches, counters should still work
    RENDER_LOG.with(|log| log.borrow_mut().clear());
    tab_counters[2].set_value(42);
    active_tab.set_value(2);
    composition.render(key, &mut render).expect("final render");

    let _ = composition
        .process_invalid_scopes()
        .expect("final recompose");

    // If scopes are preserved correctly, the counter update should have triggered recomposition
    let final_log = RENDER_LOG.with(|log| log.borrow().clone());
    assert!(
        final_log.len() >= 2,
        "final render should work after rapid tab switches, got {:?}",
        final_log
    );
}

#[test]
fn tab_switching_with_keyed_children() {
    // Test tab switching where children use keys for identity
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());
    let counter = MutableState::with_runtime(0i32, runtime.clone());

    thread_local! {
        static TAB1_KEYED_RENDERS: Cell<usize> = const { Cell::new(0) };
        static TAB2_KEYED_RENDERS: Cell<usize> = const { Cell::new(0) };
    }

    #[composable]
    fn keyed_content(tab_id: i32, counter: MutableState<i32>) {
        if tab_id == 0 {
            TAB1_KEYED_RENDERS.with(|c| c.set(c.get() + 1));
        } else {
            TAB2_KEYED_RENDERS.with(|c| c.set(c.get() + 1));
        }

        let count = counter.value();
        cranpose_core::with_key(&format!("item_{}", tab_id), || {
            let id = cranpose_test_node(TestTextNode::default);
            with_node_mut(id, |node: &mut TestTextNode| {
                node.text = format!("Tab {} with key: {}", tab_id, count);
            })
            .expect("update keyed node");
        });
    }

    let mut render = {
        move || {
            let tab = active_tab.value();
            keyed_content(tab, counter);
        }
    };

    let key = location_key(file!(), line!(), column!());

    // Initial render - Tab 0
    TAB1_KEYED_RENDERS.with(|c| c.set(0));
    TAB2_KEYED_RENDERS.with(|c| c.set(0));
    composition
        .render(key, &mut render)
        .expect("initial render");
    assert_eq!(TAB1_KEYED_RENDERS.with(|c| c.get()), 1);

    // Switch to Tab 1
    active_tab.set_value(1);
    composition
        .render(key, &mut render)
        .expect("switch to tab 1");
    assert_eq!(TAB2_KEYED_RENDERS.with(|c| c.get()), 1);

    // Update counter and switch back to Tab 0
    counter.set_value(42);
    active_tab.set_value(0);
    TAB1_KEYED_RENDERS.with(|c| c.set(0));
    composition
        .render(key, &mut render)
        .expect("switch back to tab 0");

    // Tab 0 should rerender with updated counter
    assert!(
        TAB1_KEYED_RENDERS.with(|c| c.get()) > 0,
        "Tab 0 should rerender with updated counter value"
    );

    // Verify scope still works
    TAB1_KEYED_RENDERS.with(|c| c.set(0));
    counter.set_value(100);
    let _ = composition
        .process_invalid_scopes()
        .expect("recompose after counter update");
    assert!(
        TAB1_KEYED_RENDERS.with(|c| c.get()) > 0,
        "Tab 0 scope should still work after key-based tab switching"
    );
}

#[test]
fn stable_outer_parent_node_survives_keyed_inner_switch() {
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());

    #[composable]
    fn keyed_switching_tree(active_tab: MutableState<i32>) {
        let active = active_tab.value();
        with_current_composer(|composer| {
            let root = composer.emit_node(RecordingNode::default);
            composer.push_parent(root);

            let _tab_bar = composer.emit_node(TrackingChild::default);
            let _spacer = composer.emit_node(TrackingChild::default);

            let outer_parent = composer.emit_node(RecordingNode::default);
            STABLE_OUTER_PARENT_ID.with(|slot| slot.set(Some(outer_parent)));
            composer.push_parent(outer_parent);

            cranpose_core::with_key(&active, || {
                if active == 0 {
                    let panel = composer.emit_node(RecordingNode::default);
                    composer.push_parent(panel);
                    composer.emit_node(|| TrackingChild {
                        label: "counter".to_string(),
                        ..Default::default()
                    });
                    composer.pop_parent();
                } else {
                    let panel = composer.emit_node(RecordingNode::default);
                    composer.push_parent(panel);
                    composer.emit_node(|| TrackingChild {
                        label: "animations-a".to_string(),
                        ..Default::default()
                    });
                    composer.emit_node(|| TrackingChild {
                        label: "animations-b".to_string(),
                        ..Default::default()
                    });
                    composer.pop_parent();
                }
            });

            composer.pop_parent();
            composer.pop_parent();
        });
    }

    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, &mut || keyed_switching_tree(active_tab))
        .expect("initial render");

    let outer_parent_id = STABLE_OUTER_PARENT_ID
        .with(Cell::get)
        .expect("outer parent id captured");
    assert!(
        composition.applier_mut().get_mut(outer_parent_id).is_ok(),
        "outer parent should exist after initial render",
    );

    active_tab.set_value(1);
    let recomposed = composition
        .process_invalid_scopes()
        .expect("recompose after keyed switch");
    assert!(recomposed, "switching tabs should trigger recomposition");

    let root_id = composition.root().expect("root node exists");
    let root_children = composition
        .applier_mut()
        .with_node(root_id, |node: &mut RecordingNode| node.children.clone())
        .expect("root recording node exists");
    let outer_parent_exists = composition.applier_mut().get_mut(outer_parent_id).is_ok();
    let slot_owns_outer_parent = composition
        .debug_dump_all_slots()
        .iter()
        .any(|(_, desc)| desc == &format!("Node(id={outer_parent_id})"));

    assert!(
        outer_parent_exists,
        "stable outer parent node was removed during keyed inner switch: root_children={root_children:?}",
    );
    assert!(
        root_children.contains(&outer_parent_id),
        "root should still reference outer parent after keyed inner switch: root_children={root_children:?}",
    );
    assert!(
        slot_owns_outer_parent,
        "slot table must continue owning the stable outer parent node after keyed inner switch",
    );
}

#[test]
fn tab_switching_with_different_node_types() {
    // Test switching between tabs that create different node types
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());

    thread_local! {
        static TEXT_NODE_COUNT: Cell<usize> = const { Cell::new(0) };
        static DUMMY_NODE_COUNT: Cell<usize> = const { Cell::new(0) };
    }

    #[composable]
    fn text_tab() {
        TEXT_NODE_COUNT.with(|c| c.set(c.get() + 1));
        let id = cranpose_test_node(TestTextNode::default);
        with_node_mut(id, |node: &mut TestTextNode| {
            node.text = "Text Node Tab".to_string();
        })
        .expect("update text node");
    }

    #[composable]
    fn dummy_tab() {
        DUMMY_NODE_COUNT.with(|c| c.set(c.get() + 1));
        cranpose_test_node(|| TestDummyNode);
    }

    let mut render = {
        move || match active_tab.value() {
            0 => text_tab(),
            _ => dummy_tab(),
        }
    };

    let key = location_key(file!(), line!(), column!());

    // Start with text node
    TEXT_NODE_COUNT.with(|c| c.set(0));
    DUMMY_NODE_COUNT.with(|c| c.set(0));
    composition
        .render(key, &mut render)
        .expect("initial render with text node");
    assert_eq!(TEXT_NODE_COUNT.with(|c| c.get()), 1);
    assert_eq!(DUMMY_NODE_COUNT.with(|c| c.get()), 0);

    // Switch to dummy node
    active_tab.set_value(1);
    composition
        .render(key, &mut render)
        .expect("switch to dummy node");
    assert_eq!(DUMMY_NODE_COUNT.with(|c| c.get()), 1);

    // Switch back to text node - should work without corruption
    active_tab.set_value(0);
    TEXT_NODE_COUNT.with(|c| c.set(0));
    composition
        .render(key, &mut render)
        .expect("switch back to text node");
    assert!(
        TEXT_NODE_COUNT.with(|c| c.get()) > 0,
        "Should successfully render text node after switching from different node type"
    );
}

#[test]
fn tab_switching_with_dynamic_lists() {
    // Test tab switching with tabs containing dynamic lists of varying sizes
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());
    let list_size = MutableState::with_runtime(3usize, runtime.clone());

    thread_local! {
        static LIST_TAB_CALLED: Cell<bool> = const { Cell::new(false) };
        static LAST_ITEM_COUNT: Cell<usize> = const { Cell::new(0) };
    }

    #[composable]
    fn list_tab(count: usize) {
        LIST_TAB_CALLED.with(|c| c.set(true));
        LAST_ITEM_COUNT.with(|c| c.set(count));
        for i in 0..count {
            let id = cranpose_test_node(TestTextNode::default);
            with_node_mut(id, |node: &mut TestTextNode| {
                node.text = format!("Item {}", i);
            })
            .expect("update list item");
        }
    }

    let mut render = {
        move || {
            LIST_TAB_CALLED.with(|c| c.set(false));
            if active_tab.value() == 0 {
                list_tab(list_size.value());
            }
        }
    };

    let key = location_key(file!(), line!(), column!());

    // Initial render with 3 items
    composition
        .render(key, &mut render)
        .expect("initial render");
    assert!(
        LIST_TAB_CALLED.with(|c| c.get()),
        "list_tab should be called on initial render"
    );
    assert_eq!(LAST_ITEM_COUNT.with(|c| c.get()), 3);

    // Switch away
    active_tab.set_value(1);
    composition.render(key, &mut render).expect("switch away");
    assert!(
        !LIST_TAB_CALLED.with(|c| c.get()),
        "list_tab should NOT be called when tab is inactive"
    );

    // Change list size and switch back
    list_size.set_value(5);
    active_tab.set_value(0);
    composition
        .render(key, &mut render)
        .expect("switch back with larger list");
    assert!(
        LIST_TAB_CALLED.with(|c| c.get()),
        "list_tab should be called after switch back"
    );
    assert_eq!(
        LAST_ITEM_COUNT.with(|c| c.get()),
        5,
        "Should render 5 items after switching back"
    );

    // Switch away and come back with smaller list
    active_tab.set_value(1);
    composition
        .render(key, &mut render)
        .expect("switch away again");
    assert!(
        !LIST_TAB_CALLED.with(|c| c.get()),
        "list_tab should NOT be called when inactive"
    );

    list_size.set_value(2);
    active_tab.set_value(0);
    composition
        .render(key, &mut render)
        .expect("switch back with smaller list");
    assert!(
        LIST_TAB_CALLED.with(|c| c.get()),
        "list_tab should be called after second switch back"
    );
    assert_eq!(
        LAST_ITEM_COUNT.with(|c| c.get()),
        2,
        "Should render only 2 items after shrinking list"
    );
}

#[test]
fn tab_switching_with_nested_components() {
    // Test tab switching with nested component hierarchies
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());
    let outer_counter = MutableState::with_runtime(0i32, runtime.clone());
    let inner_counter = MutableState::with_runtime(0i32, runtime.clone());

    thread_local! {
        static OUTER_RENDERS: Cell<usize> = const { Cell::new(0) };
        static INNER_RENDERS: Cell<usize> = const { Cell::new(0) };
    }

    #[composable]
    fn inner_component(counter: MutableState<i32>) {
        INNER_RENDERS.with(|c| c.set(c.get() + 1));
        let count = counter.value();
        let id = cranpose_test_node(TestTextNode::default);
        with_node_mut(id, |node: &mut TestTextNode| {
            node.text = format!("Inner: {}", count);
        })
        .expect("update inner node");
    }

    #[composable]
    fn outer_component(outer_counter: MutableState<i32>, inner_counter: MutableState<i32>) {
        println!("outer_component called");
        OUTER_RENDERS.with(|c| c.set(c.get() + 1));
        let count = outer_counter.value();
        let id = cranpose_test_node(TestTextNode::default);
        with_node_mut(id, |node: &mut TestTextNode| {
            node.text = format!("Outer: {}", count);
        })
        .expect("update outer node");

        inner_component(inner_counter);
    }

    #[composable]
    fn empty_tab() {
        // Empty tab content
    }

    let mut render = {
        move || {
            let tab = active_tab.value();
            println!("Render closure called, active_tab={}", tab);
            if tab == 0 {
                println!("About to call outer_component");
                outer_component(outer_counter, inner_counter);
            } else {
                empty_tab();
            }
        }
    };

    let key = location_key(file!(), line!(), column!());

    // Initial render
    OUTER_RENDERS.with(|c| c.set(0));
    INNER_RENDERS.with(|c| c.set(0));
    println!("=== INITIAL RENDER ===");
    composition
        .render(key, &mut render)
        .expect("initial render");
    assert_eq!(OUTER_RENDERS.with(|c| c.get()), 1);
    assert_eq!(INNER_RENDERS.with(|c| c.get()), 1);

    // Switch away
    active_tab.set_value(1);
    composition.render(key, &mut render).expect("switch away");

    // Switch back
    active_tab.set_value(0);
    OUTER_RENDERS.with(|c| c.set(0));
    INNER_RENDERS.with(|c| c.set(0));
    println!("Before switch back render");
    match composition.render(key, &mut render) {
        Ok(_) => println!("Render succeeded"),
        Err(e) => println!("Render failed: {:?}", e),
    }
    let outer_renders = OUTER_RENDERS.with(|c| c.get());
    let inner_renders = INNER_RENDERS.with(|c| c.get());
    println!(
        "After switch back: outer={}, inner={}",
        outer_renders, inner_renders
    );
    assert!(
        outer_renders > 0,
        "Outer should render, got {}",
        outer_renders
    );
    assert!(
        inner_renders > 0,
        "Inner should render, got {}",
        inner_renders
    );

    // Verify both scopes work after tab switch
    OUTER_RENDERS.with(|c| c.set(0));
    INNER_RENDERS.with(|c| c.set(0));
    outer_counter.set_value(5);
    let _ = composition
        .process_invalid_scopes()
        .expect("recompose outer");
    let outer_count = OUTER_RENDERS.with(|c| c.get());
    let inner_count = INNER_RENDERS.with(|c| c.get());
    assert!(
        outer_count > 0,
        "Outer scope should work after tab switch, got {}",
        outer_count
    );
    // NOTE: Inner should NOT rerender when only outer_counter changes because inner's inputs haven't changed
    // This is the expected Compose behavior - smart recomposition skips children when inputs are unchanged
    assert_eq!(
        inner_count, 0,
        "Inner should not rerender when only outer_counter changes (inner_counter is unchanged)"
    );

    // Verify inner scope independently
    OUTER_RENDERS.with(|c| c.set(0));
    INNER_RENDERS.with(|c| c.set(0));
    inner_counter.set_value(10);
    let _ = composition
        .process_invalid_scopes()
        .expect("recompose inner");
    assert_eq!(
        OUTER_RENDERS.with(|c| c.get()),
        0,
        "Outer should not rerender for inner-only change"
    );
    assert!(
        INNER_RENDERS.with(|c| c.get()) > 0,
        "Inner scope should work independently after tab switch"
    );
}

#[test]
fn restored_keyed_branch_forces_deep_regular_param_recompose() {
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());
    let version_state = MutableState::with_runtime(0i32, runtime);
    let key = location_key(file!(), line!(), column!());

    thread_local! {
        static STORED_ACTION: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
        static OBSERVED_VALUE: Cell<i32> = const { Cell::new(-1) };
    }

    #[composable]
    fn callback_sink(on_action: impl FnMut() + 'static) {
        STORED_ACTION.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(on_action));
        });
    }

    #[composable]
    fn action_binding(version_state: MutableState<i32>) {
        let version = version_state.value();
        crate::with_key(&version, || {
            let token = useState(|| version);
            callback_sink(move || {
                OBSERVED_VALUE.with(|cell| cell.set(token.get()));
            });
        });
    }

    #[composable]
    fn stage_three(version_state: MutableState<i32>) {
        action_binding(version_state);
    }

    #[composable]
    fn stage_two(version_state: MutableState<i32>) {
        stage_three(version_state);
    }

    #[composable]
    fn stage_one(version_state: MutableState<i32>) {
        stage_two(version_state);
    }

    #[composable]
    fn inactive_tab() {}

    let mut render = move || {
        let active = active_tab.value();
        crate::with_key(&active, || {
            if active == 0 {
                stage_one(version_state);
            } else {
                inactive_tab();
            }
        });
    };

    composition
        .render(key, &mut render)
        .expect("initial render");
    let initial_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        STORED_ACTION.with(|slot| {
            slot.borrow_mut()
                .as_mut()
                .expect("initial callback should be installed")();
        });
    }));
    assert!(
        initial_result.is_ok(),
        "initial deep callback should be callable"
    );
    assert_eq!(
        OBSERVED_VALUE.with(|cell| cell.get()),
        0,
        "initial callback should observe the initial keyed token",
    );

    active_tab.set_value(1);
    composition.render(key, &mut render).expect("switch away");

    version_state.set_value(1);

    active_tab.set_value(0);
    composition.render(key, &mut render).expect("switch back");

    let restored_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        STORED_ACTION.with(|slot| {
            slot.borrow_mut()
                .as_mut()
                .expect("restored callback should be installed")();
        });
    }));
    assert!(
        restored_result.is_ok(),
        "restored keyed branch should refresh deep callbacks instead of keeping stale ones",
    );
    assert_eq!(
        OBSERVED_VALUE.with(|cell| cell.get()),
        1,
        "deep callback should observe the restored keyed token after tab switch",
    );
}

#[test]
fn restored_branch_refreshes_deep_callback_after_recompose_via_content_lambda() {
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());
    let hover_state = MutableState::with_runtime(0i32, runtime);
    let key = location_key(file!(), line!(), column!());

    thread_local! {
        static STORED_ACTION: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
        static OBSERVED_VALUE: Cell<i32> = const { Cell::new(-1) };
    }

    #[composable]
    fn callback_sink(on_action: impl FnMut() + 'static) {
        STORED_ACTION.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(on_action));
        });
    }

    #[composable]
    fn content_host(content: impl FnMut() + 'static) {
        content();
    }

    #[composable]
    fn counter_tab(hover_state: MutableState<i32>) {
        let token = useState(|| 41i32);
        let _hover = hover_state.value();

        content_host(move || {
            callback_sink(move || {
                OBSERVED_VALUE.with(|cell| cell.set(token.get()));
            });
        });
    }

    #[composable]
    fn inactive_tab() {}

    let mut render = move || {
        let active = active_tab.value();
        crate::with_key(&active, || {
            if active == 0 {
                counter_tab(hover_state);
            } else {
                inactive_tab();
            }
        });
    };

    fn drain_invalid_scopes(
        composition: &mut Composition<MemoryApplier>,
        key: Key,
        render: &mut dyn FnMut(),
    ) {
        let _ = composition
            .process_invalid_scopes()
            .expect("recompose invalid scopes");
        if composition.take_root_render_request() {
            composition
                .render(key, render)
                .expect("render requested root");
        }
    }

    composition
        .render(key, &mut render)
        .expect("initial render");

    let initial_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        STORED_ACTION.with(|slot| {
            slot.borrow_mut()
                .as_mut()
                .expect("initial callback should be installed")();
        });
    }));
    assert!(
        initial_result.is_ok(),
        "initial callback should be callable"
    );
    assert_eq!(
        OBSERVED_VALUE.with(|cell| cell.get()),
        41,
        "initial callback should observe the initial tab-local state",
    );

    active_tab.set_value(1);
    drain_invalid_scopes(&mut composition, key, &mut render);

    active_tab.set_value(0);
    drain_invalid_scopes(&mut composition, key, &mut render);

    hover_state.set_value(1);
    drain_invalid_scopes(&mut composition, key, &mut render);

    let restored_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        STORED_ACTION.with(|slot| {
            slot.borrow_mut()
                .as_mut()
                .expect("restored callback should stay installed")();
        });
    }));
    assert!(
        restored_result.is_ok(),
        "restored callback should refresh through ancestor recomposition instead of reading a removed state cell",
    );
    assert_eq!(
        OBSERVED_VALUE.with(|cell| cell.get()),
        41,
        "restored callback should still observe the live tab-local state after ancestor recomposition",
    );
}

#[test]
fn restored_keyed_first_sibling_keeps_later_live_subtree_and_descendant_scope_identity() {
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime);
    let key = location_key(file!(), line!(), column!());

    thread_local! {
        static ROOT_ID: Cell<Option<NodeId>> = const { Cell::new(None) };
        static CONTENT_ROOT_ID: Cell<Option<NodeId>> = const { Cell::new(None) };
        static COUNTER_LABEL_ID: Cell<Option<NodeId>> = const { Cell::new(None) };
        static PARITY_LABEL_ID: Cell<Option<NodeId>> = const { Cell::new(None) };
        static COUNTER_STATE: RefCell<Option<MutableState<i32>>> = const { RefCell::new(None) };
        static WAVE_STATE: RefCell<Option<MutableState<f32>>> = const { RefCell::new(None) };
        static ANIMATED_SCOPE_ID: Cell<Option<ScopeId>> = const { Cell::new(None) };
        static ANIMATED_SCOPE_RECOMPOSITIONS: Cell<usize> = const { Cell::new(0) };
    }

    fn set_text(id: NodeId, text: String) {
        with_node_mut(id, |node: &mut TestTextNode| {
            node.text = text;
        })
        .expect("update test text node");
    }

    fn drain_invalid_scopes(
        composition: &mut Composition<MemoryApplier>,
        key: Key,
        render: &mut dyn FnMut(),
    ) {
        loop {
            let recomposed = composition
                .process_invalid_scopes()
                .expect("process invalid scopes");
            let requested_root = composition.take_root_render_request();
            if requested_root {
                composition
                    .render(key, &mut *render)
                    .expect("render requested root");
            }
            if !recomposed && !requested_root {
                break;
            }
        }
    }

    #[composable]
    fn tab_button(label: &'static str) {
        let id = cranpose_test_node(TestTextNode::default);
        set_text(id, label.to_string());
    }

    #[composable]
    fn parity_branch(counter: MutableState<i32>) {
        crate::debug_label_current_scope("parity_branch");
        let is_even = counter.get() % 2 == 0;
        crate::with_key(&is_even, || {
            let id = cranpose_test_node(TestTextNode::default);
            PARITY_LABEL_ID.with(|slot| slot.set(Some(id)));
            set_text(
                id,
                if is_even {
                    "if counter % 2 == 0".to_string()
                } else {
                    "if counter % 2 != 0".to_string()
                },
            );
        });
    }

    #[composable]
    fn animated_descendant(wave: MutableState<f32>) {
        crate::debug_label_current_scope("animated_descendant");
        ANIMATED_SCOPE_RECOMPOSITIONS.with(|count| count.set(count.get() + 1));
        with_current_composer(|composer| {
            let scope = composer
                .current_recranpose_scope()
                .expect("animated scope available");
            ANIMATED_SCOPE_ID.with(|slot| slot.set(Some(scope.id())));
        });
        let id = cranpose_test_node(TestTextNode::default);
        set_text(id, format!("Wave: {:.2}", wave.get()));
    }

    #[composable]
    fn animated_subtree(wave: MutableState<f32>) {
        with_current_composer(|composer| {
            let wrapper = composer.emit_node(RecordingNode::default);
            composer.push_parent(wrapper);
            animated_descendant(wave);
            composer.pop_parent();
        });
    }

    #[composable]
    fn main_content(counter: MutableState<i32>, wave: MutableState<f32>) {
        crate::debug_label_current_scope("main_content");
        with_current_composer(|composer| {
            let content_root = composer.emit_node(RecordingNode::default);
            CONTENT_ROOT_ID.with(|slot| slot.set(Some(content_root)));
            composer.push_parent(content_root);

            let counter_label = composer.emit_node(TestTextNode::default);
            COUNTER_LABEL_ID.with(|slot| slot.set(Some(counter_label)));
            set_text(counter_label, format!("Counter: {}", counter.get()));

            animated_subtree(wave);

            composer.pop_parent();
        });
    }

    #[composable]
    fn counter_tab() {
        crate::debug_label_current_scope("counter_tab");
        let counter = useState(|| 0i32);
        let wave = useState(|| 0.0f32);
        COUNTER_STATE.with(|slot| {
            *slot.borrow_mut() = Some(counter);
        });
        WAVE_STATE.with(|slot| {
            *slot.borrow_mut() = Some(wave);
        });

        parity_branch(counter);
        main_content(counter, wave);
    }

    #[composable]
    fn inactive_tab() {
        COUNTER_STATE.with(|slot| {
            slot.borrow_mut().take();
        });
        WAVE_STATE.with(|slot| {
            slot.borrow_mut().take();
        });
        let id = cranpose_test_node(TestTextNode::default);
        set_text(id, "Inactive Tab".to_string());
    }

    let mut render = move || {
        with_current_composer(|composer| {
            let root = composer.emit_node(RecordingNode::default);
            ROOT_ID.with(|slot| slot.set(Some(root)));
            composer.push_parent(root);

            tab_button("Counter App");
            tab_button("Other Tab");

            let active = active_tab.value();
            crate::with_key(&active, || {
                if active == 0 {
                    counter_tab();
                } else {
                    inactive_tab();
                }
            });

            composer.pop_parent();
        });
    };

    composition
        .render(key, &mut render)
        .expect("initial render");

    let initial_root_id = ROOT_ID
        .with(|slot| slot.get())
        .expect("root id after initial render");
    let initial_content_root = CONTENT_ROOT_ID
        .with(|slot| slot.get())
        .expect("content root after initial render");
    let initial_counter_label = COUNTER_LABEL_ID
        .with(|slot| slot.get())
        .expect("counter label after initial render");
    let initial_animated_scope = ANIMATED_SCOPE_ID
        .with(|slot| slot.get())
        .expect("animated scope after initial render");

    let initial_root_children = composition
        .applier_mut()
        .with_node::<RecordingNode, _>(initial_root_id, |node| node.children.clone())
        .expect("root recording node after initial render");
    assert!(
        initial_root_children.contains(&initial_content_root),
        "initial root should own the live content subtree",
    );
    assert_eq!(
        composition
            .applier_mut()
            .with_node::<TestTextNode, _>(initial_counter_label, |node| node.text.clone())
            .expect("counter label text after initial render"),
        "Counter: 0",
    );

    active_tab.set_value(1);
    drain_invalid_scopes(&mut composition, key, &mut render);

    active_tab.set_value(0);
    drain_invalid_scopes(&mut composition, key, &mut render);

    let restored_counter = COUNTER_STATE
        .with(|slot| slot.borrow().as_ref().copied())
        .expect("counter state after restore");
    let restored_wave = WAVE_STATE
        .with(|slot| slot.borrow().as_ref().copied())
        .expect("wave state after restore");
    let restored_content_root = CONTENT_ROOT_ID
        .with(|slot| slot.get())
        .expect("content root after restore");
    let restored_counter_label = COUNTER_LABEL_ID
        .with(|slot| slot.get())
        .expect("counter label after restore");
    let restored_animated_scope = ANIMATED_SCOPE_ID
        .with(|slot| slot.get())
        .expect("animated scope after restore");

    assert_eq!(
        restored_counter.get(),
        0,
        "restored counter should restart at zero"
    );
    assert_ne!(
        restored_animated_scope, initial_animated_scope,
        "roundtripping the whole tab should create a fresh descendant scope",
    );
    assert_eq!(
        composition
            .applier_mut()
            .with_node::<TestTextNode, _>(restored_counter_label, |node| node.text.clone())
            .expect("counter label text after restore"),
        "Counter: 0",
    );

    restored_wave.set_value(0.5);
    restored_counter.set_value(1);
    drain_invalid_scopes(&mut composition, key, &mut render);

    let root_children = composition
        .applier_mut()
        .with_node::<RecordingNode, _>(initial_root_id, |node| node.children.clone())
        .expect("root recording node after counter update");
    let slot_groups_after_counter = composition.debug_dump_slot_table_groups();
    let slots_after_counter = composition.debug_dump_all_slots();
    let counter_label_text = composition
        .applier_mut()
        .with_node::<TestTextNode, _>(restored_counter_label, |node| node.text.clone())
        .unwrap_or_else(|err| {
            panic!(
                "counter label text after counter update: {err:?}; root_children={root_children:?}; groups={slot_groups_after_counter:?}; slots={slots_after_counter:?}"
            )
        });
    let parity_text = composition
        .applier_mut()
        .with_node::<TestTextNode, _>(
            PARITY_LABEL_ID
                .with(|slot| slot.get())
                .expect("parity label after counter update"),
            |node| node.text.clone(),
        )
        .expect("parity label text after counter update");
    let animated_scope_after_counter = ANIMATED_SCOPE_ID
        .with(|slot| slot.get())
        .expect("animated scope after counter update");
    let animated_recompositions = ANIMATED_SCOPE_RECOMPOSITIONS.with(Cell::get);

    assert!(
        root_children.contains(&restored_content_root),
        "counter flip after restore must keep the later live subtree attached to the root: root_children={root_children:?}; groups={slot_groups_after_counter:?}; slots={slots_after_counter:?}",
    );
    assert_eq!(
        counter_label_text, "Counter: 1",
        "counter flip after restore must update the later live subtree instead of dropping it",
    );
    assert_eq!(parity_text, "if counter % 2 != 0");
    assert_eq!(
        animated_scope_after_counter, restored_animated_scope,
        "descendant animated scope identity must stay stable across sibling recomposition after restore",
    );
    assert!(
        animated_recompositions >= 2,
        "wave invalidation should recompose the descendant animated scope at least once after restore",
    );
}

#[test]
fn debug_nested_component_slot_table_state() {
    // Debug test to understand slot table state during nested component recomposition
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());
    let outer_counter = MutableState::with_runtime(0i32, runtime.clone());
    let inner_counter = MutableState::with_runtime(0i32, runtime.clone());

    thread_local! {
        static OUTER_RENDERS: Cell<usize> = const { Cell::new(0) };
        static INNER_RENDERS: Cell<usize> = const { Cell::new(0) };
    }

    #[composable]
    fn inner_component(counter: MutableState<i32>) {
        INNER_RENDERS.with(|c| c.set(c.get() + 1));
        let _count = counter.value();
    }

    #[composable]
    fn outer_component(outer_counter: MutableState<i32>, inner_counter: MutableState<i32>) {
        OUTER_RENDERS.with(|c| c.set(c.get() + 1));
        let _count = outer_counter.value();
        inner_component(inner_counter);
    }

    let mut render = {
        move || {
            if active_tab.value() == 0 {
                outer_component(outer_counter, inner_counter);
            }
        }
    };

    let key = location_key(file!(), line!(), column!());

    // Initial render
    composition
        .render(key, &mut render)
        .expect("initial render");
    println!("After initial render:");
    for (idx, kind) in composition.debug_dump_all_slots() {
        println!("  [{}] {}", idx, kind);
    }

    // Switch away
    active_tab.set_value(1);
    composition.render(key, &mut render).expect("switch away");
    println!("\nAfter switch away:");
    for (idx, kind) in composition.debug_dump_all_slots() {
        println!("  [{}] {}", idx, kind);
    }

    // Switch back
    active_tab.set_value(0);
    composition.render(key, &mut render).expect("switch back");
    println!("\nAfter switch back:");
    for (idx, kind) in composition.debug_dump_all_slots() {
        println!("  [{}] {}", idx, kind);
    }

    // Trigger recomposition
    OUTER_RENDERS.with(|c| c.set(0));
    INNER_RENDERS.with(|c| c.set(0));
    outer_counter.set_value(5);

    println!("\nBefore process_invalid_scopes:");
    let groups4 = composition.debug_dump_slot_table_groups();
    for (idx, key, scope, len) in &groups4 {
        println!(
            "  Group at {}: key={:?}, scope={:?}, len={}",
            idx, key, scope, len
        );
    }

    let _ = composition
        .process_invalid_scopes()
        .expect("recompose outer");

    println!("\nAfter process_invalid_scopes:");
    let groups5 = composition.debug_dump_slot_table_groups();
    for (idx, key, scope, len) in &groups5 {
        println!(
            "  Group at {}: key={:?}, scope={:?}, len={}",
            idx, key, scope, len
        );
    }

    println!(
        "\nOuter renders: {}, Inner renders: {}",
        OUTER_RENDERS.with(|c| c.get()),
        INNER_RENDERS.with(|c| c.get())
    );
}

#[test]
fn composable_macro_injects_scope_label() {
    let _guard = reset_snapshot_runtime();
    crate::set_debug_scope_tracking_override_for_tests(Some(true));
    crate::DEBUG_SCOPE_LABELS.with(|labels| labels.borrow_mut().clear());

    thread_local! {
        static AUTO_SCOPE_ID: Cell<Option<usize>> = const { Cell::new(None) };
    }

    #[composable]
    fn auto_labeled_leaf() {
        with_current_composer(|composer| {
            let scope = composer
                .current_recranpose_scope()
                .expect("auto-labeled scope should exist");
            AUTO_SCOPE_ID.with(|slot| slot.set(Some(scope.id())));
        });
    }

    let mut composition = test_composition();
    composition
        .render(location_key(file!(), line!(), column!()), &mut || {
            auto_labeled_leaf()
        })
        .expect("render auto-labeled composable");

    let scope_id = AUTO_SCOPE_ID
        .with(|slot| slot.get())
        .expect("scope id should be captured");
    assert_eq!(
        crate::debug_scope_label(scope_id),
        Some("auto_labeled_leaf")
    );

    crate::set_debug_scope_tracking_override_for_tests(None);
}

#[test]
fn tab_switching_memory_slot_reuse() {
    // Verify that slots are properly reused and not leaked during tab switches
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());

    #[composable]
    fn tab_with_markers(tab_id: i32) {
        // Create multiple nodes to occupy slots
        for i in 0..5 {
            let id = cranpose_test_node(TestTextNode::default);
            with_node_mut(id, |node: &mut TestTextNode| {
                node.text = format!("Tab {} Item {}", tab_id, i);
            })
            .expect("update node");
        }
    }

    let mut render = {
        move || {
            tab_with_markers(active_tab.value());
        }
    };

    let key = location_key(file!(), line!(), column!());

    // Initial render
    composition
        .render(key, &mut render)
        .expect("initial render");

    // Perform many tab switches to check for slot leaks
    for cycle in 0..10 {
        for tab in 0..4 {
            active_tab.set_value(tab);
            composition
                .render(key, &mut render)
                .unwrap_or_else(|_| panic!("render cycle {} tab {}", cycle, tab));
        }
    }

    // If we made it here without panicking or running out of memory,
    // slots are being properly reused
    // Verify final render still works correctly
    active_tab.set_value(0);
    composition
        .render(key, &mut render)
        .expect("final render after many switches");
}

#[test]
fn tab_switching_with_state_during_switch() {
    // Test updating state while switching tabs - edge case for race conditions
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());
    let shared_counter = MutableState::with_runtime(0i32, runtime.clone());

    thread_local! {
        static TAB0_RENDERS: Cell<usize> = const { Cell::new(0) };
        static TAB1_RENDERS: Cell<usize> = const { Cell::new(0) };
    }

    #[composable]
    fn tab_content(tab_id: i32, counter: MutableState<i32>) {
        if tab_id == 0 {
            TAB0_RENDERS.with(|c| c.set(c.get() + 1));
        } else {
            TAB1_RENDERS.with(|c| c.set(c.get() + 1));
        }
        let count = counter.value();
        let id = cranpose_test_node(TestTextNode::default);
        with_node_mut(id, |node: &mut TestTextNode| {
            node.text = format!("Tab {} Count {}", tab_id, count);
        })
        .expect("update node");
    }

    let mut render = {
        move || {
            tab_content(active_tab.value(), shared_counter);
        }
    };

    let key = location_key(file!(), line!(), column!());

    // Initial render
    TAB0_RENDERS.with(|c| c.set(0));
    composition
        .render(key, &mut render)
        .expect("initial render");
    assert_eq!(TAB0_RENDERS.with(|c| c.get()), 1);

    // Update state AND switch tabs simultaneously
    shared_counter.set_value(42);
    active_tab.set_value(1);
    TAB1_RENDERS.with(|c| c.set(0));
    composition
        .render(key, &mut render)
        .expect("switch with state update");

    // Tab 1 should render with updated counter
    assert!(
        TAB1_RENDERS.with(|c| c.get()) > 0,
        "Tab 1 should render with updated state"
    );

    // Switch back - scope should still work
    active_tab.set_value(0);
    TAB0_RENDERS.with(|c| c.set(0));
    composition.render(key, &mut render).expect("switch back");

    shared_counter.set_value(100);
    TAB0_RENDERS.with(|c| c.set(0));
    let _ = composition
        .process_invalid_scopes()
        .expect("recompose after state update");
    assert!(
        TAB0_RENDERS.with(|c| c.get()) > 0,
        "Tab 0 scope should still work after concurrent state/tab change"
    );
}

#[test]
fn tab_switching_with_empty_tab() {
    // Test switching to/from an empty tab (no nodes created)
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());
    let counter = MutableState::with_runtime(0i32, runtime.clone());

    thread_local! {
        static CONTENT_RENDERS: Cell<usize> = const { Cell::new(0) };
    }

    #[composable]
    fn content_tab(counter: MutableState<i32>) {
        CONTENT_RENDERS.with(|c| c.set(c.get() + 1));
        let count = counter.value();
        let id = cranpose_test_node(TestTextNode::default);
        with_node_mut(id, |node: &mut TestTextNode| {
            node.text = format!("Content: {}", count);
        })
        .expect("update node");
    }

    #[composable]
    fn empty_tab() {
        // Intentionally empty - no nodes created
    }

    let mut render = {
        move || match active_tab.value() {
            0 => content_tab(counter),
            _ => empty_tab(),
        }
    };

    let key = location_key(file!(), line!(), column!());

    // Start with content
    CONTENT_RENDERS.with(|c| c.set(0));
    composition
        .render(key, &mut render)
        .expect("initial render");
    assert_eq!(CONTENT_RENDERS.with(|c| c.get()), 1);

    // Switch to empty tab
    active_tab.set_value(1);
    composition
        .render(key, &mut render)
        .expect("switch to empty");

    // Switch back to content
    active_tab.set_value(0);
    CONTENT_RENDERS.with(|c| c.set(0));
    composition
        .render(key, &mut render)
        .expect("switch back from empty");
    assert!(
        CONTENT_RENDERS.with(|c| c.get()) > 0,
        "Should render content after empty tab"
    );

    // Verify scope still works
    CONTENT_RENDERS.with(|c| c.set(0));
    counter.set_value(42);
    let _ = composition
        .process_invalid_scopes()
        .expect("recompose after empty");
    assert!(
        CONTENT_RENDERS.with(|c| c.get()) > 0,
        "Scope should work after switching from empty tab"
    );
}

#[test]
fn tab_switching_preserves_node_order() {
    // Verify that node order is preserved correctly across tab switches
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let active_tab = MutableState::with_runtime(0i32, runtime.clone());

    thread_local! {
        static RENDER_ORDER: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    #[composable]
    fn ordered_tab(tab_id: i32) {
        RENDER_ORDER.with(|o| o.borrow_mut().clear());
        let prefix = if tab_id == 0 { "A" } else { "B" };
        for i in 0..3 {
            RENDER_ORDER.with(|o| o.borrow_mut().push(format!("{}_{}", prefix, i)));
            let id = cranpose_test_node(TestTextNode::default);
            with_node_mut(id, |node: &mut TestTextNode| {
                node.text = format!("{} Item {}", prefix, i);
            })
            .expect("update node");
        }
    }

    let mut render = {
        move || {
            let tab = active_tab.value();
            if tab <= 1 {
                ordered_tab(tab);
            }
        }
    };

    let key = location_key(file!(), line!(), column!());

    // Initial render with Tab A
    composition
        .render(key, &mut render)
        .expect("initial render");
    let order_a = RENDER_ORDER.with(|o| o.borrow().clone());
    assert_eq!(order_a, vec!["A_0", "A_1", "A_2"]);

    // Switch to Tab B
    active_tab.set_value(1);
    composition.render(key, &mut render).expect("switch to B");
    let order_b = RENDER_ORDER.with(|o| o.borrow().clone());
    assert_eq!(order_b, vec!["B_0", "B_1", "B_2"]);

    // Switch back to Tab A - order should be preserved
    active_tab.set_value(0);
    composition
        .render(key, &mut render)
        .expect("switch back to A");
    let order_a_again = RENDER_ORDER.with(|o| o.borrow().clone());
    assert_eq!(
        order_a_again,
        vec!["A_0", "A_1", "A_2"],
        "Order should be preserved after tab switch"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// SlotTable Integration Tests
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn composition_works_with_slot_table() {
    exercise_basic_slot_table_composition();
}

fn composable_params_are_preserved_during_recomposition() {
    thread_local! {
        static PARAM_VALUES: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    }

    #[composable]
    fn param_leaf(value: usize) {
        PARAM_VALUES.with(|values| values.borrow_mut().push(value));
    }

    #[composable]
    fn param_root(value_state: MutableState<usize>) {
        param_leaf(value_state.get());
    }

    let runtime = Runtime::new(Arc::new(TestScheduler));
    let value_state = MutableState::with_runtime(1usize, runtime.handle());
    let mut composition = test_composition_with_runtime(runtime);
    let key = location_key(file!(), line!(), column!());

    PARAM_VALUES.with(|values| values.borrow_mut().clear());
    composition
        .render(key, &mut || param_root(value_state))
        .expect("initial render with selected backend");

    value_state.set(2);
    while composition
        .process_invalid_scopes()
        .expect("recompose composable parameter leaf")
    {}

    PARAM_VALUES.with(|values| {
        assert_eq!(
            values.borrow().as_slice(),
            &[1, 2],
            "slot table lost composable parameter state during recomposition",
        );
    });
}

#[test]
fn slot_table_preserves_composable_params_during_recomposition() {
    composable_params_are_preserved_during_recomposition();
}

fn exercise_basic_slot_table_composition() {
    let key = 12345u64;
    let applier = test_applier();
    let runtime = Runtime::new(Arc::new(TestScheduler));
    let mut composition = Composition::with_runtime(applier, runtime.clone());

    // Track recompositions
    let recranpose_count = Rc::new(Cell::new(0));
    let recranpose_count_clone = Rc::clone(&recranpose_count);

    // Test basic composition with groups and remembered values
    composition
        .render(key, || {
            with_current_composer(|composer| {
                composer.with_group(1, |composer| {
                    recranpose_count_clone.set(recranpose_count_clone.get() + 1);

                    // Test remember
                    let value = composer.remember(|| 123);
                    value.with(|v| assert_eq!(*v, 123));

                    // Test nested group
                    composer.with_group(2, |composer| {
                        let nested = composer.remember(|| "hello".to_string());
                        nested.with(|n| assert_eq!(n, "hello"));
                    });
                });
            });
        })
        .expect("first render");

    assert_eq!(recranpose_count.get(), 1, "Should have composed once");

    // Test recomposition preserves remembered values
    composition
        .render(key, || {
            with_current_composer(|composer| {
                composer.with_group(1, |composer| {
                    recranpose_count_clone.set(recranpose_count_clone.get() + 1);

                    // Remembered value should be preserved
                    let value = composer.remember(|| 456); // Different init, but should return 123
                    value.with(|v| assert_eq!(*v, 123, "Remembered value should be preserved"));

                    composer.with_group(2, |composer| {
                        let nested = composer.remember(|| "world".to_string());
                        nested.with(|n| {
                            assert_eq!(n, "hello", "Nested remembered value should be preserved")
                        });
                    });
                });
            });
        })
        .expect("second render");

    assert_eq!(recranpose_count.get(), 2, "Should have composed twice");
}
