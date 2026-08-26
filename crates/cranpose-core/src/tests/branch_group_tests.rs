use super::*;

thread_local! {
    static BRANCH_INITS: Cell<usize> = const { Cell::new(0) };
    static BRANCH_SEEN: Cell<i32> = const { Cell::new(0) };
    static BRANCH_LOG: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn reset_branch_probes() {
    BRANCH_INITS.with(|count| count.set(0));
    BRANCH_SEEN.with(|seen| seen.set(0));
    BRANCH_LOG.with(|log| log.borrow_mut().clear());
}

fn branch_inits() -> usize {
    BRANCH_INITS.with(Cell::get)
}

fn branch_seen() -> i32 {
    BRANCH_SEEN.with(Cell::get)
}

fn branch_log() -> Vec<String> {
    BRANCH_LOG.with(|log| log.borrow().clone())
}

fn remember_branch_marker(marker: i32) -> i32 {
    let value = remember(|| {
        BRANCH_INITS.with(|count| count.set(count.get() + 1));
        marker
    });
    value.with(|value| *value)
}

#[composable]
fn if_branch_remember_probe(cond: bool) {
    if cond {
        let value = remember_branch_marker(1);
        BRANCH_SEEN.with(|seen| seen.set(value));
    } else {
        let value = remember_branch_marker(2);
        BRANCH_SEEN.with(|seen| seen.set(value));
    }
}

#[test]
fn an_if_branch_owns_its_remembered_state() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(1, || if_branch_remember_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    pass(&mut composition, false).expect("switch to the else branch");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 2),
        "the else branch must not inherit the then branch's remember slot"
    );

    pass(&mut composition, true).expect("switch back to the then branch");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (3, 1),
        "returning to a departed branch must compose fresh state"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn match_arm_remember_probe(route: u8) {
    match route {
        0 => {
            let value = remember_branch_marker(10);
            BRANCH_SEEN.with(|seen| seen.set(value));
        }
        1 => {
            let value = remember_branch_marker(20);
            BRANCH_SEEN.with(|seen| seen.set(value));
        }
        _ => {
            let value = remember_branch_marker(30);
            BRANCH_SEEN.with(|seen| seen.set(value));
        }
    }
}

#[test]
fn match_arms_do_not_share_remembered_state() {
    reset_branch_probes();
    let mut composition = test_composition();

    let mut expected_inits = 0;
    for (route, marker) in [(0u8, 10), (1, 20), (2, 30), (0, 10), (2, 30)] {
        expected_inits += 1;
        composition
            .render(2, || match_arm_remember_probe(route))
            .expect("compose the routed arm");
        assert_eq!(
            (branch_inits(), branch_seen()),
            (expected_inits, marker),
            "arm for route {route} must own its remember slot"
        );
    }
    assert_composition_valid(&composition);
}

#[composable]
fn stateful_child(marker: i32) {
    let state = rememberMutableStateOf(|| marker);
    BRANCH_SEEN.with(|seen| seen.set(state.value()));
}

#[composable]
fn branches_calling_the_same_child(cond: bool) {
    if cond {
        stateful_child(100);
    } else {
        stateful_child(200);
    }
}

#[test]
fn the_same_child_in_both_branches_gets_per_branch_state() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(3, || branches_calling_the_same_child(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!(branch_seen(), 100);

    pass(&mut composition, false).expect("switch to the else branch");
    assert_eq!(
        branch_seen(),
        200,
        "the else branch's child call must not be handed the then branch's child state"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn else_if_chain_probe(route: u8) {
    if route == 0 {
        let value = remember_branch_marker(11);
        BRANCH_SEEN.with(|seen| seen.set(value));
    } else if route == 1 {
        let value = remember_branch_marker(22);
        BRANCH_SEEN.with(|seen| seen.set(value));
    } else {
        let value = remember_branch_marker(33);
        BRANCH_SEEN.with(|seen| seen.set(value));
    }
}

#[test]
fn each_arm_of_an_else_if_chain_owns_its_slots() {
    reset_branch_probes();
    let mut composition = test_composition();

    let mut expected_inits = 0;
    for (route, marker) in [(0u8, 11), (1, 22), (2, 33), (1, 22), (0, 11)] {
        expected_inits += 1;
        composition
            .render(4, || else_if_chain_probe(route))
            .expect("compose the chain arm");
        assert_eq!(
            (branch_inits(), branch_seen()),
            (expected_inits, marker),
            "chain arm for route {route} must own its remember slot"
        );
    }
    assert_composition_valid(&composition);
}

#[composable]
fn effect_child(label: &'static str) {
    DisposableEffect!(0, move |scope| {
        BRANCH_LOG.with(|log| log.borrow_mut().push(format!("{label}:start")));
        scope.on_dispose(move || {
            BRANCH_LOG.with(|log| log.borrow_mut().push(format!("{label}:dispose")));
        })
    });
}

#[composable]
fn branch_effect_probe(cond: bool) {
    if cond {
        effect_child("then");
    } else {
        effect_child("else");
    }
}

#[test]
fn a_branch_switch_disposes_the_effects_of_the_departed_branch() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(5, || branch_effect_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!(branch_log(), vec!["then:start".to_string()]);

    pass(&mut composition, false).expect("switch to the else branch");
    let log = branch_log();
    assert!(
        log.contains(&"then:dispose".to_string()),
        "the departed branch's effect must be disposed, log: {log:?}"
    );
    assert!(
        log.contains(&"else:start".to_string()),
        "the arriving branch's effect must start, log: {log:?}"
    );
    assert_eq!(log.len(), 3, "log: {log:?}");
    assert_composition_valid(&composition);
}

#[composable]
fn value_only_branch_probe(cond: bool) {
    let margin = if cond { 4_i32 } else { 8 };
    let label = if cond {
        "on".to_string()
    } else {
        format!("off {margin}")
    };
    BRANCH_SEEN.with(|seen| seen.set(margin + label.len() as i32));
    let _anchor = remember(|| 0_i32);
}

#[composable]
fn no_branch_probe() {
    let margin = 4_i32;
    let label = "on".to_string();
    BRANCH_SEEN.with(|seen| seen.set(margin + label.len() as i32));
    let _anchor = remember(|| 0_i32);
}

#[test]
fn a_value_only_conditional_adds_no_group() {
    reset_branch_probes();
    let mut with_branch = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(6, || value_only_branch_probe(value))
    };
    pass(&mut with_branch, true).expect("compose the value-only branch probe");
    let with_branch_groups = with_branch.debug_dump_slot_table_groups().len();

    let mut without_branch = test_composition();
    without_branch
        .render(6, no_branch_probe)
        .expect("compose the branch-free probe");
    let without_branch_groups = without_branch.debug_dump_slot_table_groups().len();

    assert_eq!(
        with_branch_groups, without_branch_groups,
        "a conditional whose branches cannot reach the composer must not open groups"
    );

    pass(&mut with_branch, false).expect("flip the value-only branch");
    assert_eq!(branch_seen(), 8 + "off 8".len() as i32);
    assert_composition_valid(&with_branch);
}

#[composable]
fn format_arg_probe(cond: bool) {
    let label = if cond {
        format!("a{}", remember_branch_marker(41))
    } else {
        format!("b{}", remember_branch_marker(42))
    };
    BRANCH_LOG.with(|log| log.borrow_mut().push(label));
}

#[test]
fn a_composable_call_inside_a_value_macro_still_gets_a_branch_group() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(16, || format_arg_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!(branch_inits(), 1);
    pass(&mut composition, false).expect("switch the format-arg branch");
    assert_eq!(
        (branch_inits(), branch_log().last().cloned()),
        (2, Some("b42".to_string())),
        "the else branch's remember inside format! must compose fresh"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn early_return_probe(cond: bool) {
    let _anchor = remember(|| 0_i32);
    if cond {
        let value = remember_branch_marker(7);
        BRANCH_SEEN.with(|seen| seen.set(value));
        return;
    }
    let value = remember_branch_marker(8);
    BRANCH_SEEN.with(|seen| seen.set(value));
}

#[test]
fn an_early_return_branch_keeps_the_slot_table_balanced() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(7, || early_return_probe(value))
    };

    pass(&mut composition, true).expect("compose the early-return branch");
    assert_eq!((branch_inits(), branch_seen()), (1, 7));
    assert_composition_valid(&composition);

    pass(&mut composition, false).expect("compose past the early return");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 8),
        "the tail after the conditional must not read the departed branch's slot"
    );
    assert_composition_valid(&composition);

    pass(&mut composition, true).expect("return to the early-return branch");
    assert_eq!((branch_inits(), branch_seen()), (3, 7));
    assert_composition_valid(&composition);
}

#[composable]
fn try_branch_probe(cond: bool) -> Result<i32, i32> {
    if cond {
        let _value = remember_branch_marker(9);
        Err(7)?;
    }
    let value = remember_branch_marker(4);
    BRANCH_SEEN.with(|seen| seen.set(value));
    Ok(1)
}

