use super::*;

#[test]
fn remember_state_roundtrip() {
    let mut composition = test_composition();
    let mut text_seen = String::new();

    for _ in 0..2 {
        composition
            .render(location_key(file!(), line!(), column!()), || {
                with_current_composer(|composer| {
                    composer.with_group(location_key(file!(), line!(), column!()), |composer| {
                        let count = composer.use_state(|| 0);
                        let node_id = composer.emit_node(TestTextNode::default);
                        composer
                            .with_node_mut(node_id, |node: &mut TestTextNode| {
                                node.text = format!("{}", count.get());
                            })
                            .expect("update text node");
                        text_seen = count.get().to_string();
                    });
                });
            })
            .expect("render succeeds");
    }

    assert_eq!(text_seen, "0");
}

#[test]
fn state_update_schedules_render() {
    let mut composition = test_composition();
    let mut stored = None;
    composition
        .render(location_key(file!(), line!(), column!()), || {
            let state = cranpose_core::useState(|| 10);
            let _ = state.value();
            stored = Some(state);
        })
        .expect("render succeeds");
    let state = stored.expect("state stored");
    assert!(!composition.should_render());
    state.set(11);
    assert!(composition.should_render());
}

#[test]
fn returned_composable_state_change_recomposes_parent_consumer() {
    thread_local! {
        static OBSERVED_RETURNS: RefCell<Vec<i32>> = const { RefCell::new(Vec::new()) };
    }

    #[composable]
    fn child_value(state: MutableState<i32>) -> i32 {
        state.value()
    }

    #[composable]
    fn parent(state: MutableState<i32>) {
        let value = child_value(state);
        OBSERVED_RETURNS.with(|values| values.borrow_mut().push(value));
    }

    OBSERVED_RETURNS.with(|values| values.borrow_mut().clear());

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let state = MutableState::with_runtime(0, runtime.clone());

    composition
        .render(location_key(file!(), line!(), column!()), || parent(state))
        .expect("initial composition");

    state.set_value(1);
    while composition
        .process_invalid_scopes()
        .expect("returned value recomposition")
    {}

    OBSERVED_RETURNS.with(|values| {
        assert_eq!(values.borrow().as_slice(), &[0, 1]);
    });
}

#[test]
fn recranpose_does_not_use_stale_indices_when_prior_scope_changes_length() {
    thread_local! {
        static STABLE_RECOMPOSE_A: Cell<usize> = const { Cell::new(0) };
        static STABLE_RECOMPOSE_B: Cell<usize> = const { Cell::new(0) };
    }

    #[composable]
    fn logging_group_a(state_a: MutableState<i32>, toggle_a: MutableState<bool>) {
        STABLE_RECOMPOSE_A.with(|count| count.set(count.get() + 1));
        let _ = state_a.value();
        let expand = toggle_a.value();
        if expand {
            let _ = cranpose_core::remember(|| ());
            let _ = cranpose_core::remember(|| ());
            cranpose_core::with_key(&"nested", || {});
        } else {
            let _ = cranpose_core::remember(|| ());
        }
    }

    #[composable]
    fn logging_group_b(state_b: MutableState<i32>) {
        STABLE_RECOMPOSE_B.with(|count| count.set(count.get() + 1));
        let _ = state_b.value();
    }

    #[composable]
    fn logging_root(
        state_a: MutableState<i32>,
        state_b: MutableState<i32>,
        toggle_a: MutableState<bool>,
    ) {
        cranpose_core::with_key(&"root", || {
            cranpose_core::with_key(&"A", || logging_group_a(state_a, toggle_a));
            cranpose_core::with_key(&"B", || logging_group_b(state_b));
        });
    }

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let state_a = MutableState::with_runtime(0i32, runtime.clone());
    let state_b = MutableState::with_runtime(0i32, runtime.clone());
    let toggle_a = MutableState::with_runtime(false, runtime.clone());

    let mut render = { move || logging_root(state_a, state_b, toggle_a) };

    composition
        .render(location_key(file!(), line!(), column!()), &mut render)
        .expect("initial render");

    STABLE_RECOMPOSE_A.with(|count| assert_eq!(count.get(), 1));
    STABLE_RECOMPOSE_B.with(|count| assert_eq!(count.get(), 1));

    STABLE_RECOMPOSE_A.with(|count| count.set(0));
    STABLE_RECOMPOSE_B.with(|count| count.set(0));

    state_b.set_value(1);
    toggle_a.set_value(true);
    state_a.set_value(1);

    let recomposed = composition
        .process_invalid_scopes()
        .expect("recomposition succeeds");
    assert!(recomposed, "expected at least one scope to recompose");

    STABLE_RECOMPOSE_A.with(|count| assert!(count.get() >= 1));
    STABLE_RECOMPOSE_B.with(|count| assert!(count.get() >= 1));
}

