use super::*;

#[test]
fn slots_host_into_table_reports_live_host_references() {
    let slots_host = Rc::new(SlotsHost::new(SlotTable::new()));
    let _held = Rc::clone(&slots_host);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| slots_host.into_table()));

    assert!(
        result.is_ok(),
        "SlotsHost::into_table should report live references without panicking"
    );
    assert_eq!(
        result.unwrap().err(),
        Some(NodeError::SlotHostUnavailable {
            operation: "SlotsHost::into_table",
            reason: "other host references are alive",
        })
    );
}

#[test]
fn slots_host_reset_reports_active_pass() {
    let slots_host = Rc::new(SlotsHost::new(SlotTable::new()));
    slots_host.begin_pass(crate::slot::SlotPassMode::Compose);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| slots_host.reset()));

    slots_host.abandon_active_pass();
    assert!(
        result.is_ok(),
        "SlotsHost::reset should report an active pass without panicking"
    );
    assert_eq!(
        result.unwrap(),
        Err(NodeError::SlotHostUnavailable {
            operation: "SlotsHost::reset",
            reason: "slot pass is active",
        })
    );
}

#[test]
fn slots_host_begin_pass_while_active_is_noop() {
    let slots_host = Rc::new(SlotsHost::new(SlotTable::new()));
    slots_host.begin_pass(crate::slot::SlotPassMode::Compose);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        slots_host.begin_pass(crate::slot::SlotPassMode::Recompose);
    }));

    slots_host.abandon_active_pass();
    assert!(
        result.is_ok(),
        "SlotsHost::begin_pass should ignore an already active pass without panicking"
    );
}

#[test]
fn slots_host_into_table_does_not_transfer_runtime_state_through_slot_table() {
    let state = Rc::new(crate::composer::ComposerRuntimeState::default());
    let slots_host = Rc::new(SlotsHost::new(SlotTable::new()));
    state.bind_slots_host(&slots_host);

    let storage_key = slots_host.storage_key();
    assert!(
        state.host_for_storage_key(storage_key).is_some(),
        "bound host must be registered before transfer"
    );

    let table = slots_host.into_table().expect("slots host transfer");
    assert!(
        state.host_for_storage_key(storage_key).is_none(),
        "transfer must clear runtime ownership for the original host"
    );

    let rebound_host = SlotsHost::new(table);
    assert!(
        rebound_host.runtime_state().is_none(),
        "slot storage must not smuggle composer runtime ownership into a new host"
    );
}

#[test]
fn composer_new_with_shared_state_quarantines_mismatched_bound_host() {
    let owner_state = Rc::new(crate::composer::ComposerRuntimeState::default());
    let other_state = Rc::new(crate::composer::ComposerRuntimeState::default());
    let slots_host = Rc::new(SlotsHost::new(SlotTable::new()));
    let original_storage_key = slots_host.storage_key();
    let owner_applier: Rc<dyn ApplierHost> =
        Rc::new(ConcreteApplierHost::new(MemoryApplier::new()));
    owner_state.bind_applier_host(&owner_applier);
    owner_state.bind_slots_host(&slots_host);

    let (handle, _runtime) = runtime_handle();
    let applier = Rc::new(ConcreteApplierHost::new(MemoryApplier::new()));
    let observer = SnapshotStateObserver::new(|callback| callback());

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Composer::new_with_shared_state(
            Rc::clone(&other_state),
            Rc::clone(&slots_host),
            applier,
            handle,
            observer,
            None,
        )
    }));

    assert!(
        result.is_ok(),
        "Composer should quarantine a mismatched SlotsHost without panicking"
    );
    let composer = result.unwrap();
    assert!(!Rc::ptr_eq(&composer.core.slots, &slots_host));
    assert!(
        owner_state
            .host_for_storage_key(original_storage_key)
            .is_some()
    );
    assert!(
        other_state
            .host_for_storage_key(composer.core.slots.storage_key())
            .is_some()
    );
}

struct MismatchedSlotPassFixture {
    composer: Composer,
    mismatched_host: Rc<SlotsHost>,
    owner_state: Rc<crate::composer::ComposerRuntimeState>,
    other_state: Rc<crate::composer::ComposerRuntimeState>,
    original_storage_key: usize,
    _owner_applier: Rc<dyn ApplierHost>,
    _runtime: Runtime,
}

fn mismatched_slot_pass_fixture() -> MismatchedSlotPassFixture {
    let owner_state = Rc::new(crate::composer::ComposerRuntimeState::default());
    let other_state = Rc::new(crate::composer::ComposerRuntimeState::default());
    let mismatched_host = Rc::new(SlotsHost::new(SlotTable::new()));
    let original_storage_key = mismatched_host.storage_key();
    let owner_applier: Rc<dyn ApplierHost> =
        Rc::new(ConcreteApplierHost::new(MemoryApplier::new()));
    owner_state.bind_applier_host(&owner_applier);
    owner_state.bind_slots_host(&mismatched_host);

    let (handle, _runtime) = runtime_handle();
    let local_host = Rc::new(SlotsHost::new(SlotTable::new()));
    let applier: Rc<dyn ApplierHost> = Rc::new(ConcreteApplierHost::new(MemoryApplier::new()));
    let observer = SnapshotStateObserver::new(|callback| callback());
    let composer = Composer::new_with_shared_state(
        Rc::clone(&other_state),
        local_host,
        applier,
        handle,
        observer,
        None,
    );

    MismatchedSlotPassFixture {
        composer,
        mismatched_host,
        owner_state,
        other_state,
        original_storage_key,
        _owner_applier: owner_applier,
        _runtime,
    }
}

fn inspect_mismatched_slot_pass_host(
    composer: &Composer,
    other_state: &Rc<crate::composer::ComposerRuntimeState>,
    mismatched_host: &Rc<SlotsHost>,
) -> (usize, bool, bool, bool) {
    let active_host = composer.active_slots_host();
    let active_storage_key = active_host.storage_key();
    let rebound_to_other_state = active_host
        .runtime_state()
        .map(|state| Rc::ptr_eq(&state, other_state))
        .unwrap_or(false);
    let registered_during_pass = other_state
        .host_for_storage_key(active_storage_key)
        .is_some();
    (
        active_storage_key,
        Rc::ptr_eq(&active_host, mismatched_host),
        rebound_to_other_state,
        registered_during_pass,
    )
}

fn assert_mismatched_slot_pass_uses_replacement(
    fixture: &MismatchedSlotPassFixture,
    active_storage_key: usize,
    used_original_host: bool,
    rebound_to_other_state: bool,
    registered_during_pass: bool,
) {
    assert!(!used_original_host);
    assert!(rebound_to_other_state);
    assert!(registered_during_pass);
    assert_ne!(active_storage_key, fixture.original_storage_key);
    assert!(
        fixture
            .owner_state
            .host_for_storage_key(fixture.original_storage_key)
            .is_some()
    );
    assert!(!fixture.mismatched_host.has_active_pass());
    assert!(
        fixture
            .mismatched_host
            .runtime_state()
            .map(|state| Rc::ptr_eq(&state, &fixture.owner_state))
            .unwrap_or(false)
    );
}

#[test]
fn try_slot_host_pass_quarantines_mismatched_bound_host() {
    let fixture = mismatched_slot_pass_fixture();

    let (
        (active_storage_key, used_original_host, rebound_to_other_state, registered_during_pass),
        _,
    ) = fixture
        .composer
        .try_with_slot_host_pass(
            Rc::clone(&fixture.mismatched_host),
            crate::slot::SlotPassMode::Compose,
            |composer| {
                inspect_mismatched_slot_pass_host(
                    composer,
                    &fixture.other_state,
                    &fixture.mismatched_host,
                )
            },
        )
        .expect("replacement slot host pass should finalize");

    assert_mismatched_slot_pass_uses_replacement(
        &fixture,
        active_storage_key,
        used_original_host,
        rebound_to_other_state,
        registered_during_pass,
    );
}

