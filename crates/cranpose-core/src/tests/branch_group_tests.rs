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