#[test]
fn recranpose_handles_removed_scopes_gracefully() {
    thread_local! {
        static REMOVED_SCOPE_LOG: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    }

    fn render_optional_scope(
        composer: &Composer,
        state_a: &MutableState<i32>,
        toggle_group: &MutableState<bool>,
    ) {
        if toggle_group.value() {
            let state_clone = *state_a;
            composer.with_group(21, |composer| {
                let state_capture = state_clone;
                composer.set_recranpose_callback({
                    move |composer| {
                        let _ = state_capture.value();
                        composer.register_side_effect(|| {
                            REMOVED_SCOPE_LOG.with(|log| log.borrow_mut().push("scope"));
                        });
                    }
                });
                let _ = state_capture.value();
                composer.register_side_effect(|| {
                    REMOVED_SCOPE_LOG.with(|log| log.borrow_mut().push("scope"));
                });
            });
        }
    }

    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let state_a = MutableState::with_runtime(0i32, runtime.clone());
    let toggle_group = MutableState::with_runtime(true, runtime.clone());

    let mut render = {
        move || {
            with_current_composer(|composer| {
                render_optional_scope(composer, &state_a, &toggle_group);
            });
        }
    };

    composition
        .render(location_key(file!(), line!(), column!()), &mut render)
        .expect("initial render");

    REMOVED_SCOPE_LOG.with(|log| log.borrow_mut().clear());

    state_a.set_value(1);
    toggle_group.set_value(false);

    composition
        .render(location_key(file!(), line!(), column!()), &mut render)
        .expect("render without scope");

    let recomposed = composition
        .process_invalid_scopes()
        .expect("process invalid scopes succeeds");
    assert!(!recomposed);

    REMOVED_SCOPE_LOG.with(|log| {
        assert!(log.borrow().is_empty());
    });
}

#[test]
fn side_effect_runs_after_composition() {
    let mut composition = test_composition();
    SIDE_EFFECT_LOG.with(|log| log.borrow_mut().clear());
    SIDE_EFFECT_STATE.with(|slot| *slot.borrow_mut() = None);
    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, || {
            side_effect_component();
        })
        .expect("render succeeds");
    SIDE_EFFECT_LOG.with(|log| {
        assert_eq!(&*log.borrow(), &["compose", "effect"]);
    });
    SIDE_EFFECT_STATE.with(|slot| {
        if let Some(state) = slot.borrow().as_ref() {
            state.set_value(1);
        }
    });
    assert!(composition.should_render());
    let _ = composition
        .process_invalid_scopes()
        .expect("process invalid scopes succeeds");
    SIDE_EFFECT_LOG.with(|log| {
        assert_eq!(&*log.borrow(), &["compose", "effect", "compose", "effect"]);
    });
}

#[test]
fn disposable_effect_reacts_to_key_changes() {
    let mut composition = test_composition();
    DISPOSABLE_EFFECT_LOG.with(|log| log.borrow_mut().clear());
    DISPOSABLE_STATE.with(|slot| *slot.borrow_mut() = None);
    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, || {
            disposable_effect_host();
        })
        .expect("render succeeds");
    DISPOSABLE_EFFECT_LOG.with(|log| {
        assert_eq!(&*log.borrow(), &["start"]);
    });
    composition
        .render(key, || {
            disposable_effect_host();
        })
        .expect("render succeeds");
    DISPOSABLE_EFFECT_LOG.with(|log| {
        assert_eq!(&*log.borrow(), &["start"]);
    });
    DISPOSABLE_STATE.with(|slot| {
        if let Some(state) = slot.borrow().as_ref() {
            state.set_value(1);
        }
    });
    composition
        .render(key, || {
            disposable_effect_host();
        })
        .expect("render succeeds");
    DISPOSABLE_EFFECT_LOG.with(|log| {
        assert_eq!(&*log.borrow(), &["start", "dispose", "start"]);
    });
}