#[test]
fn slot_host_pass_wrapper_quarantines_mismatched_bound_host() {
    let fixture = mismatched_slot_pass_fixture();

    let (
        (active_storage_key, used_original_host, rebound_to_other_state, registered_during_pass),
        _,
    ) = fixture.composer.with_slot_host_pass(
        Rc::clone(&fixture.mismatched_host),
        crate::slot::SlotPassMode::Compose,
        |composer| {
            inspect_mismatched_slot_pass_host(
                composer,
                &fixture.other_state,
                &fixture.mismatched_host,
            )
        },
    );

    assert_mismatched_slot_pass_uses_replacement(
        &fixture,
        active_storage_key,
        used_original_host,
        rebound_to_other_state,
        registered_during_pass,
    );
}

#[test]
fn composer_retention_uses_checked_detached_root_key() {
    let source = include_str!("../composer.rs");

    assert!(
        !source.contains("key: subtree.root_key()"),
        "composer retention must not read detached roots through the strict root_key accessor"
    );
}

#[test]
fn memory_applier_dump_tree_reports_stale_physical_mapping() {
    let mut applier = MemoryApplier::new();
    applier.stable_to_physical.insert(42, usize::MAX);

    let tree = applier.dump_tree(Some(42));

    assert!(tree.contains("[42] (missing physical node"));
}

#[test]
fn memory_applier_create_routes_generated_high_ids_to_sparse_storage() {
    struct HighIdNode;
    impl Node for HighIdNode {}

    let mut applier = MemoryApplier::new();
    applier.next_stable_id = MemoryApplier::HIGH_ID_THRESHOLD;

    let node_id = applier.create(Box::new(HighIdNode));

    assert_eq!(node_id, MemoryApplier::HIGH_ID_THRESHOLD);
    assert!(applier.high_id_nodes.contains_key(&node_id));
    assert_eq!(applier.node_generation(node_id), 0);
    assert_eq!(applier.debug_stats().high_id_nodes_len, 1);
    assert_eq!(applier.debug_stats().nodes_len, 0);
}

#[test]
fn slot_pass_finalization_error_returns_from_try_pass() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = test_applier();

    let node_id = {
        let (composer, slots_host, applier_host) =
            setup_composer(&mut slots, &mut applier, handle.clone(), None);
        let (node_id, _) = composer
            .try_with_slot_host_pass(
                Rc::clone(&slots_host),
                crate::slot::SlotPassMode::Compose,
                |composer| {
                    composer.with_group(location_key(file!(), line!(), column!()), |composer| {
                        composer.emit_node(TestDummyNode::default)
                    })
                },
            )
            .expect("initial slot pass should finalize");
        let commands = composer.take_commands();
        drop(composer);
        teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
        commands
            .apply(&mut applier)
            .expect("initial node mount should apply");
        node_id
    };

    applier
        .remove(node_id)
        .expect("test setup should remove the mounted node");

    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle, None);
    let result = composer.try_with_slot_host_pass(
        Rc::clone(&slots_host),
        crate::slot::SlotPassMode::Compose,
        |_| {},
    );

    assert!(matches!(result, Err(NodeError::Missing { id }) if id == node_id));
    drop(composer);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
}

#[test]
fn slot_pass_wrapper_returns_body_result_after_finalization_error() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = test_applier();

    let node_id = {
        let (composer, slots_host, applier_host) =
            setup_composer(&mut slots, &mut applier, handle.clone(), None);
        let (node_id, _) = composer
            .try_with_slot_host_pass(
                Rc::clone(&slots_host),
                crate::slot::SlotPassMode::Compose,
                |composer| {
                    composer.with_group(location_key(file!(), line!(), column!()), |composer| {
                        composer.emit_node(TestDummyNode::default)
                    })
                },
            )
            .expect("initial slot pass should finalize");
        let commands = composer.take_commands();
        drop(composer);
        teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
        commands
            .apply(&mut applier)
            .expect("initial node mount should apply");
        node_id
    };

    applier
        .remove(node_id)
        .expect("test setup should remove the mounted node");

    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle, None);
    let (result, outcome) = composer.with_slot_host_pass(
        Rc::clone(&slots_host),
        crate::slot::SlotPassMode::Compose,
        |_| 42,
    );

    assert_eq!(result, 42);
    assert!(!outcome.compacted);
    assert!(!outcome.compact_anchor_registry_storage);
    assert!(!outcome.compact_payload_storage);
    drop(composer);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
}

#[test]
fn emit_node_rejects_reuse_when_parent_did_not_own_child() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = test_applier();

    let parent_a = applier.create(Box::new(RecordingNode::default()));
    let parent_b = applier.create(Box::new(RecordingNode::default()));

    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle.clone(), Some(parent_a));

    let (child_id, _) = composer.with_slot_host_pass(
        Rc::clone(&slots_host),
        crate::slot::SlotPassMode::Compose,
        |composer| {
            composer.with_group(location_key(file!(), line!(), column!()), |composer| {
                let child_id = composer.emit_node(|| TestDummyNode);

                composer.core.last_node_reused.set(Some(false));
                composer.push_parent(parent_b);

                {
                    let stack = composer.parent_stack();
                    let frame = stack.last().expect("parent frame should exist");
                    assert!(
                        frame.previous.is_empty(),
                        "New parent should have empty previous children"
                    );
                }

                composer.pop_parent();
                child_id
            })
        },
    );
    drop(composer);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);

    assert!(child_id > 0, "Child should have been created");
}

#[test]
fn push_parent_uses_empty_previous_when_not_reused() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = test_applier();

    let parent_id = applier.create(Box::new(RecordingNode::default()));

    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle.clone(), Some(parent_id));

    composer.core.last_node_reused.set(Some(false));
    composer.push_parent(parent_id);

    {
        let stack = composer.parent_stack();
        let frame = stack.last().expect("parent frame should exist");
        assert!(
            frame.previous.is_empty(),
            "When parent was not reused, previous children should be empty"
        );
    }

    composer.pop_parent();
    drop(composer);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
}

#[test]
fn new_parent_attaches_children_immediately_without_sync_children() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = test_applier();

    let parent_id = applier.create(Box::new(RecordingNode::default()));

    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle.clone(), None);

    let (child_id, _) = composer.with_slot_host_pass(
        Rc::clone(&slots_host),
        crate::slot::SlotPassMode::Compose,
        |composer| {
            composer.with_group(location_key(file!(), line!(), column!()), |composer| {
                composer.core.last_node_reused.set(Some(false));
                composer.push_parent(parent_id);
                let child_id = composer.emit_node(RecordingNode::default);
                composer.pop_parent();
                child_id
            })
        },
    );

    let commands = composer.take_commands();
    assert_eq!(commands.attach_children.len(), 1);
    assert_eq!(commands.sync_children.len(), 0);
    assert!(commands.sync_child_ids.is_empty());
    assert_eq!(commands.attach_children[0].parent_id, parent_id);
    assert_eq!(commands.attach_children[0].child_id, child_id);

    drop(composer);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
}

#[test]
fn subcompose_in_compacts_applier_after_large_teardown() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = test_applier();
    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle.clone(), None);
    let subcompose_slots = Rc::new(SlotsHost::new(SlotTable::new()));
    const NODE_COUNT: usize = 17_000;

    composer
        .subcompose_in(&subcompose_slots, None, |composer| {
            composer.with_group(location_key(file!(), line!(), column!()), |composer| {
                for _ in 0..NODE_COUNT {
                    let _ = composer.emit_node(|| TestDummyNode);
                }
            });
        })
        .expect("initial subcompose_in render");

    composer
        .subcompose_in(&subcompose_slots, None, |_composer| {})
        .expect("teardown subcompose_in render");

    drop(composer);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);

    assert_eq!(
        applier.tombstone_count(),
        0,
        "non-Composition slot passes must compact the applier after large teardowns",
    );
}

