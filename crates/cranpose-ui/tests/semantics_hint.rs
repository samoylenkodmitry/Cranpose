use cranpose_ui::{Box, BoxSpec, Modifier, Size};

use crate::{directional_layout::tagged, text_measured_layout_integration::layout_composition};

#[test]
fn a_hint_reaches_the_node_semantics() {
    let mut composition = cranpose_ui::run_test_composition(|| {
        Box(
            Modifier::empty()
                .size_points(40.0, 40.0)
                .content_description("Color")
                .hint_text("double tap to choose a color"),
            BoxSpec::default(),
            || {},
        );
    });
    let layout = layout_composition(&mut composition, Size::new(100.0, 100.0));
    let hint = tagged(layout.root(), "Color")
        .node_data
        .semantics()
        .and_then(|config| config.hint_text.clone());
    assert_eq!(hint.as_deref(), Some("double tap to choose a color"));
}
