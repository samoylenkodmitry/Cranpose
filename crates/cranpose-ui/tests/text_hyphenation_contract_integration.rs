use cranpose_ui::{
    AppContext, ParagraphStyle, SpanStyle, TextLayoutOptions, TextOverflow, TextStyle,
    prepare_text_layout, set_text_measurer,
    text::{Hyphens, TextUnit},
};

use crate::text_contract_measurer::ContractMeasurer;

#[test]
fn prepare_text_layout_uses_measurer_hyphen_contract() {
    let app_context = AppContext::new();

    let style = TextStyle {
        span_style: SpanStyle {
            font_size: TextUnit::Sp(10.0),
            ..Default::default()
        },
        paragraph_style: ParagraphStyle {
            hyphens: Hyphens::Auto,
            ..Default::default()
        },
    };
    let options = TextLayoutOptions {
        overflow: TextOverflow::Clip,
        soft_wrap: true,
        max_lines: usize::MAX,
        min_lines: 1,
    };

    let prepared = app_context.enter(|| {
        set_text_measurer(ContractMeasurer);
        prepare_text_layout(
            &cranpose_ui::text::AnnotatedString::from("Transformation"),
            &style,
            options,
            Some(24.0),
        )
    });
    assert_eq!(
        prepared.text.text.lines().collect::<Vec<_>>(),
        vec!["Tra", "nsf", "orm", "ati", "on"]
    );
}