#[test]
fn reused_parent_with_existing_children_still_defers_to_sync_children() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = test_applier();

    let parent_id = applier.create(Box::new(RecordingNode::default()));
    let child_id = applier.create(Box::new(RecordingNode::default()));
    applier
        .with_node(parent_id, |node: &mut RecordingNode| {
            node.insert_child(child_id);
        })
        .expect("parent exists");
    applier
        .with_node(child_id, |node: &mut RecordingNode| {
            node.on_attached_to_parent(parent_id);
        })
        .expect("child exists");

    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle.clone(), None);

    composer.core.last_node_reused.set(Some(true));
    composer.push_parent(parent_id);
    {
        let mut stack = composer.parent_stack();
        let frame = stack.last_mut().expect("parent frame should exist");
        frame.new_children.push(child_id);
    }
    composer.pop_parent();

    let commands = composer.take_commands();
    assert_eq!(commands.attach_children.len(), 0);
    assert_eq!(commands.sync_children.len(), 1);
    assert_eq!(commands.sync_children[0].parent_id, parent_id);
    assert_eq!(commands.sync_child_ids.as_slice(), [child_id]);

    drop(composer);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
}

#[test]
fn non_reused_parent_with_existing_children_still_defers_to_sync_children() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = test_applier();

    let parent_id = applier.create(Box::new(RecordingNode::default()));
    let stale_child_id = applier.create(Box::new(RecordingNode::default()));
    let new_child_id = applier.create(Box::new(RecordingNode::default()));
    applier
        .with_node(parent_id, |node: &mut RecordingNode| {
            node.insert_child(stale_child_id);
        })
        .expect("parent exists");
    applier
        .with_node(stale_child_id, |node: &mut RecordingNode| {
            node.on_attached_to_parent(parent_id);
        })
        .expect("stale child exists");

    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle.clone(), None);

    composer.core.last_node_reused.set(Some(false));
    composer.push_parent(parent_id);
    {
        let stack = composer.parent_stack();
        let frame = stack.last().expect("parent frame should exist");
        assert_eq!(
            frame.previous.as_slice(),
            [stale_child_id],
            "non-reused parents must still diff existing live children",
        );
    }
    {
        let mut stack = composer.parent_stack();
        let frame = stack.last_mut().expect("parent frame should exist");
        frame.new_children.push(new_child_id);
    }
    composer.pop_parent();

    let commands = composer.take_commands();
    assert_eq!(commands.attach_children.len(), 0);
    assert_eq!(commands.sync_children.len(), 1);
    assert_eq!(commands.sync_children[0].parent_id, parent_id);
    assert_eq!(commands.sync_child_ids.as_slice(), [new_child_id]);

    drop(composer);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
}

#[test]
fn record_subcompose_child_deduplicates_large_deferred_sync_frames() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = test_applier();

    let parent_id = applier.create(Box::new(RecordingNode::default()));
    let stale_child_id = applier.create(Box::new(RecordingNode::default()));
    applier
        .with_node(parent_id, |node: &mut RecordingNode| {
            node.insert_child(stale_child_id);
        })
        .expect("parent exists");
    applier
        .with_node(stale_child_id, |node: &mut RecordingNode| {
            node.on_attached_to_parent(parent_id);
        })
        .expect("stale child exists");

    let child_ids: Vec<NodeId> = (0..24)
        .map(|_| applier.create(Box::new(RecordingNode::default())))
        .collect();

    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle.clone(), None);

    composer.core.last_node_reused.set(Some(true));
    composer.push_parent(parent_id);
    for &child_id in &child_ids {
        composer.record_subcompose_child(child_id);
    }
    for &child_id in child_ids.iter().rev() {
        composer.record_subcompose_child(child_id);
    }
    composer.pop_parent();

    let commands = composer.take_commands();
    assert_eq!(commands.attach_children.len(), 0);
    assert_eq!(commands.sync_children.len(), 1);
    assert_eq!(commands.sync_children[0].parent_id, parent_id);
    assert_eq!(commands.sync_child_ids.as_slice(), child_ids.as_slice());

    drop(composer);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
}

#[test]
fn sync_children_reorders_small_child_lists_without_regressing_behavior() {
    let mut applier = test_applier();

    let parent_id = applier.create(Box::new(RecordingNode::default()));
    let child_a = applier.create(Box::new(RecordingNode::default()));
    let child_b = applier.create(Box::new(RecordingNode::default()));
    let child_c = applier.create(Box::new(RecordingNode::default()));
    let child_d = applier.create(Box::new(RecordingNode::default()));

    for child_id in [child_a, child_b, child_c] {
        applier
            .with_node(parent_id, |node: &mut RecordingNode| {
                node.insert_child(child_id);
            })
            .expect("parent exists");
        applier
            .with_node(child_id, |node: &mut RecordingNode| {
                node.on_attached_to_parent(parent_id);
            })
            .expect("child exists");
    }

    Command::SyncChildren {
        parent_id,
        expected_children: SmallVec::<[NodeId; 4]>::from_slice(&[child_c, child_a, child_d]),
    }
    .apply(&mut applier)
    .expect("sync children");

    let final_children = applier
        .with_node(parent_id, |node: &mut RecordingNode| node.children.clone())
        .expect("parent exists");
    assert_eq!(final_children, vec![child_c, child_a, child_d]);

    assert!(
        matches!(applier.get_mut(child_b), Err(NodeError::Missing { .. })),
        "removed child should no longer exist in the applier",
    );
    let parent_of_d = applier
        .with_node(child_d, |node: &mut RecordingNode| node.parent())
        .expect("inserted child exists");
    assert_eq!(parent_of_d, Some(parent_id));
}

#[test]
fn queued_sync_children_preserves_child_reparented_later_in_same_apply() {
    let mut applier = test_applier();
    let old_parent = applier.create(Box::new(RecordingNode::default()));
    let new_parent = applier.create(Box::new(RecordingNode::default()));
    let unmounts = Rc::new(Cell::new(0));
    let child = applier.create(Box::new(UnmountTrackingNode::new(Rc::clone(&unmounts))));

    insert_child_with_reparenting(&mut applier, old_parent, child);

    let mut commands = CommandQueue::default();
    commands.push(Command::SyncChildren {
        parent_id: old_parent,
        expected_children: SmallVec::new(),
    });
    commands.push(Command::AttachChild {
        parent_id: new_parent,
        child_id: child,
        bubble: DirtyBubble::LAYOUT_AND_MEASURE,
    });

    commands
        .apply(&mut applier)
        .expect("same-frame reparent should succeed");

    assert_eq!(
        unmounts.get(),
        0,
        "child must stay live until the later attach command reparents it",
    );
    let child_parent = applier
        .with_node(child, |node: &mut UnmountTrackingNode| node.parent())
        .expect("child should remain live");
    assert_eq!(child_parent, Some(new_parent));
    let old_children = applier
        .with_node(old_parent, |node: &mut RecordingNode| node.children.clone())
        .expect("old parent exists");
    assert!(
        old_children.is_empty(),
        "old parent should no longer list the reparented child",
    );
    let new_children = applier
        .with_node(new_parent, |node: &mut RecordingNode| node.children.clone())
        .expect("new parent exists");
    assert_eq!(new_children, vec![child]);
}

#[test]
fn command_queue_missing_payload_returns_node_error() {
    let mut commands = CommandQueue::default();
    commands.push_tag(CommandTag::RemoveNode);

    let result = commands.apply(&mut test_applier());

    assert!(matches!(
        result,
        Err(NodeError::MalformedCommandPayload { tag }) if tag == "RemoveNode"
    ));
}

#[test]
fn command_queue_invalid_sync_children_range_returns_node_error() {
    let mut commands = CommandQueue::default();
    commands.sync_children.push(SyncChildrenCommand {
        parent_id: 1,
        child_start: 2,
        child_len: 4,
    });
    commands.push_tag(CommandTag::SyncChildren);

    let result = commands.apply(&mut test_applier());

    assert!(matches!(
        result,
        Err(NodeError::MalformedCommandPayload { tag }) if tag == "SyncChildren"
    ));
}

