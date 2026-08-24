use std::sync::Arc;

use cranpose_foundation::{
    modifier_element, text::TextFieldState, BasicModifierNodeContext, ModifierNodeChain,
};
use cranpose_ui::{collect_modifier_slices, Color, SpanStyle, TextFieldElement, TextStyle};

fn colored_style(color: Color) -> TextStyle {
    TextStyle::from_span_style(SpanStyle {
        color: Some(color),
        ..SpanStyle::default()
    })
}

#[test]
fn text_field_modifier_slices_refresh_style_when_state_identity_is_reused() {
    let _runtime = cranpose_core::Runtime::new(Arc::new(cranpose_core::DefaultScheduler));
    let state = TextFieldState::new("hello world");
    let dark_style = colored_style(Color::from_rgba_u8(228, 240, 252, 255));
    let light_style = colored_style(Color::from_rgba_u8(14, 58, 96, 255));
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    chain.update_from_slice(
        &[modifier_element(TextFieldElement::new(state, dark_style))],
        &mut context,
    );
    chain.update_from_slice(
        &[modifier_element(TextFieldElement::new(
            state,
            light_style.clone(),
        ))],
        &mut context,
    );

    let slices = collect_modifier_slices(&chain);

    assert_eq!(slices.text_content(), Some("hello world"));
    assert_eq!(slices.text_style(), Some(&light_style));
}
