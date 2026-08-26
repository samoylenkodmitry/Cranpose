use super::*;

// Every conditional branch of a `#[composable]` body owns its composition
// slots, the way Compose's compiler plugin gives each `if`/`else` and `match`
// branch a group of its own. The arriving branch must never be handed the
// node, `remember` slots, or effects the departing branch was using just
// because the two branches share a slot shape.

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

    composition
        .render(1, || if_branch_remember_probe(true))
        .expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    composition
        .render(1, || if_branch_remember_probe(false))
        .expect("switch to the else branch");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 2),
        "the else branch must not inherit the then branch's remember slot"
    );

    composition
        .render(1, || if_branch_remember_probe(true))
        .expect("switch back to the then branch");
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

    composition
        .render(3, || branches_calling_the_same_child(true))
        .expect("initial composition");
    assert_eq!(branch_seen(), 100);

    composition
        .render(3, || branches_calling_the_same_child(false))
        .expect("switch to the else branch");
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

    composition
        .render(5, || branch_effect_probe(true))
        .expect("initial composition");
    assert_eq!(branch_log(), vec!["then:start".to_string()]);

    composition
        .render(5, || branch_effect_probe(false))
        .expect("switch to the else branch");
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
    // Method calls and std value macros cannot compose either: composables
    // are free functions, so these branches must stay group-free too.
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
    with_branch
        .render(6, || value_only_branch_probe(true))
        .expect("compose the value-only branch probe");
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

    with_branch
        .render(6, || value_only_branch_probe(false))
        .expect("flip the value-only branch");
    assert_eq!(branch_seen(), 8 + "off 8".len() as i32);
    assert_composition_valid(&with_branch);
}

#[composable]
fn format_arg_probe(cond: bool) {
    // `format!` cannot compose, but its arguments can: the token scan must
    // see the `remember*` call inside and give each branch its group.
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

    composition
        .render(16, || format_arg_probe(true))
        .expect("initial composition");
    assert_eq!(branch_inits(), 1);
    composition
        .render(16, || format_arg_probe(false))
        .expect("switch the format-arg branch");
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

    composition
        .render(7, || early_return_probe(true))
        .expect("compose the early-return branch");
    assert_eq!((branch_inits(), branch_seen()), (1, 7));
    assert_composition_valid(&composition);

    composition
        .render(7, || early_return_probe(false))
        .expect("compose past the early return");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 8),
        "the tail after the conditional must not read the departed branch's slot"
    );
    assert_composition_valid(&composition);

    composition
        .render(7, || early_return_probe(true))
        .expect("return to the early-return branch");
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
    // A closure with no composable-shaped call inside it is not content, so
    // the transform leaves it untouched: its branches carry no guards at all,
    // not even the no-op thread-local kind.
    reset_branch_probes();
    let mut composition = test_composition();
    composition
        .render(11, || closure_branch_probe(true))
        .expect("compose the closure probe");
    assert_eq!(branch_seen(), 1);
    composition
        .render(11, || closure_branch_probe(false))
        .expect("flip the closure probe");
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
    // The branch transform must reach into content lambdas — the closures a
    // caller hands to layout composables — because that is where most
    // conditionals in real applications live. The guard resolves the
    // composer through the thread-local context since a `'static` closure
    // cannot capture `__composer`.
    reset_branch_probes();
    let mut composition = test_composition();

    composition
        .render(13, || content_closure_branch_probe(true))
        .expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1000));

    composition
        .render(13, || content_closure_branch_probe(false))
        .expect("switch the content closure's branch");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 2000),
        "a branch inside a content closure must own its slots"
    );

    composition
        .render(13, || content_closure_branch_probe(true))
        .expect("switch back");
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
    // Lazy item lambdas are `|index| …`-shaped; the classifier keys on the
    // composable call inside, not on arity.
    reset_branch_probes();
    let mut composition = test_composition();

    composition
        .render(15, || indexed_closure_branch_probe(true))
        .expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 3007));

    composition
        .render(15, || indexed_closure_branch_probe(false))
        .expect("switch the indexed content branch");
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
    // The key argument itself composes, so this branch cannot elide its
    // bracket: `remembered_key_marker` runs before `with_key`'s group opens
    // and would share a slot across branches without one.
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

    composition
        .render(20, || composing_key_argument_probe(true))
        .expect("initial composition");
    assert_eq!(branch_inits(), 1);

    composition
        .render(20, || composing_key_argument_probe(false))
        .expect("switch the branch");
    assert_eq!(
        branch_inits(),
        2,
        "a remember evaluated inside the key argument must not be shared across branches"
    );

    composition
        .render(20, || composing_key_argument_probe(true))
        .expect("switch back");
    assert_eq!(branch_inits(), 3);
    assert_composition_valid(&composition);
}

