use std::{cell::Cell, rc::Rc};

use super::*;
use crate::captured_in_composition;

fn large_and_still() -> AccessibilityOptions {
    AccessibilityOptions {
        font_scale: 1.5,
        reduce_motion: true,
        ..AccessibilityOptions::default()
    }
}

#[test]
fn the_platform_options_report_a_change_once_and_mend_a_bad_scale() {
    set_platform_accessibility_options(AccessibilityOptions::default());
    assert!(set_platform_accessibility_options(large_and_still()));
    assert!(!set_platform_accessibility_options(large_and_still()));
    assert_eq!(platform_accessibility_options(), large_and_still());
    let bad_scale = AccessibilityOptions {
        font_scale: 0.0,
        ..AccessibilityOptions::default()
    };
    assert!(set_platform_accessibility_options(bad_scale));
    assert_eq!(platform_accessibility_options().font_scale, 1.0);
}

#[test]
fn a_composable_reads_the_platform_options_and_a_provider_wins_over_them() {
    set_platform_accessibility_options(large_and_still());
    let seen = captured_in_composition(|| local_accessibility_options().current());
    set_platform_accessibility_options(AccessibilityOptions::default());
    assert_eq!(seen, Some(large_and_still()));

    let under_provider = captured_in_composition(|| {
        let slot = Rc::new(Cell::new(None));
        let sink = Rc::clone(&slot);
        let local = local_accessibility_options();
        ProvideAccessibilityOptions(large_and_still(), move || sink.set(Some(local.current())));
        slot.get()
    });
    assert_eq!(under_provider, Some(Some(large_and_still())));
}