#[test]
fn state_invalidation_skips_parent_scope() {
    PARENT_RECOMPOSITIONS.with(|calls| calls.set(0));
    CHILD_RECOMPOSITIONS.with(|calls| calls.set(0));
    CAPTURED_PARENT_STATE.with(|slot| *slot.borrow_mut() = None);

    let mut composition = test_composition();
    let root_key = location_key(file!(), line!(), column!());

    composition
        .render(root_key, || {
            parent_passes_state();
        })
        .expect("initial render succeeds");

    PARENT_RECOMPOSITIONS.with(|calls| assert_eq!(calls.get(), 1));
    CHILD_RECOMPOSITIONS.with(|calls| assert_eq!(calls.get(), 1));

    let state = CAPTURED_PARENT_STATE
        .with(|slot| *slot.borrow())
        .expect("captured state");

    PARENT_RECOMPOSITIONS.with(|calls| calls.set(0));
    CHILD_RECOMPOSITIONS.with(|calls| calls.set(0));

    state.set(1);
    assert!(composition.should_render());

    let _ = composition
        .process_invalid_scopes()
        .expect("process invalid scopes succeeds");

    PARENT_RECOMPOSITIONS.with(|calls| assert_eq!(calls.get(), 0));
    CHILD_RECOMPOSITIONS.with(|calls| assert!(calls.get() > 0));
    assert!(!composition.should_render());
}

fn apply_child_diff(
    slots: &mut SlotTable,
    applier: &mut MemoryApplier,
    runtime: &Runtime,
    parent_id: NodeId,
    previous: Vec<NodeId>,
    new_children: Vec<NodeId>,
) -> Vec<Operation> {
    let handle = runtime.handle();
    let (composer, slots_host, applier_host) =
        setup_composer(slots, applier, handle, Some(parent_id));
    composer.push_parent(parent_id);
    {
        let mut stack = composer.parent_stack();
        let frame = stack.last_mut().expect("parent frame available");
        frame.previous = previous.into();
        frame.new_children = new_children.into();
    }
    composer.pop_parent();
    let commands = composer.take_commands();
    drop(composer);
    teardown_composer(slots, applier, slots_host, applier_host);
    commands.apply(applier).expect("apply diff command");
    applier
        .with_node(parent_id, |node: &mut RecordingNode| {
            node.operations.clone()
        })
        .expect("read parent operations")
}

#[test]
fn reorder_keyed_children_emits_moves() {
    let mut slots = SlotTable::default();
    let mut applier = test_applier();
    let runtime = Runtime::new(Arc::new(TestScheduler));
    let parent_id = applier.create(Box::new(RecordingNode::default()));

    let child_a = applier.create(Box::new(TrackingChild {
        label: "a".to_string(),
        mount_count: 1,
        parent: Some(parent_id),
    }));
    let child_b = applier.create(Box::new(TrackingChild {
        label: "b".to_string(),
        mount_count: 1,
        parent: Some(parent_id),
    }));
    let child_c = applier.create(Box::new(TrackingChild {
        label: "c".to_string(),
        mount_count: 1,
        parent: Some(parent_id),
    }));

    applier
        .with_node(parent_id, |node: &mut RecordingNode| {
            node.children = vec![child_a, child_b, child_c];
            node.operations.clear();
        })
        .expect("seed parent state");
    let initial_len = applier.len();

    let operations = apply_child_diff(
        &mut slots,
        &mut applier,
        &runtime,
        parent_id,
        vec![child_a, child_b, child_c],
        vec![child_c, child_b, child_a],
    );

    assert_eq!(
        operations,
        vec![
            Operation::Move { from: 2, to: 0 },
            Operation::Move { from: 2, to: 1 },
        ]
    );

    let final_children = applier
        .with_node(parent_id, |node: &mut RecordingNode| node.children.clone())
        .expect("read reordered children");
    assert_eq!(final_children, vec![child_c, child_b, child_a]);
    let final_len = applier.len();
    assert_eq!(initial_len, final_len);

    for (expected_label, child_id) in [("a", child_a), ("b", child_b), ("c", child_c)] {
        applier
            .with_node(child_id, |child: &mut TrackingChild| {
                assert_eq!(child.label, expected_label.to_string());
                assert_eq!(child.mount_count, 1);
            })
            .expect("read tracking child state");
    }
}