#[composable]
fn snake_case_closure_branch_probe(cond: bool) {
    content_host(move || {
        // `stateful_child` is snake_case: the closure classifier must key on
        // reachability, not naming convention.
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

    composition
        .render(21, || snake_case_closure_branch_probe(true))
        .expect("initial composition");
    assert_eq!(branch_seen(), 7100);

    composition
        .render(21, || snake_case_closure_branch_probe(false))
        .expect("switch the branch");
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
        // The macro's own guard binding uses mixed_site hygiene, so this
        // user binding of the same name stays visible to user code.
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
    // The CamelCase call makes the classifier treat this handler as content,
    // so its branches carry guards — but running it outside a composition
    // pass must find no composer and change nothing.
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
    // `for row { if visible { with_key(id, …) } }` is the canonical list
    // shape. Hiding the first row shifts every later iteration's branch
    // bracket by one; the explicit key inside must keep its subtree — moved,
    // not disposed and recomposed — or a 1024-row list pays a full rebuild
    // for one hidden row and loses all its keyed state.
    reset_branch_probes();
    let mut composition = test_composition();

    composition
        .render(17, || {
            keyed_visibility_probe(vec![(1, true), (2, true), (3, true)])
        })
        .expect("initial composition");
    assert_eq!(branch_inits(), 3);
    assert_eq!(keyed_values(), vec![(1, 1), (2, 2), (3, 3)]);

    composition
        .render(17, || {
            keyed_visibility_probe(vec![(1, false), (2, true), (3, true)])
        })
        .expect("hide the first row");
    assert_eq!(
        (branch_inits(), keyed_values()),
        (3, vec![(2, 2), (3, 3)]),
        "hiding one row must not recompose the keyed rows behind it"
    );
    assert_composition_valid(&composition);

    composition
        .render(17, || {
            keyed_visibility_probe(vec![(1, true), (2, true), (3, true)])
        })
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
            // Unkeyed content beside the keyed call: this branch cannot elide
            // its bracket, so the keyed subtree must travel between brackets
            // through the steal/orphan-pool machinery.
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

    composition
        .render(19, || {
            mixed_keyed_visibility_probe(vec![(1, true), (2, true), (3, true)])
        })
        .expect("initial composition");
    assert_eq!(branch_inits(), 3);
    assert_eq!(keyed_values(), vec![(1, 1), (2, 2), (3, 3)]);

    composition
        .render(19, || {
            mixed_keyed_visibility_probe(vec![(1, false), (2, true), (3, true)])
        })
        .expect("hide the first row");
    assert_eq!(
        (branch_inits(), keyed_values()),
        (3, vec![(2, 2), (3, 3)]),
        "a bracketed keyed row must be stolen across brackets, not recomposed"
    );
    assert_composition_valid(&composition);

    composition
        .render(19, || {
            mixed_keyed_visibility_probe(vec![(1, true), (2, true), (3, true)])
        })
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

    composition
        .render(18, || keyed_visibility_probe(vec![(1, true), (2, true)]))
        .expect("initial composition");
    assert_eq!(keyed_values(), vec![(1, 1), (2, 2)]);

    composition
        .render(18, || keyed_visibility_probe(vec![(2, true), (1, true)]))
        .expect("swap the rows");
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

    composition
        .render(12, || loop_branch_probe(vec![true, true]))
        .expect("compose two then-branches");
    assert_eq!(branch_log(), vec!["t1".to_string(), "t2".to_string()]);

    composition
        .render(12, || loop_branch_probe(vec![true, false]))
        .expect("flip the second iteration");
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

    composition
        .render(30, || method_call_branch_probe(true))
        .expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    composition
        .render(30, || method_call_branch_probe(false))
        .expect("switch to the else branch");
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

    composition
        .render(31, || guard_composition_probe(0))
        .expect("compose the first guard");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    composition
        .render(31, || guard_composition_probe(1))
        .expect("switch to the second guard");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 2),
        "the second guard must not be handed the first guard's remember slot"
    );

    composition
        .render(31, || guard_composition_probe(0))
        .expect("switch back to the first guard");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (3, 1),
        "returning to a departed guard must compose fresh state"
    );
    assert_composition_valid(&composition);
}

