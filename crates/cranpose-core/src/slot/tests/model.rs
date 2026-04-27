use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
struct ModelPayload {
    value: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ModelNode {
    node_id: NodeId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ModelGroup {
    payloads: Vec<ModelPayload>,
    nodes: Vec<ModelNode>,
    scope_id: ScopeId,
}

impl ModelGroup {
    fn new(key: Key, node_id: NodeId) -> Self {
        Self {
            payloads: vec![ModelPayload {
                value: (key as i32) * 10,
            }],
            nodes: vec![ModelNode { node_id }],
            scope_id: ModelState::scope_id_for(key),
        }
    }

    fn remembered_value(&self) -> i32 {
        self.payloads
            .first()
            .expect("model groups carry one remembered payload")
            .value
    }

    fn increment_remembered_value(&mut self) {
        self.payloads
            .first_mut()
            .expect("model groups carry one remembered payload")
            .value += 1;
    }

    fn node_id(&self) -> NodeId {
        self.nodes
            .first()
            .expect("model groups carry one emitted node")
            .node_id
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ModelRoot {
    key: Key,
    children: Vec<Key>,
}

#[derive(Clone, Debug)]
enum ModelOperation {
    Compose {
        order: Vec<Key>,
        retain_detached: HashSet<Key>,
        mutate_values: HashSet<Key>,
    },
    RecomposeSkip {
        key: Key,
    },
}

impl ModelOperation {
    fn compact(&self) -> String {
        match self {
            Self::Compose {
                order,
                retain_detached,
                mutate_values,
            } => format!(
                "compose(order={}, retain={}, mutate={})",
                model_key_list(order),
                model_sorted_key_set(retain_detached),
                model_sorted_key_set(mutate_values)
            ),
            Self::RecomposeSkip { key } => format!("skip(key={key})"),
        }
    }
}

#[derive(Debug, Default)]
struct ModelState {
    active_roots: Vec<ModelRoot>,
    active_groups: BTreeMap<Key, ModelGroup>,
    retained_groups: BTreeMap<Key, ModelGroup>,
    next_node_id: NodeId,
}

impl ModelState {
    fn scope_id_for(key: Key) -> ScopeId {
        10_000 + key as ScopeId
    }

    fn active_order(&self) -> &[Key] {
        self.active_roots
            .first()
            .map_or(&[], |root| root.children.as_slice())
    }

    fn set_active_root(&mut self, key: Key, children: Vec<Key>) {
        self.active_roots = vec![ModelRoot { key, children }];
    }
}

#[derive(Clone, Copy, Debug)]
struct ModelScenario<'a> {
    seed: u64,
    frame_count: usize,
    parent_key: Key,
    child_static_key: Key,
    children: &'a [Key],
}

struct ModelRunContext<'a> {
    initial_seed: Option<u64>,
    frame_seed: Option<u64>,
    frame_index: usize,
    parent_key: Key,
    child_static_key: Key,
    script: &'a [ModelOperation],
}

#[derive(Debug, Eq, PartialEq)]
struct RetainedSubtreeSummary {
    key: Key,
    root_key: GroupKey,
    root_parent_anchor: AnchorId,
    group_count: usize,
    payload_count: usize,
    node_count: usize,
    scope_count: usize,
    anchor_count: usize,
    root_nodes: Vec<NodeId>,
    group_anchors: Vec<AnchorId>,
}

impl ModelScenario<'_> {
    fn run(self) {
        let mut harness = SlotHarness::new();
        let mut retained_subtrees = BTreeMap::<Key, DetachedSubtree>::new();
        let mut model = ModelState::default();
        let mut seed = self.seed;
        let mut script = Vec::<ModelOperation>::with_capacity(self.frame_count);

        for frame_index in 0..self.frame_count {
            let frame_seed = seed;
            let operation = generate_model_operation(&mut seed, &model, self.children);
            script.push(operation.clone());
            apply_model_operation_with_diagnostics(
                &mut harness,
                &mut retained_subtrees,
                &mut model,
                operation,
                self.parent_key,
                self.child_static_key,
                ModelRunContext {
                    initial_seed: Some(self.seed),
                    frame_seed: Some(frame_seed),
                    frame_index,
                    parent_key: self.parent_key,
                    child_static_key: self.child_static_key,
                    script: &script,
                },
            );
        }
    }
}

fn model_stress_frame_count_from_env() -> usize {
    const ENV_NAME: &str = "CRANPOSE_SLOT_MODEL_STRESS_FRAMES";
    let Some(value) = std::env::var_os(ENV_NAME) else {
        return 0;
    };
    let value = value
        .into_string()
        .unwrap_or_else(|_| panic!("{ENV_NAME} must be valid UTF-8"));
    value
        .parse::<usize>()
        .unwrap_or_else(|err| panic!("{ENV_NAME} must be a non-negative integer: {err}"))
}

fn model_stress_seed(index: usize) -> u64 {
    let mut value = 0xD6E8_FEB8_6659_FD93u64 ^ (index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    value ^= value >> 30;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

fn run_model_stress_scenarios_from_env() {
    const FRAMES_PER_SCENARIO: usize = 256;

    let mut remaining_frames = model_stress_frame_count_from_env();
    let mut scenario_index = 0usize;
    while remaining_frames > 0 {
        let frame_count = remaining_frames.min(FRAMES_PER_SCENARIO);
        let child_count = 4 + (scenario_index % 9);
        let child_base = 20_000 + (scenario_index as Key) * 100;
        let children = (0..child_count)
            .map(|offset| child_base + offset as Key)
            .collect::<Vec<_>>();
        let parent_key = 30_000 + (scenario_index as Key) * 2;
        let child_static_key = parent_key + 1;

        ModelScenario {
            seed: model_stress_seed(scenario_index),
            frame_count,
            parent_key,
            child_static_key,
            children: &children,
        }
        .run();

        remaining_frames -= frame_count;
        scenario_index += 1;
    }
}

fn model_key_list(keys: &[Key]) -> String {
    let mut output = String::from("[");
    for (index, key) in keys.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        write!(&mut output, "{key}").expect("writing to String cannot fail");
    }
    output.push(']');
    output
}

fn model_sorted_key_set(keys: &HashSet<Key>) -> String {
    let mut sorted = keys.iter().copied().collect::<Vec<_>>();
    sorted.sort_unstable();
    model_key_list(&sorted)
}

fn model_failure_script(script: &[ModelOperation], frame_index: usize) -> String {
    let mut output = String::new();
    for (index, operation) in script.iter().enumerate() {
        let marker = if index == frame_index {
            " <-- failure"
        } else {
            ""
        };
        writeln!(&mut output, "{index:04}: {}{marker}", operation.compact())
            .expect("writing to String cannot fail");
    }
    output
}

fn panic_payload_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_owned();
    }
    "non-string panic payload".to_owned()
}

