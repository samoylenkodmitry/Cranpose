use super::*;

const MOVABLE_ID: &str = "movable-content";

#[derive(Default)]
struct MovableProbe {
    node: Cell<Option<NodeId>>,
    remembered: RefCell<Option<Owned<i32>>>,
    launched: RefCell<Vec<LaunchedEffectScope>>,
    launched_runs: Cell<usize>,
    disposals: Rc<Cell<usize>>,
    payload_drops: Rc<Cell<usize>>,
    node_unmounts: Rc<Cell<usize>>,
    bodies: Cell<usize>,
}

impl MovableProbe {
    fn reset_pass(&self) {
        self.node.set(None);
        self.remembered.replace(None);
    }

    fn node(&self) -> NodeId {
        self.node
            .get()
            .expect("movable content should expose a node")
    }

    fn remembered(&self) -> i32 {
        self.remembered
            .borrow()
            .as_ref()
            .expect("movable content should expose its remembered slot")
            .with(|value| *value)
    }

    fn seed_remembered(&self, value: i32) {
        self.remembered
            .borrow()
            .as_ref()
            .expect("remembered slot")
            .replace(value);
    }

    fn launched_active(&self) -> bool {
        self.launched
            .borrow()
            .last()
            .expect("movable content should have launched an effect")
            .is_active()
    }

    fn assert_untouched(&self, node: NodeId, remembered: i32, context: &str) {
        assert_eq!(self.node(), node, "node identity must survive: {context}");
        assert_eq!(self.remembered(), remembered, "remembered value: {context}");
        self.assert_alive(context);
    }

    fn assert_alive(&self, context: &str) {
        assert_eq!(self.payload_drops.get(), 0, "payload drops: {context}");
        assert_eq!(self.node_unmounts.get(), 0, "node unmounts: {context}");
        assert_eq!(
            self.launched_runs.get(),
            1,
            "launched effect runs: {context}"
        );
        assert!(
            self.launched_active(),
            "launched effect cancelled: {context}"
        );
        assert_eq!(
            self.disposals.get(),
            0,
            "disposable effect cleanups: {context}"
        );
    }
}

thread_local! {
    static PROBE: Rc<MovableProbe> = Rc::new(MovableProbe::default());
}

fn probe() -> Rc<MovableProbe> {
    PROBE.with(Rc::clone)
}

fn movable_content() {
    let probe = probe();
    probe.bodies.set(probe.bodies.get() + 1);
    with_current_composer(|composer| {
        let slot = composer.remember(|| 7_i32);
        probe.remembered.replace(Some(slot));
        let _drop_payload = composer
            .remember(|| ReentrantDropState::new(1, Rc::clone(&probe.payload_drops), false));
        let node = composer.emit_node(|| UnmountTrackingNode::new(Rc::clone(&probe.node_unmounts)));
        probe.node.set(Some(node));
    });
    let runs = Rc::clone(&probe);
    LaunchedEffect((), move |scope| {
        runs.launched_runs.set(runs.launched_runs.get() + 1);
        runs.launched.borrow_mut().push(scope);
    });
    let disposals = Rc::clone(&probe.disposals);
    DisposableEffect((), move |_| {
        DisposableEffectResult::new(move || disposals.set(disposals.get() + 1))
    });
}

fn parent_children(composition: &mut Composition<MemoryApplier>, parent: NodeId) -> Vec<NodeId> {
    composition
        .applier_mut()
        .with_node::<RecordingNode, _>(parent, |node| node.children.clone())
        .expect("parent node should exist")
}

fn child_parent(composition: &mut Composition<MemoryApplier>, child: NodeId) -> Option<NodeId> {
    composition
        .applier_mut()
        .with_node::<UnmountTrackingNode, _>(child, |node| node.parent)
        .expect("child node should exist")
}

#[composable]
fn holder(label: &'static str, show: bool) -> NodeId {
    let id = with_current_composer(|composer| composer.emit_node(RecordingNode::default));
    cranpose_core::push_parent(id);
    if show {
        movable(MOVABLE_ID, movable_content);
    }
    cranpose_core::pop_parent();
    let _ = label;
    id
}

