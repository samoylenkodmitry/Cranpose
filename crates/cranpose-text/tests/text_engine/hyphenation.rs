use cranpose_text::{
    FontFamily, FontFamilyResolver, GlyphId, HyphenationError, Hyphens, LayoutInput, LineBreak,
    ParagraphLengths, ParagraphStyle, TextLayout, TextLayoutOptions, TextLayouter, Typeface,
};

use crate::fonts::{ROBOTO, span, typeface};

// Lays `text` out in Roboto at 16 pixels in the language `language`, with
// `hyphens` and `line_break`, `width` wide.
fn layout(
    resolver: &FontFamilyResolver,
    text: &str,
    (hyphens, line_break): (Hyphens, LineBreak),
    language: Option<&str>,
    width: f32,
) -> TextLayout {
    let family = FontFamily::Default;
    let mut spans = [span(0..text.len(), &family, 400)];
    spans[0].language = language;
    let paragraph = ParagraphStyle {
        hyphens,
        line_break,
        ..ParagraphStyle::default()
    };
    TextLayouter::default().layout(
        &LayoutInput {
            text,
            spans: &spans,
            paragraph: &paragraph,
            lengths: ParagraphLengths::default(),
            min_width: 0.0,
            max_width: width,
            options: TextLayoutOptions::default(),
        },
        resolver,
    )
}

fn unbroken(resolver: &FontFamilyResolver, text: &str) -> f32 {
    let laid = layout(
        resolver,
        text,
        (Hyphens::None, LineBreak::Simple),
        None,
        f32::INFINITY,
    );
    laid.line_right(0) - laid.line_left(0)
}

fn lines<'a>(laid: &TextLayout, text: &'a str) -> Vec<&'a str> {
    (0..laid.line_count())
        .map(|line| {
            text.get(laid.line_start(line)..laid.line_end(line))
                .expect("lines end on boundaries")
        })
        .collect()
}

// The glyph of the hyphen a hyphenated line ends with in `face`.
fn hyphen_glyph(face: &Typeface) -> Option<GlyphId> {
    face.glyph('\u{2010}').or_else(|| face.glyph('-'))
}

// The last glyph line `line` draws, from its right edge.
fn last_glyph(laid: &TextLayout, line: usize) -> Option<GlyphId> {
    laid.glyph_runs(line)
        .flat_map(|run| run.positions().collect::<Vec<_>>())
        .max_by(|a, b| a.x.total_cmp(&b.x))
        .map(|glyph| glyph.id)
}

const AUTO: (Hyphens, LineBreak) = (Hyphens::Auto, LineBreak::Simple);

#[test]
fn a_word_wider_than_its_line_ends_the_line_at_a_hyphenation_point_with_a_hyphen() {
    let roboto = typeface(ROBOTO);
    let hyphen = hyphen_glyph(&roboto);
    let resolver = FontFamilyResolver::new([roboto]);
    let width = unbroken(&resolver, "Understand\u{2010}") + 0.5;

    let laid = layout(&resolver, "Understanding", AUTO, None, width);

    assert_eq!(lines(&laid, "Understanding"), ["Understand", "ing"]);
    assert_eq!(last_glyph(&laid, 0), hyphen);
    let hyphenated = laid.line_right(0) - laid.line_left(0);
    assert!(
        (hyphenated - unbroken(&resolver, "Understand\u{2010}")).abs() < 0.01,
        "{hyphenated}"
    );
}

#[test]
fn greedy_breaking_moves_a_word_to_the_next_line_before_it_hyphenates_it() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let text = "a understanding";
    let width = unbroken(&resolver, "understanding") + 0.5;
    assert!(unbroken(&resolver, "a understand\u{2010}") < width);

    let laid = layout(&resolver, text, AUTO, None, width);

    assert_eq!(lines(&laid, text), ["a ", "understanding"]);
}

#[test]
fn without_hyphens_auto_a_word_wider_than_its_line_breaks_between_clusters() {
    let roboto = typeface(ROBOTO);
    let hyphen = hyphen_glyph(&roboto);
    let resolver = FontFamilyResolver::new([roboto]);
    let width = unbroken(&resolver, "Understand\u{2010}") + 0.5;

    let laid = layout(
        &resolver,
        "Understanding",
        (Hyphens::None, LineBreak::Simple),
        None,
        width,
    );

    assert!(laid.line_count() > 1);
    assert_ne!(last_glyph(&laid, 0), hyphen);
}

#[test]
fn a_word_in_a_language_without_patterns_breaks_between_clusters() {
    let roboto = typeface(ROBOTO);
    let hyphen = hyphen_glyph(&roboto);
    let resolver = FontFamilyResolver::new([roboto]);
    let width = unbroken(&resolver, "Understand\u{2010}") + 0.5;

    let american = layout(&resolver, "Understanding", AUTO, Some("en-US"), width);
    let unknown = layout(&resolver, "Understanding", AUTO, Some("ja"), width);

    assert_eq!(last_glyph(&american, 0), hyphen);
    assert!(unknown.line_count() > 1);
    assert_ne!(last_glyph(&unknown, 0), hyphen);
}

#[test]
fn even_breaking_hyphenates_a_word_when_that_evens_the_lines() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let text = "a understanding";
    let width = unbroken(&resolver, "understanding") + 0.5;
    assert!(unbroken(&resolver, "a understand\u{2010}") < width);

    let plain = layout(
        &resolver,
        text,
        (Hyphens::None, LineBreak::Paragraph),
        None,
        width,
    );
    let hyphenated = layout(
        &resolver,
        text,
        (Hyphens::Auto, LineBreak::Paragraph),
        None,
        width,
    );

    assert_eq!(lines(&plain, text), ["a ", "understanding"]);
    assert_eq!(lines(&hyphenated, text), ["a understand", "ing"]);
}

#[test]
fn a_dictionary_for_a_locale_without_patterns_is_refused() {
    let mut layouter = TextLayouter::default();

    let refused = layouter
        .hyphenator_mut()
        .register_dictionary("xx-unknown", &mut [].as_slice());

    assert!(
        matches!(refused, Err(HyphenationError::UnsupportedLocale(ref tag)) if tag == "xx-unknown")
    );
}