#[test]
fn large_mid_composition_flush_returns_command_error() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = test_applier();
    let (composer, _slots_host, _applier_host) =
        setup_composer(&mut slots, &mut applier, handle, None);

    {
        let mut commands = composer.commands_mut();
        for _ in 0..COMMAND_FLUSH_THRESHOLD {
            commands.push_tag(CommandTag::RemoveNode);
        }
    }

    let result = composer.flush_pending_commands_if_large();

    assert!(matches!(
        result,
        Err(NodeError::MalformedCommandPayload { tag }) if tag == "RemoveNode"
    ));
}

#[test]
fn emitted_node_replacement_removes_displaced_node() {
    const GROUP_KEY: Key = 58_001;

    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = test_applier();
    let unmounts = Rc::new(Cell::new(0));

    let first_id = {
        let (composer, slots_host, applier_host) =
            setup_composer(&mut slots, &mut applier, handle.clone(), None);
        let (id, _) = composer.with_slot_host_pass(
            Rc::clone(&slots_host),
            crate::slot::SlotPassMode::Compose,
            |composer| {
                composer.with_group(GROUP_KEY, |composer| {
                    composer.emit_node(|| UnmountTrackingNode::new(Rc::clone(&unmounts)))
                })
            },
        );
        let commands = composer.take_commands();
        drop(composer);
        teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
        commands
            .apply(&mut applier)
            .expect("initial node mount should apply");
        id
    };
    assert!(applier.get_mut(first_id).is_ok());

    let second_id = {
        let (composer, slots_host, applier_host) =
            setup_composer(&mut slots, &mut applier, handle.clone(), None);
        let (id, _) = composer.with_slot_host_pass(
            Rc::clone(&slots_host),
            crate::slot::SlotPassMode::Compose,
            |composer| {
                composer.with_group(GROUP_KEY, |composer| {
                    composer.emit_node(TestDummyNode::default)
                })
            },
        );
        let commands = composer.take_commands();
        drop(composer);
        teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
        commands
            .apply(&mut applier)
            .expect("replacement commands should apply");
        id
    };

    assert_ne!(first_id, second_id);
    assert!(
        matches!(applier.get_mut(first_id), Err(NodeError::Missing { .. })),
        "replaced node must be removed from the applier",
    );
    assert!(
        applier.get_mut(second_id).is_ok(),
        "replacement node must remain active",
    );
    assert_eq!(
        unmounts.get(),
        1,
        "replaced nodes must run the same unmount lifecycle as removed nodes",
    );
    assert_eq!(slots.debug_snapshot().active_node_count, 1);
}

#[test]
fn cold_recycled_nodes_are_not_reused_in_same_frame() {
    #[derive(Default)]
    struct RecyclableTestNode;

    impl Node for RecyclableTestNode {
        fn recycle_key(&self) -> Option<std::any::TypeId> {
            Some(std::any::TypeId::of::<Self>())
        }
    }

    let mut applier = test_applier();
    let stable_id = applier.create(Box::new(RecyclableTestNode));

    applier.remove(stable_id).expect("remove recyclable node");

    assert!(
        applier
            .take_recycled_node(std::any::TypeId::of::<RecyclableTestNode>())
            .is_none(),
        "cold shells removed in the current frame must not be reused immediately",
    );
    assert_eq!(applier.node_generation(stable_id), 1);
}

#[test]
fn fresh_recyclable_nodes_seed_same_frame_shell_reuse() {
    #[derive(Default)]
    struct SeedableNode;

    impl Node for SeedableNode {
        fn recycle_key(&self) -> Option<std::any::TypeId> {
            Some(std::any::TypeId::of::<Self>())
        }

        fn recycle_pool_limit(&self) -> Option<usize> {
            Some(2)
        }

        fn rehouse_for_recycle(&self) -> Option<Box<dyn Node>> {
            Some(Box::new(Self))
        }
    }

    let mut applier = test_applier();
    let key = std::any::TypeId::of::<SeedableNode>();
    let fresh = Box::new(SeedableNode);

    applier.record_fresh_recyclable_creation(key);
    if let Some(shell) = fresh.rehouse_for_recycle() {
        applier.seed_recycled_node_shell(key, fresh.recycle_pool_limit(), shell);
    }

    let recycled = applier
        .take_recycled_node(key)
        .expect("fresh miss should seed a reusable same-frame shell");
    assert_eq!(recycled.stable_id(), 0);
}

#[test]
fn recyclable_emit_discards_mismatched_recycled_shell() {
    #[derive(Default)]
    struct DesiredNode {
        initialized: bool,
        reset_calls: usize,
    }

    impl Node for DesiredNode {}

    struct WrongNode;

    impl Node for WrongNode {}

    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = test_applier();
    let key = std::any::TypeId::of::<DesiredNode>();
    applier
        .recycled_nodes
        .entry(key)
        .or_default()
        .push(RecycledNode::from_shell(0, Box::new(WrongNode), true));

    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle, None);
    let (node_id, _) = composer.with_slot_host_pass(
        Rc::clone(&slots_host),
        crate::slot::SlotPassMode::Compose,
        |composer| {
            composer.with_group(location_key(file!(), line!(), column!()), |composer| {
                composer.emit_recyclable_node(
                    || DesiredNode {
                        initialized: true,
                        reset_calls: 0,
                    },
                    |node| {
                        node.reset_calls += 1;
                    },
                )
            })
        },
    );
    let commands = composer.take_commands();
    drop(composer);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
    commands
        .apply(&mut applier)
        .expect("fresh fallback node should mount");

    let node = applier
        .get_mut(node_id)
        .expect("emitted fallback node should exist")
        .as_any_mut()
        .downcast_mut::<DesiredNode>()
        .expect("fallback node should use the requested node type");
    assert!(node.initialized);
    assert_eq!(node.reset_calls, 0);
    assert_eq!(applier.debug_recycled_node_count_for::<DesiredNode>(), 0);
}

#[test]
fn recyclable_emit_creates_fresh_id_when_recycled_stable_id_is_live() {
    #[derive(Default)]
    struct DesiredNode {
        from_recycled_shell: bool,
        reset_calls: usize,
    }

    impl Node for DesiredNode {}

    struct ExistingNode;

    impl Node for ExistingNode {}

    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = test_applier();
    let live_id = applier.create(Box::new(ExistingNode));
    let key = std::any::TypeId::of::<DesiredNode>();
    applier
        .recycled_nodes
        .entry(key)
        .or_default()
        .push(RecycledNode::from_shell(
            live_id,
            Box::new(DesiredNode {
                from_recycled_shell: true,
                reset_calls: 0,
            }),
            true,
        ));

    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle, None);
    let (node_id, _) = composer.with_slot_host_pass(
        Rc::clone(&slots_host),
        crate::slot::SlotPassMode::Compose,
        |composer| {
            composer.with_group(location_key(file!(), line!(), column!()), |composer| {
                composer.emit_recyclable_node(
                    || DesiredNode {
                        from_recycled_shell: false,
                        reset_calls: 0,
                    },
                    |node| {
                        node.reset_calls += 1;
                    },
                )
            })
        },
    );
    let commands = composer.take_commands();
    drop(composer);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
    commands
        .apply(&mut applier)
        .expect("collision fallback node should mount");

    assert_ne!(node_id, live_id);
    assert!(
        applier
            .get_mut(live_id)
            .expect("live node should remain active")
            .as_any_mut()
            .downcast_mut::<ExistingNode>()
            .is_some(),
        "stable-id collision must not replace the live node",
    );
    let node = applier
        .get_mut(node_id)
        .expect("emitted fallback node should exist")
        .as_any_mut()
        .downcast_mut::<DesiredNode>()
        .expect("fallback node should keep the requested node type");
    assert!(node.from_recycled_shell);
    assert_eq!(node.reset_calls, 1);
}

