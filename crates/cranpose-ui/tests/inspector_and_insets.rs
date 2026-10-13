use cranpose_ui::{EdgeInsets, LayoutDirection, Modifier, Modifiers, PaddingValues};

#[test]
fn padding_values_preserve_each_edge() {
    assert_eq!(
        PaddingValues::new(1.0, 2.0, 3.0, 4.0),
        PaddingValues {
            start: 1.0,
            top: 2.0,
            end: 3.0,
            bottom: 4.0,
        }
    );
    assert_eq!(
        PaddingValues::all(6.0),
        PaddingValues::new(6.0, 6.0, 6.0, 6.0)
    );
}

#[test]
fn reading_order_padding_swaps_sides_in_a_right_to_left_layout() {
    let padding = PaddingValues::new(24.0, 2.0, 8.0, 4.0);
    assert_eq!(
        padding.physical(LayoutDirection::Ltr),
        EdgeInsets::from_components(24.0, 2.0, 8.0, 4.0)
    );
    assert_eq!(
        padding.physical(LayoutDirection::Rtl),
        EdgeInsets::from_components(8.0, 2.0, 24.0, 4.0)
    );
}

#[test]
fn merging_takes_the_larger_of_each_edge() {
    let bars = PaddingValues::new(0.0, 56.0, 0.0, 0.0);
    let insets = PaddingValues::new(0.0, 24.0, 0.0, 48.0);
    assert_eq!(insets.max(bars), PaddingValues::new(0.0, 56.0, 0.0, 48.0));
}

#[test]
fn relative_padding_mirrors_with_the_reading_order() {
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