#[test]
fn insert_and_remove_emit_expected_ops() {
    let mut slots = SlotTable::default();
    let mut applier = test_applier();
    let runtime = Runtime::new(Arc::new(TestScheduler));
    let parent_id = applier.create(Box::new(RecordingNode::default()));

    let child_a = applier.create(Box::new(TrackingChild {
        label: "a".to_string(),
        mount_count: 1,
        parent: Some(parent_id),
    }));
    let child_b = applier.create(Box::new(TrackingChild {
        label: "b".to_string(),
        mount_count: 1,
        parent: Some(parent_id),
    }));

    applier
        .with_node(parent_id, |node: &mut RecordingNode| {
            node.children = vec![child_a, child_b];
            node.operations.clear();
        })
        .expect("seed parent state");
    let initial_len = applier.len();

    let child_c = applier.create(Box::new(TrackingChild {
        label: "c".to_string(),
        mount_count: 1,
        parent: Some(parent_id),
    }));
    assert_eq!(applier.len(), initial_len + 1);

    let insert_ops = apply_child_diff(
        &mut slots,
        &mut applier,
        &runtime,
        parent_id,
        vec![child_a, child_b],
        vec![child_a, child_b, child_c],
    );

    assert_eq!(insert_ops, vec![Operation::Insert(child_c)]);
    let after_insert_children = applier
        .with_node(parent_id, |node: &mut RecordingNode| node.children.clone())
        .expect("read children after insert");
    assert_eq!(after_insert_children, vec![child_a, child_b, child_c]);

    applier
        .with_node(parent_id, |node: &mut RecordingNode| {
            node.operations.clear()
        })
        .expect("clear operations");

    let remove_ops = apply_child_diff(
        &mut slots,
        &mut applier,
        &runtime,
        parent_id,
        vec![child_a, child_b, child_c],
        vec![child_a, child_c],
    );

    assert_eq!(remove_ops, vec![Operation::Remove(child_b)]);
    let after_remove_children = applier
        .with_node(parent_id, |node: &mut RecordingNode| node.children.clone())
        .expect("read children after remove");
    assert_eq!(after_remove_children, vec![child_a, child_c]);
    assert_eq!(applier.len(), initial_len);
}

#[test]
fn removing_subtree_unmounts_descendants() {
    let mut applier = test_applier();
    let parent_id = applier.create(Box::new(RecordingNode::default()));
    let child_unmounts = Rc::new(Cell::new(0));
    let grandchild_unmounts = Rc::new(Cell::new(0));
    let child_id = applier.create(Box::new(UnmountTrackingNode::new(Rc::clone(
        &child_unmounts,
    ))));
    let grandchild_id = applier.create(Box::new(UnmountTrackingNode::new(Rc::clone(
        &grandchild_unmounts,
    ))));

    insert_child_with_reparenting(&mut applier, parent_id, child_id);
    insert_child_with_reparenting(&mut applier, child_id, grandchild_id);

    remove_child_and_cleanup_now(&mut applier, parent_id, child_id).expect("remove subtree");

    assert_eq!(child_unmounts.get(), 1);
    assert_eq!(grandchild_unmounts.get(), 1);
    assert!(matches!(
        applier.get_mut(child_id),
        Err(NodeError::Missing { id }) if id == child_id
    ));
    assert!(matches!(
        applier.get_mut(grandchild_id),
        Err(NodeError::Missing { id }) if id == grandchild_id
    ));
}