#[test]
fn recycled_nodes_reuse_stable_ids_without_growing_stable_id_arena() {
    #[derive(Default)]
    struct RecyclableTestNode;

    impl Node for RecyclableTestNode {
        fn recycle_key(&self) -> Option<std::any::TypeId> {
            Some(std::any::TypeId::of::<Self>())
        }
    }

    let mut applier = test_applier();
    let _keep_live = applier.create(Box::new(RecyclableTestNode));
    let stable_id = applier.create(Box::new(RecyclableTestNode));
    let next_stable_id_before_remove = applier.debug_stats().next_stable_id;

    applier.remove(stable_id).expect("remove recyclable node");
    applier.record_fresh_recyclable_creation(std::any::TypeId::of::<RecyclableTestNode>());
    applier.clear_recycled_nodes();

    let recycled = applier
        .take_recycled_node(std::any::TypeId::of::<RecyclableTestNode>())
        .expect("recycled node should be available after the frame boundary");
    assert_eq!(recycled.stable_id(), stable_id);
    assert_eq!(applier.node_generation(stable_id), 1);

    let (reused_id, node, warm_origin) = recycled.into_parts();
    applier
        .insert_with_id(reused_id, node)
        .expect("reinsert recycled stable id");
    applier.set_recycled_node_origin(reused_id, warm_origin);

    let stats = applier.debug_stats();
    assert_eq!(reused_id, stable_id);
    assert_eq!(stats.next_stable_id, next_stable_id_before_remove);
    assert_eq!(stats.stable_generations_len, next_stable_id_before_remove);
    assert_eq!(applier.node_generation(stable_id), 1);
}

#[test]
fn insert_with_id_preserves_stable_id_when_reusing_dense_physical_slot() {
    let mut applier = test_applier();
    let first = applier.create(Box::new(TestDummyNode));
    let reused_physical = applier.create(Box::new(TestDummyNode));
    let third = applier.create(Box::new(TestDummyNode));
    let inserted_stable_id = 10;

    applier
        .remove(reused_physical)
        .expect("remove node to open a dense physical slot");
    applier
        .insert_with_id(inserted_stable_id, Box::new(TestDummyNode))
        .expect("insert explicit stable id into freed physical slot");
    applier
        .remove(first)
        .expect("remove first node before compaction");
    applier
        .remove(third)
        .expect("remove third node before compaction");

    applier.compact();

    assert!(
        applier.get_mut(inserted_stable_id).is_ok(),
        "compaction must preserve the explicit stable id"
    );
    assert!(
        matches!(
            applier.get_mut(reused_physical),
            Err(NodeError::Missing { id }) if id == reused_physical
        ),
        "the removed stable id must not be resurrected by dense-slot metadata"
    );
}

#[test]
fn warm_recycled_nodes_can_be_reused_again_in_the_same_frame() {
    #[derive(Default)]
    struct RecyclableTestNode;

    impl Node for RecyclableTestNode {
        fn recycle_key(&self) -> Option<std::any::TypeId> {
            Some(std::any::TypeId::of::<Self>())
        }
    }

    let mut applier = test_applier();
    let _keep_live = applier.create(Box::new(RecyclableTestNode));
    let stable_id = applier.create(Box::new(RecyclableTestNode));

    applier.remove(stable_id).expect("remove recyclable node");
    applier.record_fresh_recyclable_creation(std::any::TypeId::of::<RecyclableTestNode>());
    applier.clear_recycled_nodes();

    let recycled = applier
        .take_recycled_node(std::any::TypeId::of::<RecyclableTestNode>())
        .expect("warm recyclable node");
    let (reused_id, node, warm_origin) = recycled.into_parts();
    applier
        .insert_with_id(reused_id, node)
        .expect("reinsert warm recyclable node");
    applier.set_recycled_node_origin(reused_id, warm_origin);

    applier
        .remove(reused_id)
        .expect("remove warm recyclable node again");

    let recycled_again = applier
        .take_recycled_node(std::any::TypeId::of::<RecyclableTestNode>())
        .expect("warm-origin shell should be reusable in the same frame");
    assert_eq!(recycled_again.stable_id(), stable_id);
}

#[test]
fn recycled_node_pool_limit_discards_oldest_shells() {
    #[derive(Default)]
    struct LimitedRecycleNode;

    impl Node for LimitedRecycleNode {
        fn recycle_key(&self) -> Option<std::any::TypeId> {
            Some(std::any::TypeId::of::<Self>())
        }

        fn recycle_pool_limit(&self) -> Option<usize> {
            Some(2)
        }
    }

    let mut applier = test_applier();
    let _keep_first = applier.create(Box::new(LimitedRecycleNode));
    let _keep_second = applier.create(Box::new(LimitedRecycleNode));
    let first = applier.create(Box::new(LimitedRecycleNode));
    let second = applier.create(Box::new(LimitedRecycleNode));
    let third = applier.create(Box::new(LimitedRecycleNode));

    applier.remove(first).expect("remove first recyclable node");
    applier
        .remove(second)
        .expect("remove second recyclable node");
    applier.remove(third).expect("remove third recyclable node");

    assert_eq!(
        applier.debug_recycled_node_count_for::<LimitedRecycleNode>(),
        2,
        "pending shells still respect the configured pool bound inside the frame",
    );

    applier.record_fresh_recyclable_creation(std::any::TypeId::of::<LimitedRecycleNode>());
    applier.record_fresh_recyclable_creation(std::any::TypeId::of::<LimitedRecycleNode>());
    applier.clear_recycled_nodes();

    assert_eq!(
        applier.debug_recycled_node_count_for::<LimitedRecycleNode>(),
        2,
        "warm recycle cohort should retain only the configured number of shells",
    );

    let most_recent = applier
        .take_recycled_node(std::any::TypeId::of::<LimitedRecycleNode>())
        .expect("most recent recycled node");
    let next_recent = applier
        .take_recycled_node(std::any::TypeId::of::<LimitedRecycleNode>())
        .expect("next recent recycled node");

    assert_eq!(most_recent.stable_id(), third);
    assert_eq!(next_recent.stable_id(), second);
}

#[test]
fn clear_recycled_nodes_trims_warm_pool_without_fresh_demand() {
    #[derive(Default)]
    struct LimitedRecycleNode;

    impl Node for LimitedRecycleNode {
        fn recycle_key(&self) -> Option<std::any::TypeId> {
            Some(std::any::TypeId::of::<Self>())
        }

        fn recycle_pool_limit(&self) -> Option<usize> {
            Some(4)
        }
    }

    let mut applier = test_applier();
    let _keep_live = applier.create(Box::new(LimitedRecycleNode));
    let removed: Vec<_> = (0..4)
        .map(|_| applier.create(Box::new(LimitedRecycleNode)))
        .collect();

    for id in removed {
        applier.remove(id).expect("remove recyclable node");
    }

    applier.clear_recycled_nodes();

    assert_eq!(
        applier.debug_recycled_node_count_for::<LimitedRecycleNode>(),
        0,
        "warm recycle cohort should be drained when the frame did not miss any recyclable shells",
    );
}

#[test]
fn warm_recycled_nodes_survive_idle_frame_after_recent_demand() {
    #[derive(Default)]
    struct LimitedRecycleNode;

    impl Node for LimitedRecycleNode {
        fn recycle_key(&self) -> Option<std::any::TypeId> {
            Some(std::any::TypeId::of::<Self>())
        }

        fn recycle_pool_limit(&self) -> Option<usize> {
            Some(4)
        }
    }

    let mut applier = test_applier();
    let _keep_live = applier.create(Box::new(LimitedRecycleNode));
    let recycled_id = applier.create(Box::new(LimitedRecycleNode));

    applier.remove(recycled_id).expect("remove recyclable node");
    applier.record_fresh_recyclable_creation(std::any::TypeId::of::<LimitedRecycleNode>());
    applier.clear_recycled_nodes();

    assert_eq!(
        applier.debug_recycled_node_count_for::<LimitedRecycleNode>(),
        1,
        "recent fresh demand should promote one warm shell",
    );

    applier.clear_recycled_nodes();

    assert_eq!(
        applier.debug_recycled_node_count_for::<LimitedRecycleNode>(),
        1,
        "recent demand floor should keep a compact warm shell across an idle frame",
    );
}

