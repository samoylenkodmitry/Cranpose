// A text style's font feature settings reach the shaper, as Compose's
// `TextStyle.fontFeatureSettings` do.

use cranpose_text::{FontFamilyResolver, Typeface};
use cranpose_ui::{
    AppContext, TextLayoutOptions, TextStyle, prepare_text_layout, set_text_fonts,
    text::{AnnotatedString, SpanStyle},
};

// The glyphs DejaVu Sans draws "fi" with in a style of `features`.
fn glyphs(features: Option<&str>) -> usize {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../apps/desktop-demo/assets/leetcodedaily/fonts/DejaVuSans.ttf"
    );
    let dejavu = std::fs::read(path).expect("DejaVu Sans");
    AppContext::new().enter(|| {
        set_text_fonts(FontFamilyResolver::new(Typeface::from_bytes(dejavu).ok()));
        let style = TextStyle {
            span_style: SpanStyle {
                font_feature_settings: features.map(Into::into),
                ..Default::default()
            },
            ..Default::default()
        };
        let prepared = prepare_text_layout(
            &AnnotatedString::from("fi"),
            &style,
            TextLayoutOptions::default(),
            None,
        );
        prepared
            .layout
            .glyph_runs(0)
            .map(|run| run.positions().count())
            .sum()
    })
}

#[test]
fn a_style_that_turns_ligatures_off_draws_each_letter() {
    assert_eq!(glyphs(None), 1);
    assert_eq!(glyphs(Some("liga 0")), 2);
}
