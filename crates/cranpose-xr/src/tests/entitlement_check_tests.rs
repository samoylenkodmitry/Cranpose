use std::{cell::Cell, rc::Rc};

use cranpose_core::{CompositionLocalProvider, remember};
use cranpose_testing::create_headless_robot_test;

use crate::{Entitlement, EntitlementCheck, OnNotEntitled, local_entitlement_check};

#[test]
fn a_new_check_is_running_and_quits_on_a_failure() {
    cranpose_testing::run_test_composition(|_| {
        let check = EntitlementCheck::new();
        assert_eq!(check.entitlement(), Entitlement::Checking);
        assert_eq!(check.on_not_entitled(), OnNotEntitled::Quit);
    });
}

#[test]
fn the_apps_choice_reaches_the_backend_and_the_answer_reaches_the_app() {
    cranpose_testing::run_test_composition(|_| {
        let check = EntitlementCheck::new();
        check.set_on_not_entitled(OnNotEntitled::Continue);
        assert_eq!(check.policy(), OnNotEntitled::Continue);
        check.publish(Entitlement::NotEntitled);
        assert_eq!(check.entitlement(), Entitlement::NotEntitled);
    });
}

#[test]
fn content_outside_a_headset_finds_no_check() {
    let found = Rc::new(Cell::new(None::<bool>));
    let seen = Rc::clone(&found);
    let _robot = create_headless_robot_test(100, 100, move || {
        seen.set(Some(local_entitlement_check().current().is_some()));
    });
    assert_eq!(found.get(), Some(false));
}

#[test]
fn content_in_a_headset_reads_the_check_that_the_backend_provides() {
    let seen = Rc::new(Cell::new(None::<(Entitlement, OnNotEntitled)>));
    let reader = Rc::clone(&seen);
    let _robot = create_headless_robot_test(100, 100, move || {
        let check = remember(EntitlementCheck::new).with(|check| *check);
        let reader = Rc::clone(&reader);
        CompositionLocalProvider(
            [local_entitlement_check().provides(Some(check))],
            move || {
                let check = local_entitlement_check().current().unwrap();
                reader.set(Some((check.entitlement(), check.on_not_entitled())));
            },
        );
    });
    assert_eq!(
        seen.get(),
        Some((Entitlement::Checking, OnNotEntitled::Quit))
    );
}