#[test]
fn the_question_mark_operator_unwinds_branch_groups_cleanly() {
    reset_branch_probes();
    let mut composition = test_composition();

    let result = Cell::new(Ok(0));
    composition
        .render(8, || result.set(try_branch_probe(true)))
        .expect("compose the erroring branch");
    assert_eq!(result.get(), Err(7));
    assert_composition_valid(&composition);

    composition
        .render(8, || result.set(try_branch_probe(false)))
        .expect("compose past the fallible branch");
    assert_eq!(result.get(), Ok(1));
    assert_eq!(branch_seen(), 4);
    assert_composition_valid(&composition);
}

#[composable]
fn nested_branch_probe(outer: bool, inner: bool) {
    if outer {
        if inner {
            let value = remember_branch_marker(51);
            BRANCH_SEEN.with(|seen| seen.set(value));
        } else {
            let value = remember_branch_marker(52);
            BRANCH_SEEN.with(|seen| seen.set(value));
        }
    } else {
        let value = remember_branch_marker(53);
        BRANCH_SEEN.with(|seen| seen.set(value));
    }
}

#[test]
fn nested_conditionals_key_every_level() {
    reset_branch_probes();
    let mut composition = test_composition();

    let mut expected_inits = 0;
    for (outer, inner, marker) in [
        (true, true, 51),
        (true, false, 52),
        (false, false, 53),
        (true, true, 51),
    ] {
        expected_inits += 1;
        composition
            .render(9, || nested_branch_probe(outer, inner))
            .expect("compose the nested branch");
        assert_eq!(
            (branch_inits(), branch_seen()),
            (expected_inits, marker),
            "nested branch (outer={outer}, inner={inner}) must own its slots"
        );
    }
    assert_composition_valid(&composition);
}

#[composable]
fn branch_reads_state_probe(toggle: MutableState<bool>, counter: MutableState<i32>) {
    BRANCH_INITS.with(|count| count.set(count.get() + 1));
    if toggle.value() {
        BRANCH_SEEN.with(|seen| seen.set(counter.value()));
    } else {
        BRANCH_SEEN.with(|seen| seen.set(-1));
    }
}

#[test]
fn state_read_inside_a_branch_recomposes_in_one_pass() {
    reset_branch_probes();
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let toggle = MutableState::with_runtime(true, runtime.clone());
    let counter = MutableState::with_runtime(5, runtime);

    composition
        .render(10, || branch_reads_state_probe(toggle, counter))
        .expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 5));

    counter.set_value(6);
    assert!(
        composition.process_invalid_scopes().expect("recompose"),
        "the state write must invalidate the enclosing function scope"
    );
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 6),
        "one recomposition pass must re-run the branch"
    );
    assert!(
        !composition
            .process_invalid_scopes()
            .expect("check for residual invalidations"),
        "a branch read must not need a second promotion pass"
    );
    assert_composition_valid(&composition);
}

fn plain_helper(value: i32) -> i32 {
    value
}

#[composable]
fn closure_branch_probe(cond: bool) {
    let callback: Box<dyn Fn() -> i32 + 'static> = Box::new(move || {
        if cond {
            plain_helper(1)
        } else {
            plain_helper(2)
        }
    });
    BRANCH_SEEN.with(|seen| seen.set(callback()));
    let _anchor = remember(|| 0_i32);
}

#[test]
fn conditionals_inside_plain_closures_are_left_alone() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(11, || closure_branch_probe(value))
    };
    pass(&mut composition, true).expect("compose the closure probe");
    assert_eq!(branch_seen(), 1);
    pass(&mut composition, false).expect("flip the closure probe");
    assert_eq!(branch_seen(), 2);
    assert_composition_valid(&composition);
}

#[composable]
#[allow(non_snake_case)]
fn BranchChild(marker: i32) {
    let state = rememberMutableStateOf(|| {
        BRANCH_INITS.with(|count| count.set(count.get() + 1));
        marker
    });
    BRANCH_SEEN.with(|seen| seen.set(state.value()));
}

#[composable]
fn content_host<F: FnMut() + 'static>(content: F) {
    content();
}

#[composable]
fn content_closure_branch_probe(cond: bool) {
    content_host(move || {
        if cond {
            BranchChild(1000);
        } else {
            BranchChild(2000);
        }
    });
}

#[test]
fn content_closure_branches_own_their_slots() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(13, || content_closure_branch_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1000));

    pass(&mut composition, false).expect("switch the content closure's branch");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 2000),
        "a branch inside a content closure must own its slots"
    );

    pass(&mut composition, true).expect("switch back");
    assert_eq!((branch_inits(), branch_seen()), (3, 1000));
    assert_composition_valid(&composition);
}

#[composable]
fn indexed_content_host(index: i32, content: impl FnMut(i32) + 'static) {
    let mut content = content;
    content(index);
}

#[composable]
fn indexed_closure_branch_probe(cond: bool) {
    indexed_content_host(7, move |index| {
        if cond {
            BranchChild(3000 + index);
        } else {
            BranchChild(4000 + index);
        }
    });
}

#[test]
fn an_argument_taking_content_closure_gets_branch_groups_too() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(15, || indexed_closure_branch_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 3007));

    pass(&mut composition, false).expect("switch the indexed content branch");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 4007),
        "a branch inside an argument-taking content closure must own its slots"
    );
    assert_composition_valid(&composition);
}

fn remembered_key_marker(marker: i32) -> i32 {
    remember_branch_marker(marker)
}

#[composable]
fn composing_key_argument_probe(cond: bool) {
    if cond {
        cranpose_core::with_key(&remembered_key_marker(61), || {});
    } else {
        cranpose_core::with_key(&remembered_key_marker(62), || {});
    }
}

#[test]
fn a_composing_key_argument_keeps_the_bracket() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(20, || composing_key_argument_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!(branch_inits(), 1);

    pass(&mut composition, false).expect("switch the branch");
    assert_eq!(
        branch_inits(),
        2,
        "a remember evaluated inside the key argument must not be shared across branches"
    );

    pass(&mut composition, true).expect("switch back");
    assert_eq!(branch_inits(), 3);
    assert_composition_valid(&composition);
}

#[composable]
fn snake_case_closure_branch_probe(cond: bool) {
    content_host(move || {
        if cond {
            stateful_child(7100);
        } else {
            stateful_child(7200);
        }
    });
}

#[test]
fn snake_case_composables_inside_content_closures_get_branch_groups() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(21, || snake_case_closure_branch_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!(branch_seen(), 7100);

    pass(&mut composition, false).expect("switch the branch");
    assert_eq!(
        branch_seen(),
        7200,
        "a snake_case composable in a content closure must still get per-branch state"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn guard_name_collision_probe(cond: bool) {
    if cond {
        let __cranpose_branch_group_guard = 41;
        let value = remember_branch_marker(__cranpose_branch_group_guard);
        BRANCH_SEEN.with(|seen| seen.set(value));
    } else {
        let value = remember_branch_marker(42);
        BRANCH_SEEN.with(|seen| seen.set(value));
    }
}

#[test]
fn a_user_binding_named_like_the_guard_is_not_shadowed() {
    reset_branch_probes();
    let mut composition = test_composition();
    composition
        .render(22, || guard_name_collision_probe(true))
        .expect("compose with the colliding binding");
    assert_eq!(branch_seen(), 41);
    assert_composition_valid(&composition);
}

#[allow(non_snake_case)]
fn PlainCamelHelper(value: i32) -> i32 {
    value
}

thread_local! {
    static STORED_HANDLER: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
}

#[composable]
fn handler_storing_probe(cond: bool) {
    STORED_HANDLER.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || {
            let value = if cond {
                PlainCamelHelper(71)
            } else {
                PlainCamelHelper(72)
            };
            BRANCH_SEEN.with(|seen| seen.set(value));
        }));
    });
    let _anchor = remember(|| 0_i32);
}

#[test]
fn a_misclassified_handler_closure_degrades_to_a_no_op() {
    reset_branch_probes();
    let mut composition = test_composition();
    composition
        .render(14, || handler_storing_probe(true))
        .expect("compose the handler host");

    let mut handler = STORED_HANDLER
        .with(|slot| slot.borrow_mut().take())
        .expect("handler stored");
    handler();
    assert_eq!(branch_seen(), 71);
    handler();
    assert_eq!(branch_seen(), 71, "re-running the handler must stay inert");
    assert_composition_valid(&composition);
}

thread_local! {
    static KEYED_VALUES: RefCell<Vec<(u64, i32)>> = const { RefCell::new(Vec::new()) };
}

#[composable]
fn keyed_visibility_probe(rows: Vec<(u64, bool)>) {
    KEYED_VALUES.with(|values| values.borrow_mut().clear());
    for (id, visible) in rows {
        if visible {
            cranpose_core::with_key(&id, || {
                let value = remember(|| {
                    BRANCH_INITS.with(|count| count.set(count.get() + 1));
                    BRANCH_INITS.with(Cell::get) as i32
                });
                let value = value.with(|value| *value);
                KEYED_VALUES.with(|log| log.borrow_mut().push((id, value)));
            });
        }
    }
}

fn keyed_values() -> Vec<(u64, i32)> {
    KEYED_VALUES.with(|values| values.borrow().clone())
}