#[derive(Clone, Default)]
struct TwoHolders {
    first: Rc<Cell<Option<NodeId>>>,
    second: Rc<Cell<Option<NodeId>>>,
}

impl TwoHolders {
    fn ids(&self) -> (NodeId, NodeId) {
        (
            self.first.get().expect("first holder"),
            self.second.get().expect("second holder"),
        )
    }
}

fn render_holders(
    composition: &mut Composition<MemoryApplier>,
    root_key: Key,
    holders: &TwoHolders,
    mut body: impl FnMut(&TwoHolders) + 'static,
) {
    let holders = holders.clone();
    composition
        .render(root_key, move || {
            probe().reset_pass();
            body(&holders);
        })
        .expect("render movable holders");
    assert_composition_valid(composition);
}

#[test]
fn movable_content_keeps_node_state_and_effects_in_both_orderings() {
    let mut composition = test_composition();
    let in_first = MutableState::with_runtime(true, composition.runtime_handle());
    let holders = TwoHolders::default();
    let root_key = location_key(file!(), line!(), column!());
    let render = |composition: &mut Composition<MemoryApplier>| {
        render_holders(composition, root_key, &holders, move |holders| {
            let show_first = in_first.value();
            holders.first.set(Some(holder("first", show_first)));
            holders.second.set(Some(holder("second", !show_first)));
        });
    };

    render(&mut composition);
    let probe = probe();
    let (first, second) = holders.ids();
    let node = probe.node();
    probe.seed_remembered(41);
    assert_eq!(parent_children(&mut composition, first), vec![node]);
    assert_eq!(probe.bodies.get(), 1);

    in_first.set_value(false);
    render(&mut composition);
    probe.assert_untouched(node, 41, "after moving to the later parent");
    assert_eq!(child_parent(&mut composition, node), Some(second));
    assert_eq!(parent_children(&mut composition, second), vec![node]);
    assert!(parent_children(&mut composition, first).is_empty());
    assert_eq!(composition.debug_slot_snapshot().retained_subtree_count, 0);

    in_first.set_value(true);
    render(&mut composition);
    probe.assert_untouched(node, 41, "after moving back to the earlier parent");
    assert_eq!(child_parent(&mut composition, node), Some(first));
    assert_eq!(parent_children(&mut composition, first), vec![node]);
    assert!(parent_children(&mut composition, second).is_empty());
    assert_eq!(composition.debug_slot_snapshot().retained_subtree_count, 0);
    assert_eq!(
        probe.bodies.get(),
        3,
        "each arrival recomposes the content once to attach its nodes"
    );
}

#[test]
fn movable_content_arrives_when_the_old_parent_releases_it_a_pass_later() {
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let show_first = MutableState::with_runtime(true, runtime.clone());
    let show_second = MutableState::with_runtime(false, runtime);
    let holders = TwoHolders::default();
    let root_key = location_key(file!(), line!(), column!());
    let render = |composition: &mut Composition<MemoryApplier>| {
        render_holders(composition, root_key, &holders, move |holders| {
            holders
                .second
                .set(Some(holder("second", show_second.value())));
            holders.first.set(Some(holder("first", show_first.value())));
        });
    };

    render(&mut composition);
    let probe = probe();
    let (first, second) = holders.ids();
    let node = probe.node();
    probe.seed_remembered(23);

    show_second.set_value(true);
    render(&mut composition);
    assert_eq!(
        child_parent(&mut composition, node),
        Some(first),
        "content stays with the parent still showing it"
    );
    assert!(
        parent_children(&mut composition, second).is_empty(),
        "a site waiting for content composes nothing"
    );
    assert_eq!(
        probe.bodies.get(),
        1,
        "the waiting site must not compose a second copy"
    );

    show_first.set_value(false);
    render(&mut composition);
    probe.assert_untouched(node, 23, "after the old parent released the content");
    assert_eq!(child_parent(&mut composition, node), Some(second));
    assert_eq!(parent_children(&mut composition, second), vec![node]);
    assert_eq!(composition.debug_slot_snapshot().retained_subtree_count, 0);
}

// forwards on purpose: a scope of its own around the movable content, so the
// test has a parent that can be skipped while the content it holds is restored.
#[composable]
fn framed_movable_content() {
    movable_content();
}