fn retained_subtree_summary(
    retained_subtrees: &BTreeMap<Key, DetachedSubtree>,
) -> Vec<RetainedSubtreeSummary> {
    retained_subtrees
        .iter()
        .map(|(&key, subtree)| RetainedSubtreeSummary {
            key,
            root_key: subtree.root_key(),
            root_parent_anchor: subtree.root_parent_anchor(),
            group_count: subtree.group_count(),
            payload_count: subtree.payload_count(),
            node_count: subtree.node_count(),
            scope_count: subtree.scope_count(),
            anchor_count: subtree.anchor_count(),
            root_nodes: subtree.root_nodes(),
            group_anchors: subtree.group_anchors().collect(),
        })
        .collect()
}

fn model_failure_report(
    context: &ModelRunContext<'_>,
    failure: &str,
    harness: &SlotHarness,
    retained_subtrees: &BTreeMap<Key, DetachedSubtree>,
) -> String {
    let active_snapshot = panic::catch_unwind(AssertUnwindSafe(|| harness.table.debug_snapshot()))
        .map(|snapshot| format!("{snapshot:#?}"))
        .unwrap_or_else(|payload| {
            format!(
                "<active debug snapshot panicked: {}>",
                panic_payload_message(payload.as_ref())
            )
        });
    let retained_summary = retained_subtree_summary(retained_subtrees);
    let seed = context
        .initial_seed
        .map(|seed| format!("0x{seed:016x}"))
        .unwrap_or_else(|| "scripted".to_owned());
    let frame_seed = context
        .frame_seed
        .map(|seed| format!("0x{seed:016x}"))
        .unwrap_or_else(|| "scripted".to_owned());

    format!(
        "slot model failure\n\
         seed: {seed}\n\
         frame seed: {frame_seed}\n\
         frame: {}\n\
         parent key: {}\n\
         child static key: {}\n\
         failed invariant: {failure}\n\
         compact scenario script:\n{}\
         active debug snapshot:\n{active_snapshot}\n\
         retained-subtree summary:\n{retained_summary:#?}",
        context.frame_index,
        context.parent_key,
        context.child_static_key,
        model_failure_script(context.script, context.frame_index),
    )
}