mod fake_arity_with_key {
    use super::*;

    /// Same name as the framework's `with_key`, different arity, no keyed
    /// group: the elision must not mistake it for the real one.
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

    composition
        .render(32, || fake_arity_with_key::probe(true))
        .expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 7));

    composition
        .render(32, || fake_arity_with_key::probe(false))
        .expect("switch to the else branch");
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

    composition
        .render(33, || local_fn_branch_probe(true))
        .expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    composition
        .render(33, || local_fn_branch_probe(false))
        .expect("switch to the else branch");
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

    composition
        .render(34, || method_keyed_arg_probe(true))
        .expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (2, 101));

    composition
        .render(34, || method_keyed_arg_probe(false))
        .expect("switch to the else branch");
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

    composition
        .render(35, || closure_through_method_probe(true))
        .expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    composition
        .render(35, || closure_through_method_probe(false))
        .expect("switch to the else branch");
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

    composition
        .render(36, || composing_condition_probe(true))
        .expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (2, 99));

    composition
        .render(36, || composing_condition_probe(false))
        .expect("short-circuit the condition");
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

    composition
        .render(37, || impl_method_branch_probe(true))
        .expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    composition
        .render(37, || impl_method_branch_probe(false))
        .expect("switch to the else branch");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 2),
        "a local impl method runs under the current composer; its branches need groups"
    );
    assert_composition_valid(&composition);
}

#[composable]
fn composer_shadowing_probe(cond: bool) {
    // `__composer` is the expansion's own parameter name; a user binding of
    // it must neither break the guards (they hold a hygienic alias captured
    // before any user statement) nor be disturbed.
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

    composition
        .render(38, || composer_shadowing_probe(true))
        .expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 7));

    composition
        .render(38, || composer_shadowing_probe(false))
        .expect("switch to the else branch");
    assert_eq!((branch_inits(), branch_seen()), (2, 8));
    assert_composition_valid(&composition);
}

mod fake_shape_with_key {
    use super::*;

    /// The real API's exact shape — two arguments, closure last — but no
    /// keyed group inside: the elision must repair itself at runtime.
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

    composition
        .render(39, || fake_shape_with_key::probe(true))
        .expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    composition
        .render(39, || fake_shape_with_key::probe(false))
        .expect("switch to the else branch");
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

    composition
        .render(40, || keyed_via_helper_probe(true))
        .expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    composition
        .render(40, || keyed_via_helper_probe(false))
        .expect("switch to the else branch");
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

    composition
        .render(41, || value_macro_snake_probe(true))
        .expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    composition
        .render(41, || value_macro_snake_probe(false))
        .expect("switch to the else branch");
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

    composition
        .render(42, || cross_branch_pool_probe(1, false))
        .expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    // The first branch re-keys from 1 to 2 and parks key 1; the second branch
    // then asks for key 1 from the same helper line. Branch isolation must
    // win: the parked subtree belongs to the first branch's site.
    composition
        .render(42, || cross_branch_pool_probe(2, true))
        .expect("re-key the first branch and enable the second");
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

    composition
        .render(43, || method_state_branch_probe(true))
        .expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    composition
        .render(43, || method_state_branch_probe(false))
        .expect("switch to the else branch");
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

    composition
        .render(44, || value_macro_method_probe(true))
        .expect("initial composition");
    assert_eq!((branch_inits(), branch_seen()), (1, 1));

    composition
        .render(44, || value_macro_method_probe(false))
        .expect("switch to the else branch");
    assert_eq!(
        (branch_inits(), branch_seen()),
        (2, 2),
        "`composer.remember` inside `format!` still composes; the branch needs its bracket"
    );
    assert_composition_valid(&composition);
}