#[test]
fn keyed_state_survives_a_front_removal_across_branch_shells() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, rows: Vec<(u64, bool)>| {
        composition.render(17, || keyed_visibility_probe(rows.clone()))
    };

    pass(&mut composition, vec![(1, true), (2, true), (3, true)]).expect("initial composition");
    assert_eq!(branch_inits(), 3);
    assert_eq!(keyed_values(), vec![(1, 1), (2, 2), (3, 3)]);

    pass(&mut composition, vec![(1, false), (2, true), (3, true)]).expect("hide the first row");
    assert_eq!(
        (branch_inits(), keyed_values()),
        (3, vec![(2, 2), (3, 3)]),
        "hiding one row must not recompose the keyed rows behind it"
    );
    assert_composition_valid(&composition);

    pass(&mut composition, vec![(1, true), (2, true), (3, true)])
        .expect("show the first row again");
    assert_eq!(
        (branch_inits(), keyed_values()),
        (4, vec![(1, 4), (2, 2), (3, 3)]),
        "re-showing the first row must compose it fresh and keep the others"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn mixed_keyed_visibility_probe(rows: Vec<(u64, bool)>) {
    KEYED_VALUES.with(|values| values.borrow_mut().clear());
    for (id, visible) in rows {
        if visible {
            let _breadcrumb = remember(|| 0_i32);
            cranpose_core::with_key(&id, || {
                let value = remember(|| {
                    BRANCH_INITS.with(|count| count.set(count.get() + 1));
                    BRANCH_INITS.with(Cell::get) as i32
                });
                let value = value.with(|value| *value);
                KEYED_VALUES.with(|log| log.borrow_mut().push((id, value)));
            });
        }
    }
}

#[test]
fn keyed_state_survives_a_front_removal_across_branch_brackets() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, rows: Vec<(u64, bool)>| {
        composition.render(19, || mixed_keyed_visibility_probe(rows.clone()))
    };

    pass(&mut composition, vec![(1, true), (2, true), (3, true)]).expect("initial composition");
    assert_eq!(branch_inits(), 3);
    assert_eq!(keyed_values(), vec![(1, 1), (2, 2), (3, 3)]);

    pass(&mut composition, vec![(1, false), (2, true), (3, true)]).expect("hide the first row");
    assert_eq!(
        (branch_inits(), keyed_values()),
        (3, vec![(2, 2), (3, 3)]),
        "a bracketed keyed row must be stolen across brackets, not recomposed"
    );
    assert_composition_valid(&composition);

    pass(&mut composition, vec![(1, true), (2, true), (3, true)])
        .expect("show the first row again");
    assert_eq!(
        (branch_inits(), keyed_values()),
        (4, vec![(1, 4), (2, 2), (3, 3)]),
        "re-showing the first row must reclaim the shifted rows from the orphan pool"
    );
    assert_composition_valid(&composition);
}

#[test]
fn keyed_state_follows_a_reorder_across_branch_shells() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: Vec<(u64, bool)>| {
        composition.render(18, || keyed_visibility_probe(value.clone()))
    };

    pass(&mut composition, vec![(1, true), (2, true)]).expect("initial composition");
    assert_eq!(keyed_values(), vec![(1, 1), (2, 2)]);

    pass(&mut composition, vec![(2, true), (1, true)]).expect("swap the rows");
    assert_eq!(
        (branch_inits(), keyed_values()),
        (2, vec![(2, 2), (1, 1)]),
        "a keyed row must carry its state through a reorder across brackets"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn loop_branch_probe(flags: Vec<bool>) {
    BRANCH_LOG.with(|log| log.borrow_mut().clear());
    for flag in flags {
        if flag {
            let value = remember(|| {
                BRANCH_INITS.with(|count| count.set(count.get() + 1));
                BRANCH_INITS.with(Cell::get) as i32
            });
            BRANCH_LOG.with(|log| log.borrow_mut().push(format!("t{}", value.with(|v| *v))));
        } else {
            let value = remember(|| {
                BRANCH_INITS.with(|count| count.set(count.get() + 1));
                BRANCH_INITS.with(Cell::get) as i32
            });
            BRANCH_LOG.with(|log| log.borrow_mut().push(format!("e{}", value.with(|v| *v))));
        }
    }
}

#[test]
fn branches_inside_a_loop_key_per_occurrence() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: Vec<bool>| {
        composition.render(12, || loop_branch_probe(value.clone()))
    };

    pass(&mut composition, vec![true, true]).expect("compose two then-branches");
    assert_eq!(branch_log(), vec!["t1".to_string(), "t2".to_string()]);

    pass(&mut composition, vec![true, false]).expect("flip the second iteration");
    assert_eq!(
        branch_log(),
        vec!["t1".to_string(), "e3".to_string()],
        "the flipped iteration must compose a fresh else slot"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn method_call_branch_probe(cond: bool) {
    let composer = with_current_composer(Clone::clone);
    if cond {
        let value = composer.remember(|| {
            BRANCH_INITS.with(|count| count.set(count.get() + 1));
            1
        });
        BRANCH_SEEN.with(|seen| seen.set(value.with(|value| *value)));
    } else {
        let value = composer.remember(|| {
            BRANCH_INITS.with(|count| count.set(count.get() + 1));
            2
        });
        BRANCH_SEEN.with(|seen| seen.set(value.with(|value| *value)));
    }
}

#[test]
fn branches_composing_only_through_composer_methods_own_their_state() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(30, || method_call_branch_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    pass(&mut composition, false).expect("switch to the else branch");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 2),
        "a branch composing via `composer.remember` must not inherit the other branch's slot"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn guard_composition_probe(route: u8) {
    let matched = match route {
        0 if remember_branch_marker(10) == 10 => 1,
        1 if remember_branch_marker(20) == 20 => 2,
        _ => 3,
    };
    BRANCH_SEEN.with(|seen| seen.set(matched));
}

#[test]
fn match_guards_own_their_composition_slots() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: u8| {
        composition.render(31, || guard_composition_probe(value))
    };

    pass(&mut composition, 0).expect("compose the first guard");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    pass(&mut composition, 1).expect("switch to the second guard");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 2),
        "the second guard must not be handed the first guard's remember slot"
    );

    pass(&mut composition, 0).expect("switch back to the first guard");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (3, 1),
        "returning to a departed guard must compose fresh state"
    );
    assert_composition_valid(&composition);
}

mod fake_arity_with_key {
    use super::*;

    fn with_key(_key: &i32, marker: i32, content: impl FnOnce(i32)) {
        content(marker);
    }

    #[composable]
    pub(super) fn probe(cond: bool) {
        if cond {
            with_key(&1, 7, |marker| {
                let value = remember(|| {
                    BRANCH_INITS.with(|count| count.set(count.get() + 1));
                    marker
                });
                BRANCH_SEEN.with(|seen| seen.set(value.with(|value| *value)));
            });
        } else {
            with_key(&2, 8, |marker| {
                let value = remember(|| {
                    BRANCH_INITS.with(|count| count.set(count.get() + 1));
                    marker
                });
                BRANCH_SEEN.with(|seen| seen.set(value.with(|value| *value)));
            });
        }
    }
}

#[test]
fn a_lookalike_with_key_of_different_arity_keeps_the_bracket() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(32, || fake_arity_with_key::probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 7));

    pass(&mut composition, false).expect("switch to the else branch");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 8),
        "a three-argument lookalike opens no keyed group, so the branch bracket must stay"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn local_fn_branch_probe(cond: bool) {
    fn local(cond: bool) {
        if cond {
            let value = remember_branch_marker(1);
            BRANCH_SEEN.with(|seen| seen.set(value));
        } else {
            let value = remember_branch_marker(2);
            BRANCH_SEEN.with(|seen| seen.set(value));
        }
    }
    local(cond);
}

#[test]
fn conditionals_inside_local_functions_own_their_branches() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(33, || local_fn_branch_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    pass(&mut composition, false).expect("switch to the else branch");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 2),
        "a local fn runs under the current composer; its branches need groups too"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn method_keyed_arg_probe(cond: bool) {
    let composer = with_current_composer(Clone::clone);
    if cond {
        with_key(
            &composer
                .remember(|| {
                    BRANCH_INITS.with(|count| count.set(count.get() + 1));
                    1_i32
                })
                .with(|value| *value),
            || {
                let value = remember_branch_marker(101);
                BRANCH_SEEN.with(|seen| seen.set(value));
            },
        );
    } else {
        with_key(
            &composer
                .remember(|| {
                    BRANCH_INITS.with(|count| count.set(count.get() + 1));
                    2_i32
                })
                .with(|value| *value),
            || {
                let value = remember_branch_marker(202);
                BRANCH_SEEN.with(|seen| seen.set(value));
            },
        );
    }
}

#[test]
fn a_key_argument_composing_through_methods_keeps_the_bracket() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(34, || method_keyed_arg_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (2, 101));

    pass(&mut composition, false).expect("switch to the else branch");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (4, 202),
        "the key remember composes; without the bracket the departed branch's slot leaks into it"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn closure_through_method_probe(cond: bool) {
    let occupied = Some(());
    let shown = if cond {
        occupied.map(|()| {
            let value = remember_branch_marker(1);
            BRANCH_SEEN.with(|seen| seen.set(value));
            value
        })
    } else {
        occupied.map(|()| {
            let value = remember_branch_marker(2);
            BRANCH_SEEN.with(|seen| seen.set(value));
            value
        })
    };
    let _ = shown;
}