#[composable]
fn rich_holder(show: bool, payload_drops: Rc<Cell<usize>>, framed: bool) -> NodeId {
    let id = with_current_composer(|composer| composer.emit_node(RecordingNode::default));
    cranpose_core::push_parent(id);
    if show {
        with_current_composer(|composer| {
            let _before =
                composer.remember(|| ReentrantDropState::new(2, payload_drops.clone(), false));
            composer.emit_node(TrackingChild::default);
        });
        if framed {
            movable(MOVABLE_ID, framed_movable_content);
        } else {
            movable(MOVABLE_ID, movable_content);
        }
        with_current_composer(|composer| {
            let _after =
                composer.remember(|| ReentrantDropState::new(3, payload_drops.clone(), false));
            composer.emit_node(TrackingChild::default);
        });
    }
    cranpose_core::pop_parent();
    id
}

#[test]
fn movable_content_survives_the_disposal_of_the_parent_that_held_it() {
    let mut composition = test_composition();
    let show_holder = MutableState::with_runtime(true, composition.runtime_handle());
    let holder_drops = Rc::new(Cell::new(0));
    let holders = TwoHolders::default();
    let root_key = location_key(file!(), line!(), column!());
    let render = |composition: &mut Composition<MemoryApplier>| {
        let holder_drops = Rc::clone(&holder_drops);
        render_holders(composition, root_key, &holders, move |holders| {
            let show = show_holder.value();
            holders.second.set(Some(holder("target", !show)));
            if show {
                holders
                    .first
                    .set(Some(rich_holder(true, Rc::clone(&holder_drops), false)));
            }
        });
    };

    render(&mut composition);
    let probe = probe();
    let (holder, target) = holders.ids();
    let node = probe.node();
    probe.seed_remembered(99);
    assert_eq!(parent_children(&mut composition, holder).len(), 3);

    show_holder.set_value(false);
    render(&mut composition);
    probe.assert_untouched(node, 99, "after the holding parent was disposed");
    assert_eq!(child_parent(&mut composition, node), Some(target));
    assert_eq!(parent_children(&mut composition, target), vec![node]);
    assert!(
        composition.applier_mut().get_mut(holder).is_err(),
        "the disposed parent must leave the applier"
    );
    assert_eq!(
        holder_drops.get(),
        2,
        "the disposed parent's own payloads drop while the movable's stay"
    );
    assert_eq!(composition.debug_slot_snapshot().retained_subtree_count, 0);
}

#[test]
fn movable_content_shown_twice_leaves_the_later_site_empty() {
    let mut composition = test_composition();
    let tick = MutableState::with_runtime(0, composition.runtime_handle());
    let holders = TwoHolders::default();
    let root_key = location_key(file!(), line!(), column!());
    let render = |composition: &mut Composition<MemoryApplier>| {
        render_holders(composition, root_key, &holders, move |holders| {
            let _ = tick.value();
            holders.first.set(Some(holder("first", true)));
            holders.second.set(Some(holder("second", true)));
        });
    };

    render(&mut composition);
    let probe = probe();
    let (first, second) = holders.ids();
    let node = probe.node();
    assert_eq!(parent_children(&mut composition, first), vec![node]);
    assert!(parent_children(&mut composition, second).is_empty());
    assert_eq!(probe.bodies.get(), 1);

    tick.set_value(1);
    render(&mut composition);
    assert_eq!(parent_children(&mut composition, first), vec![node]);
    assert!(parent_children(&mut composition, second).is_empty());
    assert_eq!(probe.bodies.get(), 1, "the duplicate site never composes");
    assert_eq!(composition.debug_slot_snapshot().retained_subtree_count, 0);
}

