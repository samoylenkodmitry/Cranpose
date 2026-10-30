use std::{cell::Cell, rc::Rc};

use super::{CallbackHolder, ParamSlot};
use crate::{RecomposeScope, runtime::TestRuntime};

#[test]
fn param_slot_take_reports_absence_instead_of_panicking() {
    let slot = ParamSlot::default();

    assert_eq!(slot.take(), None);
    slot.set(11);
    assert_eq!(slot.take(), Some(11));
    assert_eq!(slot.take(), None);
}

#[test]
fn callback_holder_default_forwarder_is_noop() {
    let forwarder = CallbackHolder::new().clone_rc();
    forwarder();
}

#[test]
fn callback_holder_forwarder_uses_latest_callback() {
    let holder = CallbackHolder::new();
    let total = Rc::new(Cell::new(0));
    let forwarder = holder.clone_rc();

    let first_total = Rc::clone(&total);
    holder.update(move || first_total.set(first_total.get() + 1));
    forwarder();

    let second_total = Rc::clone(&total);
    holder.update(move || second_total.set(second_total.get() + 10));
    forwarder();

    assert_eq!(total.get(), 11);
}

#[test]
fn callback_holder_does_not_invoke_after_creator_scope_deactivation() {
    let runtime = TestRuntime::new();
    let scope = RecomposeScope::new_for_test(runtime.handle());
    let holder = CallbackHolder::new();
    let invocations = Rc::new(Cell::new(0));
    let invocations_for_callback = Rc::clone(&invocations);
    holder.update(move || invocations_for_callback.set(invocations_for_callback.get() + 1));
    holder
        .shared
        .owner
        .creator_scope
        .replace(Some(scope.clone()));
    let forwarder = holder.clone_rc();

    forwarder();
    assert_eq!(invocations.get(), 1);

    scope.deactivate();
    forwarder();
    assert_eq!(
        invocations.get(),
        1,
        "callbacks cannot outlive the active composition scope that owns them",
    );
}

#[test]
fn callback_holder_does_not_invoke_under_inactive_creator_ancestor() {
    let runtime = TestRuntime::new();
    let ancestor = RecomposeScope::new_for_test(runtime.handle());
    let creator = RecomposeScope::new_for_test(runtime.handle());
    creator.set_parent_scope(Some(&ancestor));
    let holder = CallbackHolder::new();
    let invocations = Rc::new(Cell::new(0));
    let invocations_for_callback = Rc::clone(&invocations);
    holder.update(move || invocations_for_callback.set(invocations_for_callback.get() + 1));
    holder.shared.owner.creator_scope.replace(Some(creator));
    let forwarder = holder.clone_rc();

    forwarder();
    assert_eq!(invocations.get(), 1);

    ancestor.deactivate();
    forwarder();
    assert_eq!(
        invocations.get(),
        1,
        "callbacks cannot run beneath an inactive composition ancestor",
    );
}

#[test]
fn an_argument_callback_forwarder_does_nothing_before_a_callback_is_stored() {
    let forwarder = CallbackHolder::<dyn FnMut(i32)>::new().forwarder();
    forwarder.call(|callback| callback(7));
}

#[test]
fn an_argument_callback_forwarder_calls_the_latest_callback() {
    let holder = CallbackHolder::<dyn FnMut(i32)>::new();
    let total = Rc::new(Cell::new(0));
    let forwarder = holder.forwarder();

    let first_total = Rc::clone(&total);
    holder.update_boxed(Box::new(move |value| {
        first_total.set(first_total.get() + value);
    }));
    forwarder.call(|callback| callback(2));

    let second_total = Rc::clone(&total);
    holder.update_boxed(Box::new(move |value| {
        second_total.set(second_total.get() + value * 5);
    }));
    forwarder.call(|callback| callback(3));

    assert_eq!(total.get(), 17);
}

#[test]
fn an_argument_callback_does_not_run_after_its_creator_scope_deactivates() {
    let runtime = TestRuntime::new();
    let scope = RecomposeScope::new_for_test(runtime.handle());
    let holder = CallbackHolder::<dyn FnMut(i32)>::new();
    let total = Rc::new(Cell::new(0));
    let total_for_callback = Rc::clone(&total);
    holder.update_boxed(Box::new(move |value| {
        total_for_callback.set(total_for_callback.get() + value);
    }));
    holder
        .shared
        .owner
        .creator_scope
        .replace(Some(scope.clone()));
    let forwarder = holder.forwarder();

    forwarder.call(|callback| callback(3));
    assert_eq!(total.get(), 3);

    scope.deactivate();
    forwarder.call(|callback| callback(5));
    assert_eq!(
        total.get(),
        3,
        "argument callbacks cannot outlive their active composition owner",
    );
}
