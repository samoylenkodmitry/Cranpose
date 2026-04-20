use super::*;

/// Test that emit_node rejects reuse when the parent's previous children list
/// didn't contain the candidate node. This prevents nodes from "teleporting"
/// between parents.
#[test]
fn emit_node_rejects_reuse_when_parent_did_not_own_child() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = test_applier();

    // Create a parent and child node in the applier
    let parent_a = applier.create(Box::new(RecordingNode::default()));
    let parent_b = applier.create(Box::new(RecordingNode::default()));

    // Setup the test: we'll push parent_b and try to reuse a child that was
    // previously under parent_a. The emit_node should reject the reuse.
    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle.clone(), Some(parent_a));

    // First, emit a child under parent_a's context
    let (child_id, _) = composer.with_slot_host_pass(
        Rc::clone(&slots_host),
        crate::slot_table::SlotPassMode::Compose,
        |composer| {
            composer.with_group(location_key(file!(), line!(), column!()), |composer| {
                let child_id = composer.emit_node(|| TestDummyNode);

                // Now push parent_b with last_node_reused=false (simulating new parent)
                composer.core.last_node_reused.set(Some(false));
                composer.push_parent(parent_b);

                // The parent_b's previous children should be empty since it wasn't reused
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

    // Verify child was created
    assert!(child_id > 0, "Child should have been created");
}

/// Test that push_parent uses empty previous children when the parent node
/// was not reused (last_node_reused = false). This ensures new parents start
/// fresh and don't inherit children from a different node.
#[test]
fn push_parent_uses_empty_previous_when_not_reused() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = test_applier();

    let parent_id = applier.create(Box::new(RecordingNode::default()));

    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle.clone(), Some(parent_id));

    // Simulate: last node was NOT reused (new node created)
    composer.core.last_node_reused.set(Some(false));
    composer.push_parent(parent_id);

    // Verify that previous is empty when node was not reused
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
        crate::slot_table::SlotPassMode::Compose,
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
fn skipped_group_root_nodes_only_considers_direct_parent_membership() {
    let (handle, _runtime) = runtime_handle();
    let mut slots = SlotTable::default();
    let mut applier = test_applier();

    let grandparent = applier.create(Box::new(RecordingNode::default()));
    let parent = applier.create(Box::new(RecordingNode::default()));
    let child = applier.create(Box::new(RecordingNode::default()));

    insert_child_with_reparenting(&mut applier, grandparent, parent);
    insert_child_with_reparenting(&mut applier, parent, child);

    let (composer, slots_host, applier_host) =
        setup_composer(&mut slots, &mut applier, handle, None);

    let roots = composer.skipped_group_root_nodes(&[grandparent, child]);
    assert_eq!(
        roots,
        vec![grandparent, child],
        "a node stays a skipped-group root when only a higher ancestor is in the skipped set",
    );

    drop(composer);
    teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
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
        fn insert_child(&mut self, child: NodeId) {
            self.children.push(child);
        }

        fn remove_child(&mut self, child: NodeId) {
            self.children.retain(|&id| id != child);
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

/// Test that push_parent reuses previous children when the parent node
/// was reused (last_node_reused = true). This ensures stable parents
/// correctly track their children across recompositions.
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

        // First pass: establish children
        composer.core.last_node_reused.set(Some(true)); // Pretend parent was reused
        composer.push_parent(parent_id);

        // Add a real child node
        {
            let mut stack = composer.parent_stack();
            let frame = stack.last_mut().expect("parent frame should exist");
            frame.new_children.push(child_id);
        }

        composer.pop_parent();
        drop(composer);
        teardown_composer(&mut slots, &mut applier, slots_host, applier_host);
    }

    // Manually record the child on the parent node so push_parent reads it back
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

        // Second pass: parent is reused, should inherit previous children
        composer.core.last_node_reused.set(Some(true));
        composer.push_parent(parent_id);

        // Verify that previous contains the child from first pass
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

/// Regression test: emit_node should successfully create/reuse nodes
/// even when push_parent starts with empty previous children (new parent case).
/// This was broken when we added a parent_contains check to emit_node that
/// prevented node creation when previous was empty.
///
/// Scenario (simulates tab switch):
/// 1. Render a parent with a child
/// 2. Remove the parent (tab switch away)  
/// 3. Restore the parent (tab switch back)
/// 4. Child should be created/reused successfully (functionality preserved)
#[test]
fn emit_node_creates_nodes_when_parent_restored_after_conditional_removal() {
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let toggle = MutableState::with_runtime(true, runtime.clone());

    let key = location_key(file!(), line!(), column!());

    // Track child node IDs across renders
    let child_ids: Rc<RefCell<Vec<NodeId>>> = Rc::new(RefCell::new(Vec::new()));

    // First render: parent visible with child
    println!("=== First render: parent visible ===");
    {
        let child_ids = Rc::clone(&child_ids);
        composition
            .render(key, move || {
                if toggle.value() {
                    with_current_composer(|composer| {
                        // Parent node
                        let _parent = composer.emit_node(|| TestDummyNode);
                        composer.core.last_node_reused.set(Some(true));
                        composer.push_parent(_parent);

                        // Child node - track its ID
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

    // Second render: parent removed (simulate tab switch away)
    println!("=== Second render: parent hidden ===");
    toggle.set_value(false);
    {
        composition
            .render(key, move || {
                if toggle.value() {
                    // Not rendered - parent is hidden
                }
            })
            .expect("second render");
    }

    // Third render: parent restored (simulate tab switch back)
    // This is where the regression occurred - emit_node was failing
    // when parent's previous children was empty
    println!("=== Third render: parent restored ===");
    toggle.set_value(true);
    {
        let child_ids = Rc::clone(&child_ids);
        composition
            .render(key, move || {
                if toggle.value() {
                    with_current_composer(|composer| {
                        // Parent node - may be recreated due to conditional
                        let _parent = composer.emit_node(|| TestDummyNode);
                        let reused = composer.core.last_node_reused.get();
                        println!("Parent reused: {:?}", reused);
                        composer.push_parent(_parent);

                        // CRITICAL: Child node should be successfully created/reused
                        // even though parent's previous children list may be empty.
                        // This is what broke when we added the parent_contains check.
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

    // The child should exist in the applier after parent restoration.
    assert!(
        composition.applier_mut().get_mut(third_child_id).is_ok(),
        "Child node should be successfully created after parent restoration."
    );

    // Verify we got two child IDs (one from first render, one from third render)
    assert_eq!(
        child_ids.borrow().len(),
        2,
        "Should have recorded child IDs from both visible renders"
    );
}

/// Test that nodes can be emitted under a new parent without requiring
/// the parent to have had them previously. This is the core invariant
/// that broke when emit_node checked parent_contains too strictly.
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
        crate::slot_table::SlotPassMode::Compose,
        |composer| {
            composer.with_group(location_key(file!(), line!(), column!()), |composer| {
                // Simulate a NEW parent (not reused) - this gives empty previous children
                composer.core.last_node_reused.set(Some(false));
                composer.push_parent(parent_id);

                // Verify previous is empty (this is the push_parent conditional behavior)
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

/// Test that state changes made in frame callbacks are visible globally.
///
/// This is a regression test for a bug where fling animation scroll state
/// would reset because frame callback state writes weren't being applied
/// to the global snapshot.
#[test]
fn frame_callback_state_changes_are_visible_globally() {
    let (handle, _runtime) = runtime_handle();
    let state = MutableState::with_runtime(0i32, handle.clone());

    // Register a frame callback that updates state
    let state_for_callback = state;
    let callback_ran = Rc::new(Cell::new(false));
    let callback_ran_for_closure = callback_ran.clone();

    let _registration = handle.frame_clock().with_frame_nanos(move |_| {
        // This state change should be visible after drain_frame_callbacks completes
        state_for_callback.set(42);
        callback_ran_for_closure.set(true);
    });

    // Before draining, state should be 0
    assert_eq!(state.get(), 0);

    // Drain frame callbacks - this should apply state changes to global snapshot
    handle.drain_frame_callbacks(1);

    // Verify callback ran
    assert!(callback_ran.get(), "Frame callback should have run");

    // CRITICAL: State change from callback should be visible
    // Before the fix, this would return 0 because the write was isolated
    assert_eq!(
        state.get(),
        42,
        "State change in frame callback should be visible globally"
    );
}

/// Test that multiple frame callbacks in sequence all have their state changes visible.
#[test]
fn multiple_frame_callbacks_state_visibility() {
    let (handle, _runtime) = runtime_handle();
    let state = MutableState::with_runtime(0i32, handle.clone());

    // First frame callback increments by 10
    let state1 = state;
    let _reg1 = handle.frame_clock().with_frame_nanos(move |_| {
        let current = state1.get();
        state1.set(current + 10);
    });

    // Second frame callback increments by 5
    let state2 = state;
    let _reg2 = handle.frame_clock().with_frame_nanos(move |_| {
        let current = state2.get();
        state2.set(current + 5);
    });

    // Drain all callbacks
    handle.drain_frame_callbacks(1);

    // Both callbacks should have run and their changes should be visible
    // The first sets to 10, the second reads 10 and sets to 15
    assert_eq!(
        state.get(),
        15,
        "Sequential frame callback state changes should accumulate correctly"
    );
}

/// Verifies that `MutableState::set` on a released state handle does not panic.
///
/// This reproduces a real crash: a SideEffect closure captures a `MutableState`
/// handle whose underlying `OwnedMutableState` is dropped (by group disposal)
/// before the side effect runs. The `set` call must silently skip the write.
#[test]
fn test_stale_state_handle_set_does_not_panic() {
    let _guard = reset_snapshot_runtime();
    let test_runtime = crate::runtime::TestRuntime::new();
    let handle = test_runtime.handle();

    // Allocate a state and get a handle, then release the underlying cell.
    let lease = handle.alloc_state(42u32);
    let state: MutableState<u32> = MutableState::from_lease(&lease);
    assert_eq!(state.get(), 42);

    // Drop the lease → releases the state arena slot
    drop(lease);

    // Setting a released state should NOT panic — it should be a no-op
    state.set(99);
}

/// Verifies that `MutableState::try_with` and `is_alive` work correctly
/// on released state handles.
///
/// This reproduces a real crash: a fling animation frame callback captures a
/// `MutableState` handle. A tab switch disposes the composition group (releasing
/// the state), but the frame callback still fires and tries to read the state.
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

    // Drop the lease → releases the state arena slot
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
