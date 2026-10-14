//! A composable recomposes on the states its latest composition read, not on
//! the states an earlier composition read: as in Compose, a recomposition
//! drops the reads it did not repeat (#1552).

use std::{cell::Cell, rc::Rc};

use cranpose_core::MutableState;
use cranpose_macros::composable;
use cranpose_testing::ComposeTestRule;

/// A composition counter that stays equal to itself while it counts.
#[derive(Clone, Default)]
struct Count(Rc<Cell<usize>>);

impl PartialEq for Count {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Count {
    fn get(&self) -> usize {
        self.0.get()
    }
}

#[composable]
fn Reader(idle: MutableState<bool>, offset: MutableState<f32>, count: Count) {
    if idle.get() {
        let _ = offset.get();
    }
    count.0.set(count.0.get() + 1);
}

#[test]
fn a_recomposition_drops_the_reads_it_did_not_repeat() {
    let context = cranpose_ui::AppContext::new();
    let _scope = context.enter_scope();
    context.enter(cranpose_ui::reset_render_state_for_tests);
    let mut rule = ComposeTestRule::new();
    let runtime = rule.runtime_handle();
    let idle = MutableState::with_runtime(true, runtime.clone());
    let offset = MutableState::with_runtime(0.0_f32, runtime);
    let count = Count::default();
    let content = count.clone();
    rule.set_content(move || Reader(idle, offset, content.clone()))
        .expect("first composition");
    assert_eq!(count.get(), 1);

    idle.set(false);
    rule.pump_until_idle().expect("recomposition");
    assert_eq!(count.get(), 2, "the flag it read changed");

    for step in 1..=10 {
        offset.set(step as f32);
        rule.pump_until_idle().expect("frame");
    }
    assert_eq!(
        count.get(),
        2,
        "the offset changed ten times after the reader stopped reading it"
    );

    idle.set(true);
    rule.pump_until_idle().expect("recomposition");
    offset.set(42.0);
    rule.pump_until_idle().expect("recomposition");
    assert_eq!(count.get(), 4, "reading the offset again subscribes again");
}

#[composable]
fn Parent(value: MutableState<i32>, parent: Count, child: Count, seen: Rc<Cell<i32>>) {
    parent.0.set(parent.0.get() + 1);
    Child(value, child, seen);
}

#[composable]
fn Child(value: MutableState<i32>, count: Count, seen: Rc<Cell<i32>>) {
    seen.set(value.get());
    count.0.set(count.0.get() + 1);
}

#[test]
fn a_child_keeps_its_own_reads_across_recompositions() {
    let context = cranpose_ui::AppContext::new();
    let _scope = context.enter_scope();
    context.enter(cranpose_ui::reset_render_state_for_tests);
    let mut rule = ComposeTestRule::new();
    let value = MutableState::with_runtime(0, rule.runtime_handle());
    let (parent, child) = (Count::default(), Count::default());
    let seen = Rc::new(Cell::new(-1));
    let content = (parent.clone(), child.clone(), Rc::clone(&seen));
    rule.set_content(move || {
        Parent(
            value,
            content.0.clone(),
            content.1.clone(),
            Rc::clone(&content.2),
        );
    })
    .expect("first composition");
    for step in 1..=3 {
        value.set(step);
        rule.pump_until_idle().expect("recomposition");
        assert_eq!(
            seen.get(),
            step,
            "the child shows the value after step {step}"
        );
    }
    assert_eq!(parent.get(), 1, "only the child reads the value");
    assert_eq!(child.get(), 4);
}
