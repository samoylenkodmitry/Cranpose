use cranpose_ui_graphics::EdgeInsets;

use super::{WindowInsets, local_ime_insets, local_safe_area_insets};

#[test]
fn defaults_to_zero_insets() {
    assert_eq!(
        local_safe_area_insets().default_value(),
        EdgeInsets::default()
    );
}

#[test]
fn ime_insets_default_to_zero() {
    assert_eq!(local_ime_insets().default_value(), EdgeInsets::default());
}

#[test]
fn returns_one_shared_local_per_thread() {
    assert!(local_safe_area_insets() == local_safe_area_insets());
    assert!(local_ime_insets() == local_ime_insets());
    assert!(local_ime_insets() != local_safe_area_insets());
}

#[test]
fn combined_insets_do_not_double_count_overlapping_edges() {
    let combined = WindowInsets {
        safe_area: EdgeInsets::from_components(2.0, 8.0, 4.0, 20.0),
        ime: EdgeInsets::from_components(0.0, 0.0, 6.0, 100.0),
    }
    .combined();
    assert_eq!(combined, EdgeInsets::from_components(2.0, 8.0, 6.0, 100.0));
}

#[test]
fn explicit_window_insets_become_padding() {
    use crate::{Modifier, modifier::ModifierChainHandle};

    let _app_context = crate::render_state::app_context_test_scope();
    let insets = EdgeInsets::from_components(1.0, 2.0, 3.0, 4.0);
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&Modifier::empty().window_insets_padding(insets));
    assert_eq!(handle.resolved_modifiers().padding(), insets);
}