#[test]
fn branches_composing_through_method_call_closures_own_their_state() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(35, || closure_through_method_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    pass(&mut composition, false).expect("switch to the else branch");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 2),
        "`Option::map` runs its closure during composition; the branch needs its bracket"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn composing_condition_probe(enabled: bool) {
    if enabled && remember_branch_marker(1) == 1 {
        BRANCH_LOG.with(|log| log.borrow_mut().push("on".to_string()));
    }
    let after = remember(|| {
        BRANCH_INITS.with(|count| count.set(count.get() + 1));
        99
    });
    BRANCH_SEEN.with(|seen| seen.set(after.with(|value| *value)));
}

#[test]
fn a_composing_if_condition_owns_its_slots() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(36, || composing_condition_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (2, 99));

    pass(&mut composition, false).expect("short-circuit the condition");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 99),
        "skipping the condition's remember must not shift the slot after the `if`"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn impl_method_branch_probe(cond: bool) {
    struct Helper;
    impl Helper {
        fn render(&self, cond: bool) {
            if cond {
                let value = remember_branch_marker(1);
                BRANCH_SEEN.with(|seen| seen.set(value));
            } else {
                let value = remember_branch_marker(2);
                BRANCH_SEEN.with(|seen| seen.set(value));
            }
        }
    }
    Helper.render(cond);
}

#[test]
fn conditionals_inside_local_impl_methods_own_their_branches() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(37, || impl_method_branch_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    pass(&mut composition, false).expect("switch to the else branch");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 2),
        "a local impl method runs under the current composer; its branches need groups"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn composer_shadowing_probe(cond: bool) {
    let __composer = 7_u8;
    if cond {
        let value = remember_branch_marker(i32::from(__composer));
        BRANCH_SEEN.with(|seen| seen.set(value));
    } else {
        let value = remember_branch_marker(i32::from(__composer) + 1);
        BRANCH_SEEN.with(|seen| seen.set(value));
    }
}

#[test]
fn a_user_binding_named_like_the_composer_is_not_broken_by_guards() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(38, || composer_shadowing_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 7));

    pass(&mut composition, false).expect("switch to the else branch");
    assert_eq!((branch_inits(), branch_seen()), (2, 8));
    assert_composition_valid(&composition);
}

mod fake_shape_with_key {
    use super::*;

    fn with_key<K>(_key: &K, content: impl FnOnce()) {
        content();
    }

    #[composable]
    pub(super) fn probe(cond: bool) {
        if cond {
            with_key(&1, || {
                let value = remember_branch_marker(1);
                BRANCH_SEEN.with(|seen| seen.set(value));
            });
        } else {
            with_key(&2, || {
                let value = remember_branch_marker(2);
                BRANCH_SEEN.with(|seen| seen.set(value));
            });
        }
    }
}

#[test]
fn a_lookalike_with_key_of_the_real_shape_still_gets_a_bracket() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(39, || fake_shape_with_key::probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    pass(&mut composition, false).expect("switch to the else branch");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 2),
        "a lookalike opens no keyed group, so its positional slots must materialize the bracket"
    );
    assert_composition_valid(&composition);
}

fn keyed_helper(marker: i32) {
    with_key(&"shared", || {
        let value = remember_branch_marker(marker);
        BRANCH_SEEN.with(|seen| seen.set(value));
    });
}

#[composable]
fn keyed_via_helper_probe(cond: bool) {
    if cond {
        keyed_helper(1);
    } else {
        keyed_helper(2);
    }
}

#[test]
fn an_explicit_key_does_not_carry_state_across_branch_sites() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(40, || keyed_via_helper_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    pass(&mut composition, false).expect("switch to the else branch");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 2),
        "Compose parity: a plain key moves within its branch site (list reorders), \
         it does not carry state across a branch switch"
    );
    assert_composition_valid(&composition);
}

fn stateful_label(marker: i32) -> i32 {
    remember_branch_marker(marker)
}

#[composable]
fn value_macro_snake_probe(cond: bool) {
    let label = if cond {
        format!("{}", stateful_label(1))
    } else {
        format!("{}", stateful_label(2))
    };
    BRANCH_SEEN.with(|seen| seen.set(label.parse().unwrap_or(-1)));
}

#[test]
fn a_snake_case_composable_inside_a_value_macro_gets_a_branch_group() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(41, || value_macro_snake_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    pass(&mut composition, false).expect("switch to the else branch");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 2),
        "a snake_case composable inside `format!` still composes; the branch needs its bracket"
    );
    assert_composition_valid(&composition);
}

fn keyed_pool_marker(key: i32) {
    with_key(&key, || {
        let value = remember_branch_marker(key);
        BRANCH_SEEN.with(|seen| seen.set(value));
    });
}

#[composable]
fn cross_branch_pool_probe(first_key: i32, second: bool) {
    if first_key != 0 {
        let _anchor = remember(|| 0_i32);
        keyed_pool_marker(first_key);
    }
    if second {
        let _anchor = remember(|| 0_i32);
        keyed_pool_marker(1);
    }
}

#[test]
fn a_parked_keyed_subtree_stays_within_its_branch_site() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, (arg0, arg1): (i32, bool)| {
        composition.render(42, || cross_branch_pool_probe(arg0, arg1))
    };

    pass(&mut composition, (1, false)).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    pass(&mut composition, (2, true)).expect("re-key the first branch and enable the second");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (3, 1),
        "a subtree parked by one branch site must not be claimed by another"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn method_state_branch_probe(cond: bool) {
    let composer = with_current_composer(Clone::clone);
    if cond {
        let state = composer.use_state(|| {
            BRANCH_INITS.with(|count| count.set(count.get() + 1));
            1
        });
        BRANCH_SEEN.with(|seen| seen.set(state.value()));
    } else {
        let state = composer.use_state(|| {
            BRANCH_INITS.with(|count| count.set(count.get() + 1));
            2
        });
        BRANCH_SEEN.with(|seen| seen.set(state.value()));
    }
}

#[test]
fn branches_composing_through_use_state_own_their_state() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(43, || method_state_branch_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    pass(&mut composition, false).expect("switch to the else branch");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 2),
        "`composer.use_state` writes a slot; the branch needs its bracket"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn value_macro_method_probe(cond: bool) {
    let composer = with_current_composer(Clone::clone);
    let label = if cond {
        format!(
            "{}",
            composer
                .remember(|| {
                    BRANCH_INITS.with(|count| count.set(count.get() + 1));
                    1_i32
                })
                .with(|value| *value)
        )
    } else {
        format!(
            "{}",
            composer
                .remember(|| {
                    BRANCH_INITS.with(|count| count.set(count.get() + 1));
                    2_i32
                })
                .with(|value| *value)
        )
    };
    BRANCH_SEEN.with(|seen| seen.set(label.parse().unwrap_or(-1)));
}

#[test]
fn a_composing_method_inside_a_value_macro_gets_a_branch_group() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(44, || value_macro_method_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    pass(&mut composition, false).expect("switch to the else branch");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 2),
        "`composer.remember` inside `format!` still composes; the branch needs its bracket"
    );
    assert_composition_valid(&composition);
}

mod deferred_shell_ordinals {
    use super::*;

    fn with_key<K>(_key: &K, content: impl FnOnce()) {
        content();
    }

    #[composable]
    pub(super) fn probe(cond: bool) {
        stateful_child(0);
        if cond {
            with_key(&1, || stateful_child(1));
        } else {
            with_key(&2, || stateful_child(2));
        }
    }
}

#[test]
fn a_materialized_shell_keeps_group_ordinals_consistent() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(45, || deferred_shell_ordinals::probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!(branch_seen(), 1);

    pass(&mut composition, false).expect("switch to the else branch");
    assert_eq!(
        branch_seen(),
        2,
        "the shell-materialized branch child must own fresh state"
    );
    assert_composition_valid(&composition);
}

mod deferred_shell_nodes {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        pub(super) static NODE_BUILDS: Cell<usize> = const { Cell::new(0) };
    }

    fn with_key<K>(_key: &K, content: impl FnOnce()) {
        content();
    }

    #[composable(no_skip)]
    pub(super) fn probe() {
        let composer = with_current_composer(Clone::clone);
        if NODE_BUILDS.with(Cell::get) < usize::MAX {
            with_key(&1, || {
                composer.emit_node(|| {
                    NODE_BUILDS.with(|count| count.set(count.get() + 1));
                    TestDummyNode
                });
            });
        }
    }
}