fn apply_model_operation_with_diagnostics(
    harness: &mut SlotHarness,
    retained_subtrees: &mut BTreeMap<Key, DetachedSubtree>,
    model: &mut ModelState,
    operation: ModelOperation,
    parent_key: Key,
    child_static_key: Key,
    context: ModelRunContext<'_>,
) {
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        apply_model_operation(
            harness,
            retained_subtrees,
            model,
            operation,
            parent_key,
            child_static_key,
        );
    }));

    if let Err(payload) = result {
        let failure = panic_payload_message(payload.as_ref());
        eprintln!(
            "{}",
            model_failure_report(&context, &failure, harness, retained_subtrees)
        );
        panic::resume_unwind(payload);
    }
}

fn next_index(seed: &mut u64, len: usize) -> usize {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    (*seed as usize) % len
}

fn generate_model_operation(
    seed: &mut u64,
    model: &ModelState,
    children: &[Key],
) -> ModelOperation {
    let active_order = model.active_order();
    if !active_order.is_empty() && next_bool(seed) {
        let key = active_order[next_index(seed, active_order.len())];
        return ModelOperation::RecomposeSkip { key };
    }

    let mut order = children
        .iter()
        .copied()
        .filter(|_| next_bool(seed))
        .collect::<Vec<_>>();
    shuffle(&mut order, seed);

    let order_set = order.iter().copied().collect::<HashSet<_>>();
    let retain_detached = model
        .active_order()
        .iter()
        .copied()
        .filter(|key| !order_set.contains(key) && next_bool(seed))
        .collect::<HashSet<_>>();
    let mutate_values = order
        .iter()
        .copied()
        .filter(|_| next_bool(seed))
        .collect::<HashSet<_>>();

    ModelOperation::Compose {
        order,
        retain_detached,
        mutate_values,
    }
}

fn assert_model_matches_slot_table(
    harness: &SlotHarness,
    retained_subtrees: &BTreeMap<Key, DetachedSubtree>,
    model: &ModelState,
    parent_key: Key,
    child_static_key: Key,
) {
    let snapshot = harness.table.debug_snapshot();
    let active_order = model.active_order();
    assert_eq!(model.active_roots.len(), 1);
    assert_eq!(snapshot.active_groups.len(), active_order.len() + 1);
    assert_eq!(snapshot.active_payload_count, active_order.len());
    assert_eq!(snapshot.active_node_count, active_order.len());
    assert_eq!(snapshot.active_scope_count, active_order.len());
    assert_eq!(snapshot.scope_registry_count, active_order.len());

    let root = snapshot
        .active_groups
        .first()
        .expect("model test root group must exist");
    let expected_root = model
        .active_roots
        .first()
        .expect("model test root group must exist");
    assert_eq!(expected_root.key, parent_key);
    assert_eq!(root.static_key, expected_root.key);
    assert_eq!(root.depth, 0);
    assert_eq!(root.subtree_len as usize, active_order.len() + 1);
    assert_eq!(root.payload_len, 0);
    assert_eq!(root.node_len, 0);

    let active_children = snapshot
        .active_groups
        .iter()
        .skip(1)
        .copied()
        .collect::<Vec<_>>();
    assert_eq!(active_children.len(), active_order.len());

    for (group, key) in active_children.iter().zip(active_order.iter()) {
        let expected = model.active_groups.get(key).expect("active model group");
        assert_eq!(group.static_key, child_static_key);
        assert_eq!(group.explicit_key, Some(*key));
        assert_eq!(group.ordinal, 0);
        assert_eq!(group.scope_id, Some(expected.scope_id));
        assert_eq!(group.depth, 1);
        assert_eq!(group.subtree_len, 1);
        assert_eq!(group.payload_len, 1);
        assert_eq!(group.node_len, 1);
        assert_eq!(group.subtree_node_count, 1);
        assert_eq!(group.parent_anchor, root.anchor);

        let payload = harness
            .table
            .group_payload_records_at(group.index)
            .first()
            .expect("model child payload must exist")
            .value
            .downcast_ref::<i32>()
            .expect("model child payload must be i32");
        assert_eq!(
            *payload,
            expected.remembered_value(),
            "remembered values must match the reference model for key {key}"
        );
        assert_eq!(
            harness
                .table
                .group_node_records_at(group.index)
                .first()
                .expect("model child node must exist")
                .id,
            expected.node_id(),
            "node identity must match the reference model for key {key}"
        );
    }

    let active_anchors = snapshot
        .active_groups
        .iter()
        .map(|group| group.anchor)
        .collect::<HashSet<_>>();
    assert_eq!(
        active_anchors.len(),
        snapshot.active_groups.len(),
        "generated model operations must not create duplicate active anchors"
    );

    let retained_keys = retained_subtrees.keys().copied().collect::<HashSet<_>>();
    let active_keys = model.active_groups.keys().copied().collect::<HashSet<_>>();
    assert!(
        active_keys.is_disjoint(&retained_keys),
        "active groups and retained groups must stay disjoint"
    );
    assert_eq!(retained_keys.len(), model.retained_groups.len());

    for (&key, subtree) in retained_subtrees {
        let expected = model
            .retained_groups
            .get(&key)
            .expect("retained model group");
        let root_group = subtree
            .groups
            .first()
            .expect("retained subtree must contain a root group");
        assert_eq!(root_group.key.explicit_key, Some(key));
        assert_eq!(
            subtree.root_parent_anchor(),
            AnchorId::INVALID,
            "retained subtree roots must detach from the active parent chain"
        );
        for anchor in subtree.group_anchors() {
            assert!(
                !active_anchors.contains(&anchor),
                "retained subtree anchors must not remain active: key={key} anchor={anchor:?}"
            );
        }
        let payload = detached_group_payloads(subtree, 0)
            .first()
            .expect("retained subtree payload must exist")
            .value
            .downcast_ref::<i32>()
            .expect("retained subtree payload must be i32");
        assert_eq!(
            *payload,
            expected.remembered_value(),
            "retained values must match the reference model for key {key}"
        );
        assert_eq!(
            detached_group_nodes(subtree, 0)
                .first()
                .expect("retained subtree node must exist")
                .id,
            expected.node_id(),
            "retained node identity must match the reference model for key {key}"
        );
        for group in &subtree.groups {
            assert!(
                !active_anchors.contains(&group.anchor),
                "active and retained storage must stay disjoint"
            );
        }
    }

    assert_eq!(harness.table.validate(), Ok(()));
}