#[test]
fn movable_content_is_pinned_until_forgotten() {
    let mut composition = test_composition_retaining_at_most(1);
    let show = MutableState::with_runtime(true, composition.runtime_handle());
    let holders = TwoHolders::default();
    let root_key = location_key(file!(), line!(), column!());
    let plain_key = location_key(file!(), line!(), column!());
    let render = |composition: &mut Composition<MemoryApplier>| {
        render_holders(composition, root_key, &holders, move |holders| {
            let show = show.value();
            holders.first.set(Some(holder("only", show)));
            if show {
                with_current_composer(|composer| {
                    composer.cranpose_with_reuse(
                        plain_key,
                        RecomposeOptions::default(),
                        |composer| {
                            let _plain = composer.remember(|| 5_i32);
                        },
                    );
                });
            }
        });
    };

    render(&mut composition);
    let probe = probe();
    let node = probe.node();

    show.set_value(false);
    render(&mut composition);
    let stats = composition.debug_slot_table_stats();
    assert_eq!(
        stats.retained_subtree_count, 1,
        "the budget evicts the plain retained group, never the movable"
    );
    assert_eq!(stats.retained_evictions_total, 1);
    assert_eq!(probe.payload_drops.get(), 0);
    assert!(probe.launched_active());
    assert!(composition.applier_mut().get_mut(node).is_ok());

    forget_movable(MOVABLE_ID);
    render(&mut composition);
    assert_eq!(composition.debug_slot_snapshot().retained_subtree_count, 0);
    assert_eq!(
        probe.payload_drops.get(),
        1,
        "forgetting drops the payloads"
    );
    assert_eq!(
        probe.disposals.get(),
        1,
        "forgetting runs the disposable cleanup"
    );
    assert!(
        !probe.launched_active(),
        "forgetting cancels the launched effect"
    );
    assert!(
        composition.applier_mut().get_mut(node).is_err(),
        "forgetting removes the node"
    );

    show.set_value(true);
    render(&mut composition);
    assert_ne!(probe.node(), node, "forgotten content composes fresh");
    assert_eq!(probe.remembered(), 7);
}

thread_local! {
    static LEAF_RUNS: Cell<usize> = const { Cell::new(0) };
    static LEAF_NODE: Cell<Option<NodeId>> = const { Cell::new(None) };
    static LEAF_EXTRA: Cell<Option<NodeId>> = const { Cell::new(None) };
}

#[composable]
fn leaf(counter: MutableState<i32>) {
    let value = counter.value();
    LEAF_RUNS.with(|runs| runs.set(runs.get() + 1));
    let id = with_current_composer(|composer| composer.emit_node(TrackingChild::default));
    cranpose_core::with_node_mut::<TrackingChild, _>(id, |node| node.label = value.to_string())
        .expect("leaf node should accept its label");
    LEAF_NODE.with(|node| node.set(Some(id)));
    if value > 0 {
        let extra = with_current_composer(|composer| composer.emit_node(TrackingChild::default));
        LEAF_EXTRA.with(|node| node.set(Some(extra)));
    }
}

// forwards on purpose: likewise a scope, so the test can ask whether
// recomposition reaches through it.
#[composable]
fn inner(counter: MutableState<i32>) {
    leaf(counter);
}

#[composable]
fn scope_holder(show: bool, counter: MutableState<i32>) -> NodeId {
    let id = with_current_composer(|composer| composer.emit_node(RecordingNode::default));
    cranpose_core::push_parent(id);
    if show {
        movable("inner-scope", move || inner(counter));
    }
    cranpose_core::pop_parent();
    id
}

#[test]
fn movable_inner_scope_recomposes_under_the_new_parent_after_a_move() {
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let in_first = MutableState::with_runtime(true, runtime.clone());
    let counter = MutableState::with_runtime(0, runtime);
    let holders = TwoHolders::default();
    let root_key = location_key(file!(), line!(), column!());
    let render = |composition: &mut Composition<MemoryApplier>| {
        render_holders(composition, root_key, &holders, move |holders| {
            let show_first = in_first.value();
            holders.first.set(Some(scope_holder(show_first, counter)));
            holders.second.set(Some(scope_holder(!show_first, counter)));
        });
    };

    render(&mut composition);
    let (first, second) = holders.ids();
    let leaf_node = LEAF_NODE.with(|node| node.get()).expect("leaf node");
    assert_eq!(LEAF_RUNS.with(|runs| runs.get()), 1);
    assert_eq!(parent_children(&mut composition, first), vec![leaf_node]);

    in_first.set_value(false);
    render(&mut composition);
    assert_eq!(
        LEAF_RUNS.with(|runs| runs.get()),
        1,
        "a skipped composable keeps the leaf composed as it was"
    );
    assert_eq!(parent_children(&mut composition, second), vec![leaf_node]);

    counter.set_value(1);
    while composition
        .process_invalid_scopes()
        .expect("recompose the leaf")
    {}
    assert_composition_valid(&composition);
    assert_eq!(
        LEAF_RUNS.with(|runs| runs.get()),
        2,
        "the leaf scope must be active again after the move"
    );
    assert_eq!(LEAF_NODE.with(|node| node.get()), Some(leaf_node));
    let label = composition
        .applier_mut()
        .with_node::<TrackingChild, _>(leaf_node, |node| node.label.clone())
        .expect("leaf node");
    assert_eq!(label, "1");
    let extra = LEAF_EXTRA
        .with(|node| node.get())
        .expect("the recomposed leaf emits a second node");
    assert_eq!(
        parent_children(&mut composition, second),
        vec![leaf_node, extra],
        "nodes the leaf emits after the move must attach under the parent it moved to"
    );
    assert!(parent_children(&mut composition, first).is_empty());
}