#[test]
fn a_materialized_shell_reuses_its_node_across_passes() {
    deferred_shell_nodes::NODE_BUILDS.with(|count| count.set(0));
    let mut composition = test_composition();

    composition
        .render(46, deferred_shell_nodes::probe)
        .expect("initial composition");
    assert_eq!(
        deferred_shell_nodes::NODE_BUILDS.with(std::cell::Cell::get),
        1
    );

    composition
        .render(46, deferred_shell_nodes::probe)
        .expect("recompose the same shape");
    assert_eq!(
        deferred_shell_nodes::NODE_BUILDS.with(std::cell::Cell::get),
        1,
        "the node inside a materialized shell must be reused, not rebuilt every pass"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn nested_bracket_rows(rows: Vec<i32>) {
    for row in rows {
        if row != 0 {
            let _crumb = remember(|| 0_i32);
            if row > 0 {
                let _inner_crumb = remember(|| 0_i32);
                keyed_pool_marker(row);
            }
        }
    }
}

#[test]
fn keyed_state_survives_a_front_removal_through_nested_brackets() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: Vec<i32>| {
        composition.render(47, || nested_bracket_rows(value.clone()))
    };

    pass(&mut composition, vec![1, 2]).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (2, 2));

    pass(&mut composition, vec![2]).expect("remove the front row");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 2),
        "row 2's keyed state sits two brackets deep; the steal must ascend the branch path"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn value_macro_paren_callee_probe(cond: bool) {
    let label = if cond {
        format!("{}", (stateful_label)(1))
    } else {
        format!("{}", (stateful_label)(2))
    };
    BRANCH_SEEN.with(|seen| seen.set(label.parse().unwrap_or(-1)));
}

#[test]
fn a_parenthesized_callee_inside_a_value_macro_gets_a_branch_group() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(48, || value_macro_paren_callee_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    pass(&mut composition, false).expect("switch to the else branch");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 2),
        "a parenthesized callee still composes; the branch needs its bracket"
    );
    assert_composition_valid(&composition);
}

#[test]
fn a_key_flattened_out_of_a_skipped_inner_bracket_keeps_its_path() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: Vec<i32>| {
        composition.render(50, || nested_bracket_rows(value.clone()))
    };

    pass(&mut composition, vec![1, 2]).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (2, 2));

    pass(&mut composition, vec![-1, 1]).expect("skip the inner branch of the first occurrence");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 1),
        "a key parked out of a nested bracket must be claimable under its full path"
    );
    assert_composition_valid(&composition);
}

fn retained_marker(marker: i32) {
    with_current_composer(|composer| {
        let key = location_key(file!(), line!(), column!());
        composer.cranpose_with_reuse(key, RecomposeOptions::default(), |composer| {
            let value = composer.remember(|| {
                BRANCH_INITS.with(|count| count.set(count.get() + 1));
                marker
            });
            BRANCH_SEEN.with(|seen| seen.set(value.with(|value| *value)));
        });
    });
}

#[composable]
fn dual_retention_probe(first: bool, second: bool) {
    if first {
        retained_marker(1);
    }
    if second {
        retained_marker(2);
    }
}

#[test]
fn both_branches_retain_the_same_reuse_child_without_colliding() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, (first, second)| {
        composition.render(51, || dual_retention_probe(first, second))
    };

    pass(&mut composition, (true, true)).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (2, 2));

    pass(&mut composition, (false, false)).expect("retain both children");

    pass(&mut composition, (true, true)).expect("restore both children");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 2),
        "each branch site's retained child must come back; the second must not be \
         rejected as a duplicate of the first"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn duplicate_keyed_rows(rows: Vec<i32>) {
    for row in rows {
        if row != 0 {
            let _crumb = remember(|| 0_i32);
            keyed_pool_marker(1);
        }
    }
}

#[test]
#[should_panic(expected = "duplicate sibling group key")]
fn duplicate_explicit_keys_across_brackets_of_one_site_panic_loudly() {
    reset_branch_probes();
    let mut composition = test_composition();

    let _ = composition.render(52, || duplicate_keyed_rows(vec![1, 2]));
}

fn branch_labels() -> Vec<String> {
    vec!["row".to_string()]
}

#[composable]
fn scrutinee_borrow_probe() {
    if let Some(label) = branch_labels().first() {
        let value = remember_branch_marker(label.len() as i32);
        BRANCH_SEEN.with(|seen| seen.set(value));
    }
}

#[test]
fn an_if_let_scrutinee_keeps_its_temporaries_borrowable() {
    reset_branch_probes();
    let mut composition = test_composition();

    composition
        .render(53, scrutinee_borrow_probe)
        .expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 3));
    assert_composition_valid(&composition);
}

#[composable]
fn looped_retention_probe(rows: Vec<bool>) {
    for on in rows {
        if on {
            retained_marker(9);
        }
    }
}

#[test]
fn repeated_occurrences_of_one_branch_site_retain_independently() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: Vec<bool>| {
        composition.render(54, || looped_retention_probe(value.clone()))
    };

    pass(&mut composition, vec![true, true]).expect("initial composition");
    assert_eq!(branch_inits(), 2);

    pass(&mut composition, vec![false, false]).expect("retain both occurrences");

    pass(&mut composition, vec![true, true]).expect("restore both occurrences");
    assert_eq!(
        branch_inits(),
        2,
        "two occurrences of one site retain two identities; neither is a duplicate"
    );
    assert_composition_valid(&composition);
}

mod delegating_with_key {
    use super::*;

    fn with_key<K: std::hash::Hash>(key: &K, content: impl FnOnce()) {
        cranpose_core::with_key(key, content);
    }

    #[composable]
    pub(super) fn probe(cond: bool) {
        if cond {
            with_key(&"dup", || {
                let value = remember_branch_marker(1);
                BRANCH_SEEN.with(|seen| seen.set(value));
            });
        } else {
            with_key(&"dup", || {
                let value = remember_branch_marker(2);
                BRANCH_SEEN.with(|seen| seen.set(value));
            });
        }
    }
}

#[test]
fn a_delegating_with_key_lookalike_does_not_merge_branch_identity() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(55, || delegating_with_key::probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    pass(&mut composition, false).expect("switch to the else branch");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 2),
        "two branches funneling one key through a wrapper are still two identities"
    );
    assert_composition_valid(&composition);
}

fn remember_opt() -> Option<i32> {
    Some(remember_branch_marker(5))
}

#[composable]
fn composing_scrutinee_probe() {
    if let Some(value) = remember_opt().filter(|value| *value > 0) {
        BRANCH_SEEN.with(|seen| seen.set(value));
    }
}

#[test]
fn a_composing_if_let_scrutinee_composes_inside_its_own_group() {
    reset_branch_probes();
    let mut composition = test_composition();

    composition
        .render(56, composing_scrutinee_probe)
        .expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 5));

    composition
        .render(56, composing_scrutinee_probe)
        .expect("recompose");
    assert_eq!((branch_inits(), branch_seen()), (1, 5));
    assert_composition_valid(&composition);
}

fn index_helper() -> usize {
    0
}

#[composable]
fn place_scrutinee_probe() {
    let values: Vec<Option<String>> = vec![Some("row".to_string())];
    if let Some(ref value) = values[index_helper()] {
        let marker = remember_branch_marker(value.len() as i32);
        BRANCH_SEEN.with(|seen| seen.set(marker));
    }
}

#[test]
fn a_place_scrutinee_with_a_ref_binding_still_compiles() {
    reset_branch_probes();
    let mut composition = test_composition();

    composition
        .render(57, place_scrutinee_probe)
        .expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 3));
    assert_composition_valid(&composition);
}

#[composable]
fn closure_select_probe(cond: bool) {
    let render: fn() = if cond {
        || {
            stateful_child(301);
        }
    } else {
        || {
            stateful_child(302);
        }
    };
    render();
}

#[test]
fn a_branch_selected_closure_carries_its_branch_identity() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(58, || closure_select_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!(branch_seen(), 301);

    pass(&mut composition, false).expect("switch to the else branch");
    assert_eq!(
        branch_seen(),
        302,
        "a branch-selected closure invoked later must not inherit the other branch's state"
    );
    assert_composition_valid(&composition);
}

struct MethodHelper;

impl MethodHelper {
    fn render(&self, marker: i32) {
        stateful_child(marker);
    }
}

#[composable]
fn transitive_method_probe(cond: bool) {
    if cond {
        MethodHelper.render(401);
    } else {
        MethodHelper.render(402);
    }
}

#[test]
fn branches_composing_through_arbitrary_methods_own_their_state() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(59, || transitive_method_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!(branch_seen(), 401);

    pass(&mut composition, false).expect("switch to the else branch");
    assert_eq!(
        branch_seen(),
        402,
        "a branch composing through an arbitrary method must own its state"
    );
    assert_composition_valid(&composition);
}

fn stateful_index() -> usize {
    let value = remember(|| {
        BRANCH_INITS.with(|count| count.set(count.get() + 1));
        0_usize
    });
    value.with(|value| *value)
}

#[composable]
fn place_index_probe() {
    let values: Vec<Option<String>> = vec![Some("row".to_string())];
    if let Some(ref value) = values[stateful_index()] {
        BRANCH_SEEN.with(|seen| seen.set(value.len() as i32));
    }
}

#[test]
fn a_composing_index_inside_a_place_scrutinee_gets_its_own_group() {
    reset_branch_probes();
    let mut composition = test_composition();

    composition
        .render(60, place_index_probe)
        .expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 3));

    composition
        .render(60, place_index_probe)
        .expect("recompose");
    assert_eq!((branch_inits(), branch_seen()), (1, 3));
    assert_composition_valid(&composition);
}

fn nested_keyed_same_line(marker: i32) {
    if marker != 0 {
        with_key(&"same", || {
            let value = remember_branch_marker(marker);
            BRANCH_SEEN.with(|seen| seen.set(value));
        });
    }
}

#[composable]
fn nested_shell_provenance_probe(cond: bool) {
    if cond {
        nested_keyed_same_line(501);
    } else {
        nested_keyed_same_line(502);
    }
}