#[test]
fn clear_recycled_nodes_rebuilds_warm_pool_from_compact_prototype() {
    #[derive(Default)]
    struct PrototypeRecycleNode;

    impl Node for PrototypeRecycleNode {
        fn recycle_key(&self) -> Option<std::any::TypeId> {
            Some(std::any::TypeId::of::<Self>())
        }

        fn recycle_pool_limit(&self) -> Option<usize> {
            Some(8)
        }

        fn rehouse_for_recycle(&self) -> Option<Box<dyn Node>> {
            Some(Box::new(Self))
        }
    }

    let mut applier = test_applier();
    let key = std::any::TypeId::of::<PrototypeRecycleNode>();
    let seed = PrototypeRecycleNode;
    let shell = seed
        .rehouse_for_recycle()
        .expect("seed node should provide a compact shell");

    applier.seed_recycled_node_shell(key, seed.recycle_pool_limit(), shell);
    let recycled = applier
        .take_recycled_node(key)
        .expect("seeded shell should be immediately available");
    let (reused_id, node, warm_origin) = recycled.into_parts();
    applier
        .insert_with_id(reused_id, node)
        .expect("consumed warm shell should become the live cohort member");
    applier.set_recycled_node_origin(reused_id, warm_origin);

    for _ in 0..3 {
        applier.record_fresh_recyclable_creation(key);
    }
    applier.clear_recycled_nodes();

    assert_eq!(
        applier.debug_recycled_node_count_for::<PrototypeRecycleNode>(),
        3,
        "recent demand should rebuild the warm pool even after all spare shells were consumed",
    );
}

#[test]
fn large_recycle_pools_converge_to_standing_reserve_on_first_demand() {
    #[derive(Default)]
    struct LargeReserveNode;

    impl Node for LargeReserveNode {
        fn recycle_key(&self) -> Option<std::any::TypeId> {
            Some(std::any::TypeId::of::<Self>())
        }

        fn recycle_pool_limit(&self) -> Option<usize> {
            Some(128)
        }

        fn rehouse_for_recycle(&self) -> Option<Box<dyn Node>> {
            Some(Box::new(Self))
        }
    }

    let mut applier = test_applier();
    let key = std::any::TypeId::of::<LargeReserveNode>();
    let shell = LargeReserveNode
        .rehouse_for_recycle()
        .expect("large reserve node should provide a compact shell");
    applier.seed_recycled_node_shell(key, Some(128), shell);
    let recycled = applier
        .take_recycled_node(key)
        .expect("seeded shell should be immediately available");
    let (reused_id, node, warm_origin) = recycled.into_parts();
    applier
        .insert_with_id(reused_id, node)
        .expect("consumed shell should remain live");
    applier.set_recycled_node_origin(reused_id, warm_origin);

    applier.record_fresh_recyclable_creation(key);
    applier.clear_recycled_nodes();

    assert_eq!(
        applier.debug_recycled_node_count_for::<LargeReserveNode>(),
        32,
        "large recyclable types should pre-warm the standing reserve instead of growing it only after a spike",
    );
}

#[test]
fn clear_recycled_nodes_releases_excess_warm_id_capacity() {
    #[derive(Default)]
    struct LargeReserveNode;

    impl Node for LargeReserveNode {
        fn recycle_key(&self) -> Option<std::any::TypeId> {
            Some(std::any::TypeId::of::<Self>())
        }

        fn recycle_pool_limit(&self) -> Option<usize> {
            Some(128)
        }

        fn rehouse_for_recycle(&self) -> Option<Box<dyn Node>> {
            Some(Box::new(Self))
        }
    }

    let mut applier = test_applier();
    let key = std::any::TypeId::of::<LargeReserveNode>();
    let mut live_ids = Vec::with_capacity(4096);

    for _ in 0..4096 {
        if let Some(recycled) = applier.take_recycled_node(key) {
            let (id, node, warm_origin) = recycled.into_parts();
            applier
                .insert_with_id(id, node)
                .expect("reinsert recycled node");
            applier.set_recycled_node_origin(id, warm_origin);
            live_ids.push(id);
        } else {
            let node = Box::new(LargeReserveNode);
            applier.record_fresh_recyclable_creation(key);
            if let Some(shell) = node.rehouse_for_recycle() {
                applier.seed_recycled_node_shell(key, node.recycle_pool_limit(), shell);
            }
            let id = applier.create(node);
            live_ids.push(id);
        }
    }

    for id in live_ids {
        applier.remove(id).expect("remove recyclable node");
    }

    applier.clear_recycled_nodes();

    let stats = applier.debug_stats();
    assert!(
        stats.warm_recycled_node_id_count <= 32,
        "warm id bookkeeping should converge to the standing reserve instead of keeping spike-era ids live: {stats:?}",
    );
    assert!(
        stats.warm_recycled_node_id_capacity
            <= stats.warm_recycled_node_id_count.max(32).saturating_mul(4),
        "warm id bookkeeping retained excess capacity after the frame boundary: {stats:?}",
    );
}

#[test]
fn compact_prunes_stable_generation_entries_for_removed_nodes() {
    #[derive(Default)]
    struct PlainNode;

    impl Node for PlainNode {}

    let mut applier = test_applier();
    let keep = applier.create(Box::new(PlainNode));
    let removed: Vec<_> = (0..4096)
        .map(|_| applier.create(Box::new(PlainNode)))
        .collect();

    for id in removed {
        applier.remove(id).expect("remove node");
    }

    applier.compact();

    let stats = applier.debug_stats();
    assert_eq!(stats.stable_to_physical_len, 1);
    assert_eq!(stats.nodes_len, 1);
    assert_eq!(keep, 0);
    assert_eq!(
        stats.stable_generations_len, 1,
        "dead node generations should be pruned after compaction instead of retaining a dense arena",
    );
    assert!(
        stats.next_stable_id > keep,
        "stable ids must stay monotonic even after pruning generation entries",
    );
}

#[test]
fn remove_balanced_tree_uses_depth_bounded_traversal_stack() {
    #[derive(Default)]
    struct TreeNode {
        children: Vec<NodeId>,
        parent: Option<NodeId>,
    }

    impl Node for TreeNode {
        fn insert_child(&mut self, child: NodeId) -> bool {
            if self.children.contains(&child) {
                return false;
            }
            self.children.push(child);
            true
        }

        fn remove_child(&mut self, child: NodeId) -> bool {
            let before = self.children.len();
            self.children.retain(|&id| id != child);
            self.children.len() < before
        }

        fn children(&self) -> Vec<NodeId> {
            self.children.clone()
        }

        fn on_attached_to_parent(&mut self, parent: NodeId) {
            self.parent = Some(parent);
        }

        fn on_removed_from_parent(&mut self) {
            self.parent = None;
        }

        fn parent(&self) -> Option<NodeId> {
            self.parent
        }
    }

    fn build_balanced_binary_tree(applier: &mut MemoryApplier, remaining_depth: usize) -> NodeId {
        let node_id = applier.create(Box::new(TreeNode::default()));
        if remaining_depth == 0 {
            return node_id;
        }

        let left = build_balanced_binary_tree(applier, remaining_depth - 1);
        let right = build_balanced_binary_tree(applier, remaining_depth - 1);

        applier
            .with_node::<TreeNode, _>(node_id, |node| {
                node.insert_child(left);
                node.insert_child(right);
            })
            .expect("attach children to parent");

        for child_id in [left, right] {
            applier
                .with_node::<TreeNode, _>(child_id, |node| {
                    node.on_attached_to_parent(node_id);
                })
                .expect("attach parent to child");
        }

        node_id
    }

    let mut applier = test_applier();
    let tree_depth = 12usize;
    let root = build_balanced_binary_tree(&mut applier, tree_depth);

    let max_depth = applier
        .debug_remove_max_traversal_depth(root)
        .expect("remove balanced tree");

    assert!(
        max_depth <= tree_depth + 1,
        "removal traversal should scale with tree depth, not subtree size: tree_depth={tree_depth} max_depth={max_depth}",
    );
    assert!(applier.is_empty(), "all nodes should be removed");
}

