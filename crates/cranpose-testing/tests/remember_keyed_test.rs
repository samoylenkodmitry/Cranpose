//! `rememberKeyed` contract: a slot reused with a DIFFERENT key re-runs
//! its initializer, while a stable key never recomputes. The position-only
//! `remember` served a stale parsed icon when the accordion's "Sections" row
//! took over the slot of a row that had cached a chevron path.

use std::{cell::Cell, rc::Rc};

use cranpose_core::{MutableState, rememberKeyed};
use cranpose_macros::composable;
use cranpose_testing::ComposeTestRule;
use cranpose_ui::*;

#[composable]
fn keyed_app(key: MutableState<&'static str>, init_runs: Rc<Cell<u32>>, seen: Rc<Cell<usize>>) {
    let current = key.get();
    let runs = Rc::clone(&init_runs);
    let value = rememberKeyed(current, move |k| {
        runs.set(runs.get() + 1);
        k.len()
    });
    seen.set(value);
    Text(
        format!("len {value}"),
        Modifier::empty(),
        TextStyle::default(),
    );
}

#[test]
fn remember_keyed_recomputes_on_key_change_only() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);

    let mut rule = ComposeTestRule::new();
    let runtime = rule.runtime_handle();
    let init_runs = Rc::new(Cell::new(0u32));
    let seen = Rc::new(Cell::new(0usize));
    let key = MutableState::with_runtime("chevron", runtime.clone());

    rule.set_content({
        let init_runs = Rc::clone(&init_runs);
        let seen = Rc::clone(&seen);
        move || keyed_app(key, Rc::clone(&init_runs), Rc::clone(&seen))
    })
    .expect("compose");
    assert_eq!(init_runs.get(), 1);
    assert_eq!(seen.get(), "chevron".len());

    // Same key: recomposition must NOT recompute.
    rule.pump_until_idle().expect("idle");
    assert_eq!(init_runs.get(), 1);

    // The slot is reused with a different key: the value must be recomputed
    // (the stale-icon class this API exists to prevent).
    key.set("list-outline");
    rule.pump_until_idle().expect("recompose after key change");
    assert_eq!(init_runs.get(), 2);
    assert_eq!(seen.get(), "list-outline".len());
}