#[test]
fn nested_pending_shells_fold_into_keyed_provenance() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(61, || nested_shell_provenance_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!(branch_seen(), 501);

    pass(&mut composition, false).expect("switch to the else branch");
    assert_eq!(
        branch_seen(),
        502,
        "a keyed group under nested shells belongs to the full branch path"
    );
    assert_composition_valid(&composition);
}

fn boxed_render(render: impl Fn() + 'static) -> Box<dyn Fn()> {
    Box::new(render)
}

#[composable]
fn boxed_closure_probe(cond: bool) {
    let render: Box<dyn Fn()> = if cond {
        boxed_render(|| {
            stateful_child(601);
        })
    } else {
        boxed_render(|| {
            stateful_child(602);
        })
    };
    render();
}

#[test]
fn a_branch_tail_closure_through_a_helper_keeps_branch_identity() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(62, || boxed_closure_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!(branch_seen(), 601);

    pass(&mut composition, false).expect("switch to the else branch");
    assert_eq!(
        branch_seen(),
        602,
        "a closure escaping through the branch's value must keep its branch identity"
    );
    assert_composition_valid(&composition);
}

struct StatefulHolder {
    item: Option<String>,
}

fn stateful_holder() -> StatefulHolder {
    let value = remember(|| {
        BRANCH_INITS.with(|count| count.set(count.get() + 1));
        "row".to_string()
    });
    StatefulHolder {
        item: Some(value.with(std::clone::Clone::clone)),
    }
}

#[composable]
fn field_base_scrutinee_probe() {
    if let Some(ref value) = stateful_holder().item {
        BRANCH_SEEN.with(|seen| seen.set(value.len() as i32));
    }
}

#[test]
fn a_composing_field_base_inside_a_place_scrutinee_gets_its_own_group() {
    reset_branch_probes();
    let mut composition = test_composition();

    composition
        .render(63, field_base_scrutinee_probe)
        .expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 3));

    composition
        .render(63, field_base_scrutinee_probe)
        .expect("recompose");
    assert_eq!((branch_inits(), branch_seen()), (1, 3));
    assert_composition_valid(&composition);
}

mod shadowed_format {
    use super::*;

    macro_rules! format {
        ($marker:literal) => {
            remember_branch_marker($marker)
        };
    }

    #[composable]
    pub(super) fn probe(cond: bool) {
        let value = if cond { format!(701) } else { format!(702) };
        BRANCH_SEEN.with(|seen| seen.set(value));
    }
}

#[test]
fn a_shadowed_value_macro_still_brackets_its_branch() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(64, || shadowed_format::probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 701));

    pass(&mut composition, false).expect("switch to the else branch");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 702),
        "a local macro named like a std value macro can compose; the branch needs its shell"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn nested_shell_probe(first: bool) {
    fn shared_nested_branch_helper(marker: i32) {
        if marker > 0 {
            let value = remember_branch_marker(marker);
            BRANCH_SEEN.with(|seen| seen.set(value));
        }
    }
    if first {
        shared_nested_branch_helper(1);
    } else {
        shared_nested_branch_helper(2);
    }
}

#[test]
fn a_helper_branch_called_from_both_arms_keeps_the_outer_identity() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(65, || nested_shell_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    pass(&mut composition, false).expect("switch to the else arm");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 2),
        "the helper's inner branch must nest inside the outer arm's shell, not replace it"
    );

    pass(&mut composition, true).expect("switch back to the then arm");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (3, 1),
        "returning to a departed arm must compose the helper fresh"
    );
    assert_composition_valid(&composition);
}

fn keyed_shell_helper(marker: i32) {
    cranpose_core::with_key(&"same", || {
        let value = remember_branch_marker(marker);
        BRANCH_SEEN.with(|seen| seen.set(value));
    });
}

#[composable]
fn keyed_provenance_probe(flag: bool) {
    if flag {
        keyed_shell_helper(1);
        return;
    }
    keyed_shell_helper(2);
}

#[test]
fn a_keyed_group_does_not_cross_between_branch_and_tail_occurrences() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(66, || keyed_provenance_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    pass(&mut composition, false).expect("switch to the tail occurrence");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 2),
        "the unbracketed tail call must not adopt the branch occurrence's keyed state"
    );

    pass(&mut composition, true).expect("switch back to the branch occurrence");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (3, 1),
        "the branch occurrence must not adopt the tail occurrence's keyed state"
    );
    assert_composition_valid(&composition);
}

struct BranchAddend {
    marker: i32,
}

impl std::ops::Add for BranchAddend {
    type Output = i32;

    fn add(self, rhs: Self) -> i32 {
        remember_branch_marker(self.marker + rhs.marker)
    }
}

#[composable]
fn operator_branch_probe(first: bool) {
    let value = if first {
        BranchAddend { marker: 1 } + BranchAddend { marker: 0 }
    } else {
        BranchAddend { marker: 2 } + BranchAddend { marker: 0 }
    };
    BRANCH_SEEN.with(|seen| seen.set(value));
}

#[test]
fn an_operator_that_composes_still_gets_branch_shells() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(67, || operator_branch_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    pass(&mut composition, false).expect("switch to the else arm");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 2),
        "an arm whose only call is an operator impl must still own its shell"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn unkeyed_neighbor_probe(inner: bool) {
    if true {
        if inner {
            let _ = remember(|| 0_i32);
        }
        cranpose_core::with_key(&7, || {
            let value = remember_branch_marker(70);
            BRANCH_SEEN.with(|seen| seen.set(value));
        });
    }
}

#[test]
fn a_keyed_identity_survives_an_unkeyed_neighbor_toggling() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(68, || unkeyed_neighbor_probe(value))
    };

    pass(&mut composition, false).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 70));

    pass(&mut composition, true).expect("the neighbor branch materializes the bracket");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (1, 70),
        "a neighbor's remember must not move the keyed subtree's identity"
    );

    pass(&mut composition, false).expect("the neighbor branch leaves again");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (1, 70),
        "the keyed subtree must keep its identity when the bracket empties"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn deref_place_probe() {
    let current = Box::new(Some(3_i32));
    if let Some(ref value) = *current {
        let seen = remember_branch_marker(*value);
        BRANCH_SEEN.with(|seen_cell| seen_cell.set(seen));
    }
    let _again = &current;
}

#[test]
fn a_deref_place_scrutinee_keeps_its_binding_usable() {
    reset_branch_probes();
    let mut composition = test_composition();

    composition
        .render(69, deref_place_probe)
        .expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 3));
    assert_composition_valid(&composition);
}

#[composable]
fn escaped_closure_probe(cond: bool) {
    let render: Box<dyn Fn()> = if cond {
        boxed_render(|| {
            let value = remember_branch_marker(81);
            BRANCH_SEEN.with(|seen| seen.set(value));
        })
    } else {
        boxed_render(|| {
            let value = remember_branch_marker(82);
            BRANCH_SEEN.with(|seen| seen.set(value));
        })
    };
    render();
}

#[test]
fn a_branch_selected_boxed_closure_keeps_its_branch_identity() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(70, || escaped_closure_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 81));

    pass(&mut composition, false).expect("switch to the else arm's closure");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 82),
        "an escaped branch closure must not adopt the other arm's remember slot"
    );
    assert_composition_valid(&composition);
}

macro_rules! routed_branch {
    ($cond:expr) => {
        if $cond {
            let value = remember_branch_marker(91);
            BRANCH_SEEN.with(|seen| seen.set(value));
        } else {
            let value = remember_branch_marker(92);
            BRANCH_SEEN.with(|seen| seen.set(value));
        }
    };
}

#[composable]
fn macro_rules_probe(cond: bool) {
    routed_branch!(cond);
}

#[test]
fn a_macro_rules_conditional_shares_slots_by_construction() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(71, || macro_rules_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 91));

    pass(&mut composition, false).expect("switch arms inside the macro expansion");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (1, 91),
        "arms born from macro_rules expansion share one slot: the attribute \
         macro runs before function-like macros expand, and caller locations \
         inside an expansion collapse to the invocation site, so neither \
         brackets nor slot stamps can tell the arms apart; key such content \
         explicitly with with_key"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn cross_branch_invoked_closure_probe(use_first: bool) {
    let render = boxed_render(|| {
        let value = remember_branch_marker(101);
        BRANCH_SEEN.with(|seen| seen.set(value));
    });
    if use_first {
        render();
    } else {
        render();
    }
}

#[test]
fn a_stored_closure_composes_fresh_per_invoking_branch() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(72, || cross_branch_invoked_closure_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 101));

    pass(&mut composition, false).expect("invoke the same closure from the other branch");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 101),
        "one closure invoked from two branches is two composition identities"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn twice_invoked_closure_probe() {
    let render = boxed_render(|| {
        let value = remember_branch_marker(111);
        BRANCH_SEEN.with(|seen| seen.set(value));
    });
    if true {
        render();
        render();
    }
}

