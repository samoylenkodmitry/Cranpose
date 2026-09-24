use std::{cell::RefCell, rc::Rc};

use super::*;
use crate::run_test_composition;

const READER_ON: AccessibilityState = AccessibilityState {
    screen_reader_on: true,
};

#[test]
fn the_platform_state_reports_a_change_once() {
    set_platform_accessibility_state(AccessibilityState::default());
    assert!(set_platform_accessibility_state(READER_ON));
    assert!(!set_platform_accessibility_state(READER_ON));
    assert_eq!(platform_accessibility_state(), READER_ON);
    assert!(set_platform_accessibility_state(
        AccessibilityState::default()
    ));
}

#[test]
fn a_composable_reads_what_the_platform_reported() {
    set_platform_accessibility_state(READER_ON);
    let captured = Rc::new(RefCell::new(None));
    {
        let captured = Rc::clone(&captured);
        run_test_composition(move || {
            *captured.borrow_mut() = Some(local_accessibility_state().current());
        });
    }
    set_platform_accessibility_state(AccessibilityState::default());

    assert_eq!(*captured.borrow(), Some(READER_ON));
}

#[test]
fn a_provider_overrides_the_platform_state() {
    set_platform_accessibility_state(AccessibilityState::default());
    let local = local_accessibility_state();
    let captured = Rc::new(RefCell::new(None));
    {
        let captured = Rc::clone(&captured);
        run_test_composition(move || {
            let captured = Rc::clone(&captured);
            let local = local.clone();
            ProvideAccessibilityState(READER_ON, move || {
                *captured.borrow_mut() = Some(local.current());
            });
        });
    }

    assert_eq!(*captured.borrow(), Some(READER_ON));
}