#[test]
fn push_parent_inherits_previous_when_reused() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = test_applier();

    let parent_id = applier.create(Box::new(RecordingNode::default()));
    let child_id = applier.create(Box::new(RecordingNode::default()));

    {
        let (composer, slots_host, applier_host) =
            setup_composer(&mut slots, &mut applier, handle.clone(), Some(parent_id));

        composer.core.last_node_reused.set(Some(true));
        composer.push_parent(parent_id);

        {
            let mut stack = composer.parent_stack();
            let frame = stack.last_mut().expect("parent frame should exist");
            frame.new_children.push(child_id);
        }

        composer.pop_parent();
        drop(composer);
        teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
    }

    applier
        .with_node(parent_id, |node: &mut RecordingNode| {
            if !node.children.contains(&child_id) {
                node.children.push(child_id);
            }
        })
        .expect("parent node exists");

    {
        let (composer, slots_host, applier_host) =
            setup_composer(&mut slots, &mut applier, handle.clone(), Some(parent_id));

        composer.core.last_node_reused.set(Some(true));
        composer.push_parent(parent_id);

        {
            let stack = composer.parent_stack();
            let frame = stack.last().expect("parent frame should exist");
            assert_eq!(
                frame.previous.as_slice(),
                [child_id],
                "When parent was reused, previous children should be inherited"
            );
        }

        composer.pop_parent();
        drop(composer);
        teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
    }
}

#[test]
fn emit_node_creates_nodes_when_parent_restored_after_conditional_removal() {
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let toggle = MutableState::with_runtime(true, runtime.clone());

    let key = location_key(file!(), line!(), column!());

    let child_ids: Rc<RefCell<Vec<NodeId>>> = Rc::new(RefCell::new(Vec::new()));

    println!("=== First render: parent visible ===");
    {
        let child_ids = Rc::clone(&child_ids);
        composition
            .render(key, move || {
                if toggle.value() {
                    with_current_composer(|composer| {
                        let _parent = composer.emit_node(|| TestDummyNode);
                        composer.core.last_node_reused.set(Some(true));
                        composer.push_parent(_parent);

                        let child = composer.emit_node(|| TestTextNode {
                            text: "Reusable Child".to_string(),
                        });
                        child_ids.borrow_mut().push(child);

                        composer.pop_parent();
                    });
                }
            })
            .expect("first render");
    }

    let first_child_id = child_ids.borrow()[0];
    println!("First child ID: {}", first_child_id);
    assert!(first_child_id > 0, "First child should be created");

    println!("=== Second render: parent hidden ===");
    toggle.set_value(false);
    {
        composition
            .render(key, move || if toggle.value() {})
            .expect("second render");
    }

    println!("=== Third render: parent restored ===");
    toggle.set_value(true);
    {
        let child_ids = Rc::clone(&child_ids);
        composition
            .render(key, move || {
                if toggle.value() {
                    with_current_composer(|composer| {
                        let _parent = composer.emit_node(|| TestDummyNode);
                        let reused = composer.core.last_node_reused.get();
                        println!("Parent reused: {:?}", reused);
                        composer.push_parent(_parent);

                        let child = composer.emit_node(|| TestTextNode {
                            text: "Reusable Child".to_string(),
                        });
                        child_ids.borrow_mut().push(child);
                        println!("Third render child ID: {}", child);

                        composer.pop_parent();
                    });
                }
            })
            .expect("third render");
    }

    let third_child_id = child_ids.borrow().last().copied().unwrap();
    println!("Third child ID: {}", third_child_id);

    assert!(
        composition.applier_mut().get_mut(third_child_id).is_ok(),
        "Child node should be successfully created after parent restoration."
    );

    assert_eq!(
        child_ids.borrow().len(),
        2,
        "Should have recorded child IDs from both visible renders"
    );
}

#[test]
fn emit_node_works_with_new_parent_having_empty_previous() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = test_applier();

    let parent_id = applier.create(Box::new(RecordingNode::default()));

    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle.clone(), Some(parent_id));

    let (_child_id, _) = composer.with_slot_host_pass(
        Rc::clone(&slots_host),
        crate::slot::SlotPassMode::Compose,
        |composer| {
            composer.with_group(location_key(file!(), line!(), column!()), |composer| {
                composer.core.last_node_reused.set(Some(false));
                composer.push_parent(parent_id);

                {
                    let stack = composer.parent_stack();
                    let frame = stack.last().expect("parent frame should exist");
                    assert!(
                        frame.previous.is_empty(),
                        "New parent should have empty previous"
                    );
                }

                let child_id = composer.emit_node(|| TestDummyNode);
                assert!(child_id > 0, "Child should be emitted successfully");
                let was_reused = composer.core.last_node_reused.get();
                assert!(
                    was_reused.is_some(),
                    "emit_node should set last_node_reused"
                );

                composer.pop_parent();
                child_id
            })
        },
    );
    drop(composer);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
}

#[test]
fn frame_callback_state_changes_are_visible_globally() {
    let (handle, _runtime) = runtime_handle();
    let state = MutableState::with_runtime(0i32, handle.clone());

    let state_for_callback = state;
    let callback_ran = Rc::new(Cell::new(false));
    let callback_ran_for_closure = callback_ran.clone();

    let _registration = handle.frame_clock().with_frame_nanos(move |_| {
        state_for_callback.set(42);
        callback_ran_for_closure.set(true);
    });

    assert_eq!(state.get(), 0);

    handle.drain_frame_callbacks(1);

    assert!(callback_ran.get(), "Frame callback should have run");

    assert_eq!(
        state.get(),
        42,
        "State change in frame callback should be visible globally"
    );
}

#[test]
fn multiple_frame_callbacks_state_visibility() {
    let (handle, _runtime) = runtime_handle();
    let state = MutableState::with_runtime(0i32, handle.clone());

    let state1 = state;
    let _reg1 = handle.frame_clock().with_frame_nanos(move |_| {
        let current = state1.get();
        state1.set(current + 10);
    });

    let state2 = state;
    let _reg2 = handle.frame_clock().with_frame_nanos(move |_| {
        let current = state2.get();
        state2.set(current + 5);
    });

    handle.drain_frame_callbacks(1);

    assert_eq!(
        state.get(),
        15,
        "Sequential frame callback state changes should accumulate correctly"
    );
}

#[test]
fn test_stale_state_handle_set_does_not_panic() {
    let _guard = reset_snapshot_runtime();
    let test_runtime = crate::runtime::TestRuntime::new();
    let handle = test_runtime.handle();

    let lease = handle.alloc_state(42u32);
    let state: MutableState<u32> = MutableState::from_lease(&lease);
    assert_eq!(state.get(), 42);

    drop(lease);

    state.set(99);
}

#[test]
fn test_stale_state_handle_try_with_returns_none() {
    let _guard = reset_snapshot_runtime();
    let test_runtime = crate::runtime::TestRuntime::new();
    let handle = test_runtime.handle();

    let lease = handle.alloc_state(42u32);
    let state: MutableState<u32> = MutableState::from_lease(&lease);
    assert!(state.is_alive());
    assert_eq!(state.try_value(), Some(42));
    assert_eq!(state.try_with(|v| *v + 1), Some(43));

    drop(lease);

    assert!(!state.is_alive());
    assert_eq!(state.try_value(), None);
    assert_eq!(state.try_with(|v| *v + 1), None);
}