#[test]
fn a_stored_closure_invoked_twice_owns_two_slots() {
    reset_branch_probes();
    let mut composition = test_composition();

    composition
        .render(73, twice_invoked_closure_probe)
        .expect("initial composition");
    assert_eq!(branch_inits(), 2);

    composition
        .render(73, twice_invoked_closure_probe)
        .expect("recompose");
    assert_eq!(
        branch_inits(),
        2,
        "each invocation keeps its own slot across passes"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn folded_duplicate_probe() {
    for _ in 0..2 {
        if true {
            cranpose_core::with_key(&5, || {
                let _ = remember_branch_marker(5);
            });
        }
    }
}

#[test]
#[should_panic(expected = "duplicate sibling group key")]
fn duplicate_keys_in_folded_brackets_panic_loudly() {
    reset_branch_probes();
    let mut composition = test_composition();
    let _ = composition.render(74, folded_duplicate_probe);
}

#[composable]
fn folded_retention_probe(visible: bool) {
    if visible {
        cranpose_core::with_key(&9, || retained_marker(9));
    }
}

#[composable]
fn materialized_retention_probe(visible: bool) {
    if visible {
        let _ = remember(|| 0_i32);
        cranpose_core::with_key(&9, || retained_marker(9));
    }
}

#[test]
fn keyed_wrapper_retention_behaves_identically_in_both_shell_classes() {
    reset_branch_probes();
    let mut folded = test_composition();
    for visible in [true, false, true] {
        folded
            .render(75, || folded_retention_probe(visible))
            .expect("folded pass");
    }
    let folded_inits = branch_inits();
    assert_composition_valid(&folded);

    reset_branch_probes();
    let mut materialized = test_composition();
    for visible in [true, false, true] {
        materialized
            .render(77, || materialized_retention_probe(visible))
            .expect("materialized pass");
    }
    assert_eq!(
        (folded_inits, branch_inits()),
        (2, 2),
        "a reuse scope inside a keyed wrapper recomposes fresh when the wrapper \
         leaves - the same pre-existing boundary origin/main has - and the fold \
         and materialize classes must agree on it"
    );
    assert_composition_valid(&materialized);
}

#[composable]
fn folded_recompose_probe(bump: i32) {
    if true {
        cranpose_core::with_key(&3, || {
            let state = rememberMutableStateOf(|| 0_i32);
            if bump > state.value() {
                state.set_value(bump);
            }
            BRANCH_SEEN.with(|seen| seen.set(state.value()));
            let _ = remember(|| {
                BRANCH_INITS.with(|count| count.set(count.get() + 1));
            });
        });
    }
}

#[test]
fn invalidation_inside_a_folded_keyed_bracket_recomposes_in_place() {
    reset_branch_probes();
    let mut composition = test_composition();

    composition
        .render(76, || folded_recompose_probe(1))
        .expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    composition
        .process_invalid_scopes()
        .expect("state write recomposes the keyed content in place");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (1, 1),
        "targeted recomposition must reuse the folded keyed group, not remake it"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn located_composable_macro_probe(cond: bool) {
    macro_rules! shared_child_route {
        ($cond:expr) => {
            if $cond {
                stateful_child(201);
            } else {
                stateful_child(202);
            }
        };
    }
    shared_child_route!(cond);
}

#[test]
fn a_macro_rules_conditional_collapses_composable_caller_identity() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(79, || located_composable_macro_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!(branch_seen(), 201);

    pass(&mut composition, false).expect("switch arms inside the macro expansion");
    assert_eq!(
        branch_seen(),
        201,
        "a composable called from both expanded arms is one collapsed identity: \
         track_caller locations inside a macro_rules expansion resolve to the \
         invocation site, the same boundary the arms-share pin documents"
    );
    assert_composition_valid(&composition);
}

struct ComposingHolder {
    value: Option<i32>,
}

impl std::ops::Deref for ComposingHolder {
    type Target = Option<i32>;

    fn deref(&self) -> &Option<i32> {
        let _ = remember(|| 0_i32);
        &self.value
    }
}

#[composable]
fn deref_composing_probe(enabled: bool) {
    let holder = ComposingHolder { value: Some(4) };
    if enabled {
        if let Some(value) = *holder {
            BRANCH_LOG.with(|log| log.borrow_mut().push(format!("deref {value}")));
        }
    }
    let tail = remember(|| {
        BRANCH_INITS.with(|count| count.set(count.get() + 1));
        7_i32
    });
    BRANCH_SEEN.with(|seen| seen.set(tail.with(|value| *value)));
}

#[test]
fn a_composing_deref_slot_is_not_adopted_by_a_following_remember() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, value: bool| {
        composition.render(80, || deref_composing_probe(value))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 7));

    pass(&mut composition, false).expect("the composing deref stops running");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (1, 7),
        "the tail remember must keep its own slot, not inherit the deref's"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn loop_boundary_probe(count: usize) {
    for _ in 0..count {
        let value = remember_branch_marker(31);
        BRANCH_SEEN.with(|seen| seen.set(value));
    }
    let tail = remember_branch_marker(32);
    BRANCH_LOG.with(|log| log.borrow_mut().push(format!("tail {tail}")));
}

#[test]
fn a_loop_body_slot_is_not_adopted_by_the_call_after_the_loop() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, count: usize| {
        composition.render(82, || loop_boundary_probe(count))
    };

    pass(&mut composition, 1).expect("initial composition");
    assert_eq!(branch_inits(), 2);
    assert_eq!(branch_log(), vec!["tail 32"]);

    pass(&mut composition, 0).expect("the loop empties");
    assert_eq!(
        (branch_inits(), branch_log().last().cloned()),
        (2, Some("tail 32".to_string())),
        "the tail call must keep its own slot when the loop body vanishes"
    );
    assert_composition_valid(&composition);
}

mod raw_node_arms {
    use super::*;

    thread_local! {
        pub(super) static NODE_LABELS: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    }

    pub(super) struct LabeledNode;

    impl crate::Node for LabeledNode {}

    #[composable(no_skip)]
    pub(super) fn probe(first: bool) {
        let composer = with_current_composer(Clone::clone);
        if first {
            composer.emit_node(|| {
                NODE_LABELS.with(|labels| labels.borrow_mut().push("A"));
                LabeledNode
            });
        } else {
            composer.emit_node(|| {
                NODE_LABELS.with(|labels| labels.borrow_mut().push("B"));
                LabeledNode
            });
        }
    }
}

#[test]
fn raw_nodes_in_arms_do_not_trade_places() {
    raw_node_arms::NODE_LABELS.with(|labels| labels.borrow_mut().clear());
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, first: bool| {
        composition.render(83, || raw_node_arms::probe(first))
    };

    pass(&mut composition, true).expect("initial composition");
    raw_node_arms::NODE_LABELS.with(|labels| {
        assert_eq!(&*labels.borrow(), &["A"]);
    });

    pass(&mut composition, false).expect("switch to the else arm");
    raw_node_arms::NODE_LABELS.with(|labels| {
        assert_eq!(
            &*labels.borrow(),
            &["A", "B"],
            "the else arm must build its own node, not adopt the then arm's"
        );
    });
    assert_composition_valid(&composition);
}

#[composable]
fn branch_entry_probe(enabled: bool) {
    if enabled {
        let value = remember_branch_marker(41);
        BRANCH_SEEN.with(|seen| seen.set(value));
    }
    let tail = remember(|| {
        BRANCH_INITS.with(|count| count.set(count.get() + 1));
        42_i32
    });
    BRANCH_LOG.with(|log| {
        log.borrow_mut()
            .push(format!("tail {}", tail.with(|value| *value)))
    });
}

#[test]
fn an_appearing_branch_inserts_before_the_tail_slot() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, enabled: bool| {
        composition.render(84, || branch_entry_probe(enabled))
    };

    pass(&mut composition, false).expect("initial composition");
    assert_eq!(branch_inits(), 1);

    pass(&mut composition, true).expect("the branch appears");
    assert_eq!(
        branch_inits(),
        2,
        "the appearing branch must insert its slot, not destroy the tail's"
    );

    pass(&mut composition, false).expect("the branch leaves again");
    assert_eq!(
        branch_inits(),
        2,
        "the tail slot survives the branch in both directions"
    );
    assert_composition_valid(&composition);
}

mod node_entry {
    use super::*;

    thread_local! {
        pub(super) static TAIL_BUILDS: Cell<usize> = const { Cell::new(0) };
        pub(super) static BRANCH_BUILDS: Cell<usize> = const { Cell::new(0) };
    }

    pub(super) struct EntryNode;

    impl crate::Node for EntryNode {}

    #[composable(no_skip)]
    pub(super) fn probe(enabled: bool) {
        let composer = with_current_composer(Clone::clone);
        if enabled {
            composer.emit_node(|| {
                BRANCH_BUILDS.with(|count| count.set(count.get() + 1));
                EntryNode
            });
        }
        composer.emit_node(|| {
            TAIL_BUILDS.with(|count| count.set(count.get() + 1));
            EntryNode
        });
    }
}