#[test]
fn memory_applier_compact_packs_live_nodes_without_invalidating_stable_ids() {
    let mut applier = test_applier();
    let root_id = applier.create(Box::new(TestDummyNode));
    assert_eq!(root_id, 0);

    let removed_ids: Vec<_> = (0..9)
        .map(|_| applier.create(Box::new(TestDummyNode)))
        .collect();
    assert_eq!(removed_ids, (1..=9).collect::<Vec<_>>());

    for &id in &removed_ids {
        applier.remove(id).expect("remove freed node");
    }

    let reused_first = applier.create(Box::new(TestDummyNode));
    let reused_second = applier.create(Box::new(TestDummyNode));
    applier.compact();

    assert_eq!(applier.len(), 3);
    assert_eq!(applier.capacity(), 3);
    assert_eq!(applier.tombstone_count(), 0);
    assert!(applier.get_mut(root_id).is_ok());
    assert!(applier.get_mut(reused_first).is_ok());
    assert!(applier.get_mut(reused_second).is_ok());
}

#[test]
fn memory_applier_compact_skips_dense_tables_until_tombstones_dominate() {
    let mut applier = test_applier();
    let ids: Vec<_> = (0..2_048)
        .map(|_| applier.create(Box::new(TestDummyNode)))
        .collect();

    for &id in ids.iter().take(512) {
        applier.remove(id).expect("remove dense-table node");
    }

    let dense_capacity = applier.capacity();
    let dense_live = applier.len();
    let dense_tombstones = applier.tombstone_count();
    assert!(
        dense_capacity > 1_024 && dense_tombstones < dense_live,
        "expected a dense table before compact: capacity={dense_capacity} live={dense_live} tombstones={dense_tombstones}",
    );

    applier.compact();

    assert_eq!(
        applier.capacity(),
        dense_capacity,
        "compact should keep dense tables warm for reuse",
    );
    assert_eq!(
        applier.tombstone_count(),
        dense_tombstones,
        "dense-table compact should not rewrite storage",
    );

    for &id in ids.iter().skip(512).take(1_200) {
        applier.remove(id).expect("remove sparse-table node");
    }

    let sparse_live = applier.len();
    let sparse_tombstones = applier.tombstone_count();
    assert!(
        sparse_tombstones > sparse_live,
        "expected sparse teardown before compact: live={sparse_live} tombstones={sparse_tombstones}",
    );

    applier.compact();

    assert_eq!(
        applier.capacity(),
        sparse_live,
        "compact should pack sparse tables after teardown",
    );
    assert_eq!(applier.tombstone_count(), 0);
}

#[test]
fn memory_applier_compact_rehouses_live_nodes_after_large_majority_drop() {
    let mut applier = test_applier();
    let root_id = applier.create(Box::new(RehousingDummyNode { marker: 7 }));
    let removed_ids: Vec<_> = (0..2_048)
        .map(|_| applier.create(Box::new(TestDummyNode)))
        .collect();

    let before_ptr = {
        let node = applier
            .get_mut(root_id)
            .expect("root should remain accessible before compact")
            .as_any_mut()
            .downcast_mut::<RehousingDummyNode>()
            .expect("root node type");
        node as *mut RehousingDummyNode
    };

    for id in removed_ids {
        applier.remove(id).expect("remove sparse-table node");
    }

    applier.compact();

    let after_ptr = {
        let node = applier
            .get_mut(root_id)
            .expect("root should remain accessible after compact")
            .as_any_mut()
            .downcast_mut::<RehousingDummyNode>()
            .expect("root node type");
        assert_eq!(node.marker, 7);
        node as *mut RehousingDummyNode
    };

    assert_ne!(
        before_ptr, after_ptr,
        "large-majority compact should move surviving live nodes onto fresh boxes"
    );
}