fn apply_model_operation(
    harness: &mut SlotHarness,
    retained_subtrees: &mut BTreeMap<Key, DetachedSubtree>,
    model: &mut ModelState,
    operation: ModelOperation,
    parent_key: Key,
    child_static_key: Key,
) {
    match operation {
        ModelOperation::Compose {
            order,
            retain_detached,
            mutate_values,
        } => {
            let previous_active = model.active_groups.clone();
            let previous_retained = model.retained_groups.clone();
            let mut next_retained = previous_retained.clone();
            let mut next_active = BTreeMap::<Key, ModelGroup>::new();
            let mut next_node_id = model.next_node_id;
            let mut carried_retained = std::mem::take(retained_subtrees);

            harness.begin_pass(SlotPassMode::Compose);
            let detached_children = harness.session(|session| {
                begin_unkeyed(session, parent_key, None);

                for key in order.iter().copied() {
                    let restored = carried_retained.remove(&key);
                    let started = begin_keyed(session, child_static_key, key, restored);
                    let existing_active = previous_active.get(&key).cloned();
                    let existing_retained = previous_retained.get(&key).cloned();

                    match (existing_active.is_some(), existing_retained.is_some()) {
                        (_, true) => assert_eq!(
                            started.kind,
                            GroupStartKind::Restored,
                            "retained children must restore instead of inserting or reusing"
                        ),
                        (true, false) => assert!(
                            matches!(started.kind, GroupStartKind::Reused | GroupStartKind::Moved),
                            "active keyed children must reuse or move; got {:?} for key {key}",
                            started.kind
                        ),
                        (false, false) => assert_eq!(
                            started.kind,
                            GroupStartKind::Inserted,
                            "new keyed children must insert"
                        ),
                    }

                    let mut child = existing_active.or(existing_retained).unwrap_or_else(|| {
                        let node_id = {
                            let node_id = next_node_id;
                            next_node_id += 1;
                            node_id
                        };
                        ModelGroup::new(key, node_id)
                    });

                    next_retained.remove(&key);
                    session.set_group_scope(started.group, child.scope_id);
                    let slot =
                        session.value_slot_with_kind(PayloadKind::Internal, || (key as i32) * 10);
                    session.record_node_with_parent(child.node_id(), 1, None);
                    if mutate_values.contains(&key) {
                        child.increment_remembered_value();
                        session.table.write_value(slot, child.remembered_value());
                    }

                    let child_result = session.finish_group_body();
                    assert!(child_result.detached_children.is_empty());
                    session.end_group();
                    next_active.insert(key, child);
                }

                let parent_result = session.finish_group_body();
                session.end_group();
                parent_result.detached_children
            });
            harness.finish_pass();

            for subtree in detached_children {
                let key = subtree
                    .root_key()
                    .explicit_key
                    .expect("generated model test uses explicit child keys");
                let expected_child = previous_active
                    .get(&key)
                    .cloned()
                    .expect("detached child must have been active previously");
                if retain_detached.contains(&key) {
                    next_retained.insert(key, expected_child);
                    carried_retained.insert(key, subtree);
                }
            }

            model.set_active_root(parent_key, order);
            model.active_groups = next_active;
            model.retained_groups = next_retained;
            model.next_node_id = next_node_id;
            *retained_subtrees = carried_retained;
        }
        ModelOperation::RecomposeSkip { key } => {
            let before = harness.table.debug_snapshot();
            harness.begin_pass(SlotPassMode::Recompose);
            let group_index = harness.session(|session| {
                let group = session
                    .begin_recompose_at_scope(ModelState::scope_id_for(key))
                    .expect("active scopes must resolve through the scope index");
                session.skip_group();
                let result = session.finish_group_body();
                assert!(result.detached_children.is_empty());
                session.end_recompose();
                group.index()
            });
            harness.finish_pass();
            assert_eq!(
                harness.table.groups[group_index].key.explicit_key,
                Some(key),
                "recompose must target the expected keyed child",
            );
            assert_eq!(
                harness.table.debug_snapshot(),
                before,
                "skipping a recomposed keyed group must leave the active tree unchanged",
            );
        }
    }

    assert_model_matches_slot_table(
        harness,
        retained_subtrees,
        model,
        parent_key,
        child_static_key,
    );
}