#[composable]
fn wrapped_rich_holder(
    pass: u32,
    payload_drops: Rc<Cell<usize>>,
    inner: Rc<Cell<Option<NodeId>>>,
) -> NodeId {
    let _ = pass;
    let id = with_current_composer(|composer| composer.emit_node(RecordingNode::default));
    cranpose_core::push_parent(id);
    inner.set(Some(rich_holder(true, payload_drops, true)));
    cranpose_core::pop_parent();
    id
}

#[test]
fn movable_content_arriving_in_a_fresh_parent_keeps_its_place_among_siblings() {
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let torn = MutableState::with_runtime(false, runtime.clone());
    let pass = MutableState::with_runtime(0u32, runtime);
    let holder_drops = Rc::new(Cell::new(0));
    let source_drops = Rc::new(Cell::new(0));
    let holders = TwoHolders::default();
    let inner = Rc::new(Cell::new(None));
    let root_key = location_key(file!(), line!(), column!());
    let render = |composition: &mut Composition<MemoryApplier>| {
        let holder_drops = Rc::clone(&holder_drops);
        let source_drops = Rc::clone(&source_drops);
        let inner = Rc::clone(&inner);
        render_holders(composition, root_key, &holders, move |holders| {
            let torn = torn.value();
            holders
                .first
                .set(Some(rich_holder(!torn, Rc::clone(&source_drops), true)));
            if torn {
                holders.second.set(Some(wrapped_rich_holder(
                    pass.value(),
                    Rc::clone(&holder_drops),
                    Rc::clone(&inner),
                )));
            }
        });
    };

    render(&mut composition);
    let probe = probe();
    let node = probe.node();

    torn.set_value(true);
    render(&mut composition);
    let (source, wrapper) = holders.ids();
    let target = inner.get().expect("the rich holder composed");
    probe.assert_alive("after arriving in the fresh parent behind a skipped composable");
    assert_eq!(
        probe.bodies.get(),
        1,
        "the framed content is skipped on arrival, not composed again"
    );
    assert!(parent_children(&mut composition, source).is_empty());
    let children = parent_children(&mut composition, target);
    assert_eq!(children.len(), 3, "the frame's two nodes and the page");
    assert_eq!(
        children[1], node,
        "the page sits between the nodes emitted before and after it: {children:?}"
    );
    assert_eq!(parent_children(&mut composition, wrapper), vec![target]);

    pass.set_value(1);
    render(&mut composition);
    assert_eq!(
        parent_children(&mut composition, target),
        children,
        "a pass that skips the parent leaves the page where it was"
    );
    assert_eq!(
        parent_children(&mut composition, wrapper),
        vec![target],
        "the page must not be hung under the wrapper as a root of the skipped group"
    );
    assert_eq!(child_parent(&mut composition, node), Some(target));
}

