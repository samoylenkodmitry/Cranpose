//! Reading-order padding and the keyboard's platform-shaped modifier key.
//!
//! Two rules that only misbehave somewhere the author was not looking: a
//! padding that is correct in English and mirrored in Arabic, and a shortcut
//! that works on Linux and not on a Mac.

use cranpose_ui::{widgets::scaffold::PaddingValues, LayoutDirection, Modifier, Modifiers};

#[test]
fn relative_padding_mirrors_with_the_reading_order() {
    // Start-and-end padding exists precisely so that a right-to-left layout
    // does not need its own values. If both directions produce the same
    // modifier, that mirroring is not happening.
    let padding = PaddingValues::new(8.0, 0.0, 0.0, 0.0);
    let ltr = padding.apply_to_in(Modifier::empty(), LayoutDirection::Ltr);
    let rtl = padding.apply_to_in(Modifier::empty(), LayoutDirection::Rtl);
    assert_ne!(
        ltr, rtl,
        "8 of start padding landed on the same edge in both reading orders"
    );
}

#[test]
fn symmetric_padding_is_the_same_in_both_reading_orders() {
    let padding = PaddingValues::all(8.0);
    assert_eq!(
        padding.apply_to_in(Modifier::empty(), LayoutDirection::Ltr),
        padding.apply_to_in(Modifier::empty(), LayoutDirection::Rtl),
        "padding that is equal on every edge cannot depend on reading order"
    );
}

#[test]
fn the_command_key_is_whichever_key_the_platform_calls_command() {
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };
    let meta = Modifiers {
        meta: true,
        ..Modifiers::NONE
    };

    if cfg!(target_os = "macos") {
        assert!(meta.command_or_ctrl(), "Cmd is the shortcut key on macOS");
        assert!(!ctrl.command_or_ctrl());
    } else {
        assert!(ctrl.command_or_ctrl(), "Ctrl is the shortcut key elsewhere");
        assert!(!meta.command_or_ctrl());
    }

    assert!(
        !Modifiers::NONE.command_or_ctrl(),
        "no modifier held is not the shortcut key"
    );
}
