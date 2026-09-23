//! What the platform says about the assistive technology in use, for an app
//! that speaks guidance, slows an automatic step down or drops a gesture-only
//! path while a screen reader is on.

use std::cell::{Cell, RefCell};

use cranpose_core::{CompositionLocal, CompositionLocalProvider, compositionLocalOf};
use cranpose_macros::composable;

/// What the platform reports about the assistive technology in use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct AccessibilityState {
    /// Whether a screen reader is on: VoiceOver on iOS, a service such as
    /// TalkBack on Android, or a reader that connected to the app through
    /// accesskit on the desktop. The web has no such signal and says no.
    pub screen_reader_on: bool,
}

thread_local! {
    static PLATFORM_ACCESSIBILITY_STATE: Cell<AccessibilityState> =
        const { Cell::new(AccessibilityState { screen_reader_on: false }) };
}

/// Installs what the platform reports. A backend calls this on every check
/// and forces a root render when the answer is true, so composition reads
/// the new state.
pub fn set_platform_accessibility_state(state: AccessibilityState) -> bool {
    PLATFORM_ACCESSIBILITY_STATE.with(|cell| {
        let changed = cell.get() != state;
        cell.set(state);
        changed
    })
}

/// What the platform last reported.
pub fn platform_accessibility_state() -> AccessibilityState {
    PLATFORM_ACCESSIBILITY_STATE.with(Cell::get)
}

/// The state a composable reads: what the platform reported, unless a
/// [`ProvideAccessibilityState`] above it says otherwise.
pub fn local_accessibility_state() -> CompositionLocal<AccessibilityState> {
    thread_local! {
        static LOCAL: RefCell<Option<CompositionLocal<AccessibilityState>>> =
            const { RefCell::new(None) };
    }

    LOCAL.with(|cell| {
        let mut local = cell.borrow_mut();
        local
            .get_or_insert_with(|| compositionLocalOf(platform_accessibility_state))
            .clone()
    })
}

/// Gives the content below it a fixed state, for a preview or a test that
/// wants to see the app as a screen reader user does.
#[composable]
pub fn ProvideAccessibilityState(state: AccessibilityState, content: impl FnOnce()) {
    let local = local_accessibility_state();
    CompositionLocalProvider(vec![local.provides(state)], move || {
        content();
    });
}

#[cfg(test)]
mod tests {
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
            let local = local.clone();
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
}
