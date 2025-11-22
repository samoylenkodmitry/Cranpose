use compose_core::MutableState;
use compose_testing::ComposeTestRule;
use desktop_app::app::{
    combined_app, DemoTab, TEST_ACTIVE_TAB_STATE, TEST_COMPOSITION_LOCAL_COUNTER,
};

fn with_active_tab<F>(f: F)
where
    F: FnOnce(&MutableState<DemoTab>),
{
    TEST_ACTIVE_TAB_STATE.with(|cell| {
        let state = *cell.borrow().as_ref().expect("active tab state registered");
        f(&state);
    });
}

fn set_active_tab(tab: DemoTab) {
    with_active_tab(|state| state.set(tab));
}

fn increment_composition_local_counter() {
    TEST_COMPOSITION_LOCAL_COUNTER.with(|cell| {
        let state = *cell
            .borrow()
            .as_ref()
            .expect("composition local counter state not registered");
        state.set(state.get() + 1);
    });
}

fn wait_for_counter_registration(rule: &mut ComposeTestRule) {
    for _ in 0..10 {
        let registered = TEST_COMPOSITION_LOCAL_COUNTER.with(|cell| cell.borrow().is_some());
        if registered {
            return;
        }
        rule.pump_until_idle()
            .expect("pump while waiting for composition local counter registration");
    }
    let tree = rule.dump_tree();
    panic!(
        "composition local counter state not registered after retries. tree:\n{}",
        tree
    );
}

#[test]
fn composition_local_view_duplicates_regression() {
    TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow_mut().take());
    TEST_COMPOSITION_LOCAL_COUNTER.with(|cell| cell.borrow_mut().take());

    let mut rule = ComposeTestRule::new();
    rule.set_content(combined_app)
        .expect("install combined app content");
    rule.pump_until_idle()
        .expect("initial idle after counter view");

    set_active_tab(DemoTab::CompositionLocal);
    rule.pump_until_idle()
        .expect("switch to composition local view");
    wait_for_counter_registration(&mut rule);
    println!("tree after switch:\n{}", rule.dump_tree());

    let baseline_nodes = rule.applier_mut().len();

    for step in 1..=2 {
        increment_composition_local_counter();
        rule.pump_until_idle()
            .unwrap_or_else(|_| panic!("pump after increment {}", step));
        rule.advance_frame(0)
            .unwrap_or_else(|_| panic!("advance frame after increment {}", step));
        println!("tree after increment {}:\n{}", step, rule.dump_tree());
    }

    let after_nodes = rule.applier_mut().len();
    assert_eq!(
        after_nodes, baseline_nodes,
        "node count changed after increments: before={}, after={}",
        baseline_nodes, after_nodes
    );
}