#[test]
fn an_appearing_branch_node_does_not_remount_the_tail_node() {
    node_entry::TAIL_BUILDS.with(|count| count.set(0));
    node_entry::BRANCH_BUILDS.with(|count| count.set(0));
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, enabled: bool| {
        composition.render(85, || node_entry::probe(enabled))
    };

    pass(&mut composition, false).expect("initial composition");
    assert_eq!(node_entry::TAIL_BUILDS.with(Cell::get), 1);

    pass(&mut composition, true).expect("the branch node appears");
    assert_eq!(
        (
            node_entry::BRANCH_BUILDS.with(Cell::get),
            node_entry::TAIL_BUILDS.with(Cell::get)
        ),
        (1, 1),
        "the appearing branch node must not unmount and rebuild the tail node"
    );

    pass(&mut composition, false).expect("the branch node leaves");
    assert_eq!(
        node_entry::TAIL_BUILDS.with(Cell::get),
        1,
        "the tail node survives the branch in both directions"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn nested_fn_escape_probe(first: bool) {
    fn arm_a() {
        let value = remember_branch_marker(51);
        BRANCH_SEEN.with(|seen| seen.set(value));
    }
    fn arm_b() {
        let value = remember_branch_marker(52);
        BRANCH_SEEN.with(|seen| seen.set(value));
    }
    let selected: fn() = if first { arm_a } else { arm_b };
    selected();
}

#[test]
fn a_branch_selected_fn_item_keeps_its_identity() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, first: bool| {
        composition.render(86, || nested_fn_escape_probe(first))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 51));

    pass(&mut composition, false).expect("switch to the other fn item");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 52),
        "a fn item selected per arm must compose its own state"
    );
    assert_composition_valid(&composition);
}

struct NotProbe {
    marker: i32,
}

impl NotProbe {
    fn new(marker: i32) -> Self {
        Self { marker }
    }
}

impl std::ops::Not for NotProbe {
    type Output = bool;

    fn not(self) -> bool {
        let value = remember(|| {
            BRANCH_INITS.with(|count| count.set(count.get() + 1));
            self.marker
        });
        BRANCH_SEEN.with(|seen| seen.set(value.with(|value| *value)));
        false
    }
}

#[composable]
fn unary_condition_probe(enabled: bool) {
    if enabled && !NotProbe::new(1) {
        BRANCH_LOG.with(|log| log.borrow_mut().push("on".to_string()));
    }
    let _ = !NotProbe::new(2);
}

#[test]
fn a_composing_unary_in_a_condition_stays_inside_its_fold() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, enabled: bool| {
        composition.render(87, || unary_condition_probe(enabled))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (2, 2));

    pass(&mut composition, false).expect("short-circuit the condition");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 2),
        "the unconditional Not must keep its own slot when the condition's Not vanishes"
    );
    assert_composition_valid(&composition);
}

#[composable]
#[allow(non_snake_case)]
fn PageA() {
    let value = remember_branch_marker(61);
    BRANCH_SEEN.with(|seen| seen.set(value));
}

#[composable]
#[allow(non_snake_case)]
fn PageB() {
    let value = remember_branch_marker(62);
    BRANCH_SEEN.with(|seen| seen.set(value));
}

#[composable]
fn fn_pointer_page_probe(first: bool) {
    let page: fn() = if first { PageA } else { PageB };
    page();
}

#[test]
fn two_composables_selected_through_one_call_site_stay_distinct() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, first: bool| {
        composition.render(88, || fn_pointer_page_probe(first))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 61));

    pass(&mut composition, false).expect("switch pages through the pointer");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 62),
        "identity must carry the callee too; PageB is not PageA at the same call site"
    );
    assert_composition_valid(&composition);
}

macro_rules! page_route {
    ($first:expr) => {
        if $first {
            PageA();
        } else {
            PageB();
        }
    };
}

#[composable]
fn macro_routed_page_probe(first: bool) {
    page_route!(first);
}

#[test]
fn two_composables_selected_through_one_macro_call_site_stay_distinct() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, first: bool| {
        composition.render(89, || macro_routed_page_probe(first))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 61));

    pass(&mut composition, false).expect("switch pages through the macro");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 62),
        "span collapse makes both calls report one caller; the definition key must \
         still separate PageB from PageA"
    );

    pass(&mut composition, true).expect("switch back to the first page");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (3, 61),
        "returning to PageA remakes its slot fresh instead of adopting PageB's"
    );
    assert_composition_valid(&composition);
}

fn next_marker(enabled: bool, marker: i32) -> Option<i32> {
    enabled.then(|| remember_branch_marker(marker))
}

struct ComposingIter {
    remaining: usize,
    marker: i32,
}

impl Iterator for ComposingIter {
    type Item = i32;

    fn next(&mut self) -> Option<i32> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        Some(remember_branch_marker(self.marker))
    }
}

#[composable]
fn adjacent_loops_probe(first_count: usize) {
    for value in (ComposingIter {
        remaining: first_count,
        marker: 75,
    }) {
        let _ = value;
    }
    for value in (ComposingIter {
        remaining: 1,
        marker: 76,
    }) {
        BRANCH_SEEN.with(|seen| seen.set(value));
    }
}

#[test]
fn a_shrinking_loop_does_not_feed_the_next_loops_iterator() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, first_count: usize| {
        composition.render(95, || adjacent_loops_probe(first_count))
    };

    pass(&mut composition, 1).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (2, 76));

    pass(&mut composition, 0).expect("run the first loop zero times");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 76),
        "a composing next() outside the body fold must not hand the first \
         loop's slot to the second loop"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn while_let_scrutinee_probe(mut count: usize) {
    while let Some(_marker) = next_marker(count > 0, 10) {
        count -= 1;
    }
    let tail = next_marker(true, 20).expect("the tail always composes");
    BRANCH_SEEN.with(|seen| seen.set(tail));
}

#[test]
fn a_shrinking_while_let_scrutinee_does_not_feed_a_later_helper_call() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, count: usize| {
        composition.render(94, || while_let_scrutinee_probe(count))
    };

    pass(&mut composition, 1).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (2, 20));

    pass(&mut composition, 0).expect("run the loop zero times");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 20),
        "the tail call must keep its own slot when the loop scrutinee composes \
         fewer times"
    );
    assert_composition_valid(&composition);
}

struct ChainHolder {
    value: Option<i32>,
    marker: i32,
}

impl std::ops::Deref for ChainHolder {
    type Target = Option<i32>;

    fn deref(&self) -> &Option<i32> {
        let value = remember_branch_marker(self.marker);
        if self.marker == 72 {
            BRANCH_SEEN.with(|seen| seen.set(value));
        }
        &self.value
    }
}

#[composable]
fn short_circuited_place_deref_probe(enabled: bool) {
    let first = ChainHolder {
        value: Some(1),
        marker: 71,
    };
    let second = ChainHolder {
        value: Some(2),
        marker: 72,
    };
    if enabled && let Some(_) = *first {}
    let _got = *second;
}

#[test]
fn a_short_circuited_place_deref_does_not_leak_into_a_later_deref() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, enabled: bool| {
        composition.render(92, || short_circuited_place_deref_probe(enabled))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (2, 72));

    pass(&mut composition, false).expect("short-circuit the chained deref");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 72),
        "the unconditional deref must keep its own slot, not adopt the short-circuited \
         deref's slot and not reinitialize"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn chain_binding_probe(enabled: bool) {
    let first = ChainHolder {
        value: Some(5),
        marker: 71,
    };
    if enabled
        && let Some(ref x) = *first
        && *x > 0
    {
        BRANCH_SEEN.with(|seen| seen.set(*x));
    }
}

#[test]
fn chain_let_bindings_still_reach_the_arm() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, enabled: bool| {
        composition.render(93, || chain_binding_probe(enabled))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!(branch_seen(), 5);

    pass(&mut composition, false).expect("drop the arm");
    pass(&mut composition, true).expect("re-enter the arm");
    assert_eq!(branch_seen(), 5);
    assert_composition_valid(&composition);
}

fn require_send<T: Send>(value: T) -> T {
    value
}

#[composable]
fn async_closure_probe() {
    let make = async || std::future::ready(7).await;
    let future = require_send(make());
    drop(future);
}

#[test]
fn an_async_closure_future_stays_send() {
    let mut composition = test_composition();
    composition
        .render(91, || async_closure_probe())
        .expect("compose the async closure");
    assert_composition_valid(&composition);
}

macro_rules! make_template_pages {
    ($a:ident: $ma:expr, $b:ident: $mb:expr) => {
        #[composable]
        #[allow(non_snake_case)]
        fn $a() {
            let value = remember_branch_marker($ma);
            BRANCH_SEEN.with(|seen| seen.set(value));
        }

        #[composable]
        #[allow(non_snake_case)]
        fn $b() {
            let value = remember_branch_marker($mb);
            BRANCH_SEEN.with(|seen| seen.set(value));
        }
    };
}

make_template_pages!(TemplatePageA: 63, TemplatePageB: 64);

#[composable]
fn template_page_probe(first: bool) {
    let page: fn() = if first { TemplatePageA } else { TemplatePageB };
    page();
}

#[test]
fn two_composables_from_one_template_invocation_stay_distinct() {
    reset_branch_probes();
    let mut composition = test_composition();
    let pass = |composition: &mut Composition<MemoryApplier>, first: bool| {
        composition.render(90, || template_page_probe(first))
    };

    pass(&mut composition, true).expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 63));

    pass(&mut composition, false).expect("switch template pages");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 64),
        "one template invocation collapses both definition locations; the local \
         marker type must still separate the two functions"
    );
    assert_composition_valid(&composition);
}