fn model_key_set(keys: &[Key]) -> HashSet<Key> {
    keys.iter().copied().collect()
}

fn model_compose_frame(
    order: &[Key],
    retain_detached: &[Key],
    mutate_values: &[Key],
) -> ModelOperation {
    ModelOperation::Compose {
        order: order.to_vec(),
        retain_detached: model_key_set(retain_detached),
        mutate_values: model_key_set(mutate_values),
    }
}

fn run_model_script(parent_key: Key, child_static_key: Key, script: &[ModelOperation]) {
    let mut harness = SlotHarness::new();
    let mut retained_subtrees = BTreeMap::<Key, DetachedSubtree>::new();
    let mut model = ModelState::default();

    for (frame_index, operation) in script.iter().cloned().enumerate() {
        apply_model_operation_with_diagnostics(
            &mut harness,
            &mut retained_subtrees,
            &mut model,
            operation,
            parent_key,
            child_static_key,
            ModelRunContext {
                initial_seed: None,
                frame_seed: None,
                frame_index,
                parent_key,
                child_static_key,
                script,
            },
        );
    }
}

#[test]
fn model_operation_compact_format_is_deterministic() {
    let operation = model_compose_frame(&[3, 1, 2], &[8, 4, 6], &[7, 5]);

    assert_eq!(
        operation.compact(),
        "compose(order=[3,1,2], retain=[4,6,8], mutate=[5,7])"
    );
    assert_eq!(
        ModelOperation::RecomposeSkip { key: 11 }.compact(),
        "skip(key=11)"
    );
}

#[test]
fn deterministic_model_render_frames_match_slot_table() {
    let scenarios = [
        ModelScenario {
            seed: 0x9E37_79B9_7F4A_7C15,
            frame_count: 96,
            parent_key: 700,
            child_static_key: 701,
            children: &[1, 2, 3, 4],
        },
        ModelScenario {
            seed: 0xD1B5_4A32_D192_ED03,
            frame_count: 128,
            parent_key: 710,
            child_static_key: 711,
            children: &[11, 12, 13, 14, 15, 16],
        },
        ModelScenario {
            seed: 0xA24B_AED4_963E_E407,
            frame_count: 160,
            parent_key: 720,
            child_static_key: 721,
            children: &[21, 22, 23, 24, 25, 26, 27, 28],
        },
    ];

    for scenario in scenarios {
        scenario.run();
    }
    run_model_stress_scenarios_from_env();
}

#[test]
fn scripted_model_render_frames_cover_slot_table_behaviors() {
    let script = [
        model_compose_frame(&[1, 2, 3], &[], &[1, 2]),
        model_compose_frame(&[3, 2, 1], &[], &[3]),
        ModelOperation::RecomposeSkip { key: 2 },
        model_compose_frame(&[3, 1], &[2], &[]),
        model_compose_frame(&[2, 3, 1], &[], &[2]),
        model_compose_frame(&[10], &[2, 3, 1], &[10]),
        ModelOperation::RecomposeSkip { key: 10 },
        model_compose_frame(&[1, 2], &[], &[1]),
        model_compose_frame(&[], &[1, 2], &[]),
        model_compose_frame(&[2, 1, 4], &[], &[4]),
    ];

    run_model_script(730, 731, &script);
}