#[test]
fn movable_content_crosses_into_a_subcomposition_and_back() {
    let mut composition = test_composition();
    let inside = MutableState::with_runtime(false, composition.runtime_handle());
    let sub_slots = Rc::new(SlotsHost::new(SlotTable::new()));
    let holders = TwoHolders::default();
    let root_key = location_key(file!(), line!(), column!());
    // The probe is not reset between passes here: content that arrives
    // already composed is skipped, and what matters is that it is alive and
    // under the right parent, not that its body ran again.
    let render = |composition: &mut Composition<MemoryApplier>| {
        let holders = holders.clone();
        let sub_slots = Rc::clone(&sub_slots);
        composition
            .render(root_key, move || {
                let show_inside = inside.value();
                holders.first.set(Some(holder("outside", !show_inside)));
                let host =
                    with_current_composer(|composer| composer.emit_node(RecordingNode::default));
                holders.second.set(Some(host));
                with_current_composer(|composer| {
                    composer
                        .subcompose_in(&sub_slots, Some(host), |_| {
                            if show_inside {
                                movable(MOVABLE_ID, movable_content);
                            }
                        })
                        .expect("subcomposition render");
                });
            })
            .expect("render the movable across a subcomposition");
        assert_composition_valid(composition);
    };

    render(&mut composition);
    let probe = probe();
    let (outside, host) = holders.ids();
    let node = probe.node();
    probe.seed_remembered(31);
    assert_eq!(parent_children(&mut composition, outside), vec![node]);

    inside.set_value(true);
    render(&mut composition);
    // The subcomposition composed while the old parent still held the
    // content, so it opened a placeholder and waits; it takes the content
    // over when it is driven again, the way a SubcomposeLayout re-measures.
    render(&mut composition);
    probe.assert_alive("after moving into a subcomposition's slot table");
    assert_eq!(
        probe.remembered(),
        31,
        "remembered value crossed the tables"
    );
    assert!(parent_children(&mut composition, outside).is_empty());
    assert_eq!(child_parent(&mut composition, node), Some(host));
    assert_eq!(
        composition.debug_slot_snapshot().retained_subtree_count,
        0,
        "the table it left must not go on holding it"
    );

    inside.set_value(false);
    render(&mut composition);
    render(&mut composition);
    probe.assert_alive("after coming back out of the subcomposition");
    assert_eq!(probe.remembered(), 31);
    assert_eq!(parent_children(&mut composition, outside), vec![node]);
    assert_eq!(child_parent(&mut composition, node), Some(outside));
    assert_eq!(
        sub_slots.debug_snapshot().retained_subtree_count,
        0,
        "and neither must the subcomposition's"
    );
}

#[test]
fn movable_content_written_once_moves_between_parents() {
    let mut composition = test_composition();
    let in_first = MutableState::with_runtime(true, composition.runtime_handle());
    let holders = TwoHolders::default();
    let root_key = location_key(file!(), line!(), column!());
    let render = |composition: &mut Composition<MemoryApplier>| {
        let holders = holders.clone();
        composition
            .render(root_key, move || {
                probe().reset_pass();
                // The body appears once. Which parent shows it is the only
                // thing the two branches differ in.
                let pane = rememberMovableContentOf(movable_content);
                let first = holder_showing(in_first.value(), pane.clone());
                let second = holder_showing(!in_first.value(), pane);
                holders.first.set(Some(first));
                holders.second.set(Some(second));
            })
            .expect("render movable content held as a value");
        assert_composition_valid(composition);
    };

    render(&mut composition);
    let probe = probe();
    let (first, second) = holders.ids();
    let node = probe.node();
    probe.seed_remembered(17);
    assert_eq!(parent_children(&mut composition, first), vec![node]);
    assert!(parent_children(&mut composition, second).is_empty());

    in_first.set_value(false);
    render(&mut composition);
    probe.assert_untouched(node, 17, "after the other parent showed it");
    assert!(parent_children(&mut composition, first).is_empty());
    assert_eq!(parent_children(&mut composition, second), vec![node]);
    assert_eq!(composition.debug_slot_snapshot().retained_subtree_count, 0);

    in_first.set_value(true);
    render(&mut composition);
    probe.assert_untouched(node, 17, "and back again");
    assert_eq!(parent_children(&mut composition, first), vec![node]);
}

#[composable]
fn holder_showing(show: bool, pane: MovableContent) -> NodeId {
    let id = with_current_composer(|composer| composer.emit_node(RecordingNode::default));
    cranpose_core::push_parent(id);
    if show {
        pane.show();
    }
    cranpose_core::pop_parent();
    id
}