#[test]
fn child_diff_handles_interleaved_remove_move_and_insert() {
    let mut slots = SlotTable::default();
    let mut applier = test_applier();
    let runtime = Runtime::new(Arc::new(TestScheduler));
    let parent_id = applier.create(Box::new(RecordingNode::default()));

    let child_a = applier.create(Box::new(TrackingChild {
        label: "a".to_string(),
        mount_count: 1,
        parent: Some(parent_id),
    }));
    let child_b = applier.create(Box::new(TrackingChild {
        label: "b".to_string(),
        mount_count: 1,
        parent: Some(parent_id),
    }));
    let child_c = applier.create(Box::new(TrackingChild {
        label: "c".to_string(),
        mount_count: 1,
        parent: Some(parent_id),
    }));
    let child_d = applier.create(Box::new(TrackingChild {
        label: "d".to_string(),
        mount_count: 1,
        parent: Some(parent_id),
    }));

    applier
        .with_node(parent_id, |node: &mut RecordingNode| {
            node.children = vec![child_a, child_b, child_c, child_d];
            node.operations.clear();
        })
        .expect("seed parent state");
    let initial_len = applier.len();

    let child_e = applier.create(Box::new(TrackingChild {
        label: "e".to_string(),
        mount_count: 1,
        parent: Some(parent_id),
    }));
    assert_eq!(applier.len(), initial_len + 1);

    let operations = apply_child_diff(
        &mut slots,
        &mut applier,
        &runtime,
        parent_id,
        vec![child_a, child_b, child_c, child_d],
        vec![child_d, child_a, child_e, child_c],
    );

    assert_eq!(
        operations,
        vec![
            Operation::Remove(child_b),
            Operation::Move { from: 2, to: 0 },
            Operation::Insert(child_e),
            Operation::Move { from: 3, to: 2 },
        ]
    );

    let final_children = applier
        .with_node(parent_id, |node: &mut RecordingNode| node.children.clone())
        .expect("read final children");
    assert_eq!(final_children, vec![child_d, child_a, child_e, child_c]);
    assert_eq!(applier.len(), initial_len);
}

#[test]
fn composable_skips_when_inputs_unchanged() {
    INVOCATIONS.with(|calls| calls.set(0));
    let mut composition = test_composition();
    let key = location_key(file!(), line!(), column!());

    composition
        .render(key, || {
            counted_text(1);
        })
        .expect("render succeeds");
    INVOCATIONS.with(|calls| assert_eq!(calls.get(), 1));

    composition
        .render(key, || {
            counted_text(1);
        })
        .expect("render succeeds");
    INVOCATIONS.with(|calls| assert_eq!(calls.get(), 1));

    composition
        .render(key, || {
            counted_text(2);
        })
        .expect("render succeeds");
    INVOCATIONS.with(|calls| assert_eq!(calls.get(), 2));
}

#[test]
fn unit_return_composable_skips_without_return_value_slot() {
    thread_local! {
        static UNIT_INVOCATIONS: Cell<usize> = const { Cell::new(0) };
    }

    #[composable]
    fn unit_leaf(value: i32) {
        let _ = value;
        UNIT_INVOCATIONS.with(|calls| calls.set(calls.get() + 1));
    }

    let mut composition = test_composition();
    let key = location_key(file!(), line!(), column!());

    composition
        .render(key, || unit_leaf(1))
        .expect("initial unit render");

    let all_slots = composition.debug_dump_slot_entries();
    let value_count = all_slots
        .iter()
        .filter(|entry| entry.kind == crate::SlotDebugEntryKind::Payload)
        .count();
    let scoped_group_count = all_slots
        .iter()
        .filter(|entry| {
            entry.kind == crate::SlotDebugEntryKind::Group && !entry.line.contains("scope=None")
        })
        .count();

    assert_eq!(
        value_count, 1,
        "unit-return composable should store 1 Value (parameter state)",
    );
    assert_eq!(
        scoped_group_count, 2,
        "unit-return composable should store 2 scoped groups (root + child scopes)",
    );
    UNIT_INVOCATIONS.with(|calls| assert_eq!(calls.get(), 1));

    composition
        .render(key, || unit_leaf(1))
        .expect("skip render succeeds");

    UNIT_INVOCATIONS.with(|calls| assert_eq!(calls.get(), 1));
}