#[test]
fn param_state_update_reuses_existing_buffer_via_clone_from() {
    let mut state = crate::ParamState::<String> {
        value: Some(String::with_capacity(64)),
    };
    state
        .value
        .as_mut()
        .expect("seeded string")
        .push_str("seed value");

    let initial_ptr = state.value.as_ref().expect("seeded string").as_ptr();
    let updated = "short replacement";
    assert!(state.update(&updated.to_string()));

    let stored = state.value.as_ref().expect("updated string");
    assert_eq!(stored, updated);
    assert_eq!(
        stored.as_ptr(),
        initial_ptr,
        "ParamState::update should reuse the existing String allocation when capacity permits",
    );
}

#[test]
fn reattaching_a_child_to_its_current_parent_records_no_structural_change() {
    let mut applier = test_applier();
    let parent = applier.create(Box::new(RecordingNode::default()));
    let child = applier.create(Box::new(RecordingNode::default()));

    insert_child_with_reparenting(&mut applier, parent, child);
    let _ = applier.take_structural_change_parents_attached_to(parent);

    insert_child_with_reparenting(&mut applier, parent, child);

    assert_eq!(
        applier.take_structural_change_parents_attached_to(parent),
        Vec::<NodeId>::new(),
        "re-attaching a child that is already this parent's child changes nothing"
    );
}

#[test]
fn detaching_a_child_that_is_not_attached_records_no_structural_change() {
    let mut applier = test_applier();
    let parent = applier.create(Box::new(RecordingNode::default()));
    let child = applier.create(Box::new(RecordingNode::default()));

    detach_child_from_parent(&mut applier, parent, child).expect("detach of a non-child");

    assert_eq!(
        applier.take_structural_change_parents_attached_to(parent),
        Vec::<NodeId>::new(),
        "detaching a child this parent never had removes nothing"
    );
}

#[test]
fn a_real_attach_and_a_real_detach_each_record_a_structural_change() {
    let mut applier = test_applier();
    let parent = applier.create(Box::new(RecordingNode::default()));
    let child = applier.create(Box::new(RecordingNode::default()));

    insert_child_with_reparenting(&mut applier, parent, child);
    assert_eq!(
        applier.take_structural_change_parents_attached_to(parent),
        vec![parent],
        "attaching a new child is a structural change"
    );

    detach_child_from_parent(&mut applier, parent, child).expect("detach of a real child");
    assert_eq!(
        applier.take_structural_change_parents_attached_to(parent),
        vec![parent],
        "detaching a child this parent actually had is a structural change"
    );
}

#[test]
fn reparenting_records_a_structural_change_on_both_parents() {
    let mut applier = test_applier();
    let old_parent = applier.create(Box::new(RecordingNode::default()));
    let new_parent = applier.create(Box::new(RecordingNode::default()));
    let child = applier.create(Box::new(RecordingNode::default()));

    insert_child_with_reparenting(&mut applier, old_parent, child);
    let _ = applier.take_structural_change_parents_attached_to(old_parent);
    let _ = applier.take_structural_change_parents_attached_to(new_parent);

    insert_child_with_reparenting(&mut applier, new_parent, child);

    assert_eq!(
        applier.take_structural_change_parents_attached_to(new_parent),
        vec![new_parent],
        "the new parent gained a child"
    );
}

struct DirtyFlagNode {
    children: Vec<NodeId>,
    parent: Option<NodeId>,
    needs_measure: Cell<bool>,
    needs_layout: Cell<bool>,
    needs_semantics: Cell<bool>,
    semantics_marks: Cell<usize>,
}

impl Default for DirtyFlagNode {
    fn default() -> Self {
        Self {
            children: Vec::new(),
            parent: None,
            needs_measure: Cell::new(false),
            needs_layout: Cell::new(false),
            needs_semantics: Cell::new(true),
            semantics_marks: Cell::new(0),
        }
    }
}

impl Node for DirtyFlagNode {
    fn insert_child(&mut self, child: NodeId) -> bool {
        if self.children.contains(&child) {
            return false;
        }
        self.children.push(child);
        true
    }

    fn remove_child(&mut self, child: NodeId) -> bool {
        let before = self.children.len();
        self.children.retain(|&id| id != child);
        self.children.len() < before
    }

    fn children(&self) -> Vec<NodeId> {
        self.children.clone()
    }

    fn on_attached_to_parent(&mut self, parent: NodeId) {
        self.parent = Some(parent);
    }

    fn on_removed_from_parent(&mut self) {
        self.parent = None;
    }

    fn parent(&self) -> Option<NodeId> {
        self.parent
    }

    fn mark_needs_measure(&self) {
        self.needs_measure.set(true);
    }

    fn needs_measure(&self) -> bool {
        self.needs_measure.get()
    }

    fn mark_needs_layout(&self) {
        self.needs_layout.set(true);
    }

    fn needs_layout(&self) -> bool {
        self.needs_layout.get()
    }

    fn mark_needs_semantics(&self) {
        self.needs_semantics.set(true);
        self.semantics_marks.set(self.semantics_marks.get() + 1);
    }

    fn needs_semantics(&self) -> bool {
        self.needs_semantics.get()
    }
}

fn dirty_flag_tree(applier: &mut MemoryApplier) -> (NodeId, NodeId, NodeId) {
    let grandparent = applier.create(Box::new(DirtyFlagNode::default()));
    let parent = applier.create(Box::new(DirtyFlagNode::default()));
    let child = applier.create(Box::new(DirtyFlagNode::default()));
    insert_child_with_reparenting(applier, grandparent, parent);
    insert_child_with_reparenting(applier, parent, child);
    for id in [grandparent, parent, child] {
        applier
            .with_node::<DirtyFlagNode, _>(id, |node| {
                node.needs_measure.set(false);
                node.needs_layout.set(false);
            })
            .expect("tree node exists");
    }
    (grandparent, parent, child)
}

#[test]
fn reattaching_a_dirty_child_still_bubbles_to_ancestors() {
    let mut applier = test_applier();
    let (grandparent, parent, child) = dirty_flag_tree(&mut applier);

    applier
        .with_node::<DirtyFlagNode, _>(child, |node| node.needs_measure.set(true))
        .expect("child exists");

    let mut commands = CommandQueue::default();
    commands.push(Command::AttachChild {
        parent_id: parent,
        child_id: child,
        bubble: DirtyBubble::LAYOUT_AND_MEASURE,
    });
    commands
        .apply(&mut applier)
        .expect("re-attach of an existing child succeeds");

    for (label, id) in [("parent", parent), ("grandparent", grandparent)] {
        let marked = applier
            .with_node::<DirtyFlagNode, _>(id, |node| node.needs_measure.get())
            .expect("ancestor exists");
        assert!(
            marked,
            "{label} must be marked needs_measure when a dirty child re-attaches: \
             the unchanged child list says nothing about unchanged geometry"
        );
    }
}

#[test]
fn reattaching_a_clean_child_bubbles_nothing() {
    let mut applier = test_applier();
    let (grandparent, parent, child) = dirty_flag_tree(&mut applier);

    let mut commands = CommandQueue::default();
    commands.push(Command::AttachChild {
        parent_id: parent,
        child_id: child,
        bubble: DirtyBubble::LAYOUT_AND_MEASURE,
    });
    commands
        .apply(&mut applier)
        .expect("re-attach of an existing child succeeds");

    for (label, id) in [
        ("parent", parent),
        ("grandparent", grandparent),
        ("child", child),
    ] {
        let (measure, layout, semantics_marks) = applier
            .with_node::<DirtyFlagNode, _>(id, |node| {
                (
                    node.needs_measure.get(),
                    node.needs_layout.get(),
                    node.semantics_marks.get(),
                )
            })
            .expect("tree node exists");
        assert!(
            !measure && !layout,
            "{label} must stay clean when an already-present, clean child \
             re-attaches: bubbling a no-op re-measures the tree every frame"
        );
        assert_eq!(
            semantics_marks, 0,
            "{label} must take no semantics walk either: production nodes are \
             semantics-dirty by default while semantics are off, so keying a \
             re-attach bubble on that flag walks ancestors on every no-op \
             re-attach of a steady scroll"
        );
    }
}
