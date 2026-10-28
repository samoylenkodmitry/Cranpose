use cranpose_text::{FontFamily, FontFamilyResolver, Itemizer, ShapingSpan, TextDirection};

use crate::fonts::{ARABIC_VF, DEJAVU, DEVANAGARI_VF, ROBOTO, span, typeface};

// Each item of `text` as its text, level, script, first family name and span.
fn items<'t>(
    itemizer: &mut Itemizer,
    resolver: &FontFamilyResolver,
    text: &'t str,
    direction: TextDirection,
    spans: &[ShapingSpan<'_>],
) -> Vec<(&'t str, u8, [u8; 4], String, usize)> {
    let mut items = Vec::new();
    itemizer.itemize(text, direction, spans, resolver, |item| {
        items.push((
            text.get(item.range)
                .expect("items end on character boundaries"),
            item.level.value(),
            item.script.tag(),
            item.typeface.family_names()[0].clone(),
            item.span,
        ));
    });
    items
}

fn whole<'f>(text: &str, family: &'f FontFamily) -> [ShapingSpan<'f>; 1] {
    [span(0..text.len(), family, 400)]
}

const DEVANAGARI_NAME: &str = "CranScan UI Devanagari";
const ARABIC_NAME: &str = "CranScan UI Arabic";

#[test]
fn scripts_and_fonts_split_left_to_right_text() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO), typeface(DEVANAGARI_VF)]);
    let mut itemizer = Itemizer::default();
    let default = FontFamily::Default;
    let mut items = |text| {
        items(
            &mut itemizer,
            &resolver,
            text,
            TextDirection::Ltr,
            &whole(text, &default),
        )
    };

    assert_eq!(
        items("Hello, world!"),
        [("Hello, world!", 0, *b"Latn", "Roboto".into(), 0)]
    );
    assert_eq!(
        items("Hello नमस्ते"),
        [
            ("Hello ", 0, *b"Latn", "Roboto".into(), 0),
            ("नमस्ते", 0, *b"Deva", DEVANAGARI_NAME.into(), 0),
        ]
    );
}

#[test]
fn a_closing_bracket_takes_the_script_of_its_opening_bracket() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO), typeface(DEVANAGARI_VF)]);
    let text = "नमस्ते (abc) नमस्ते";
    let default = FontFamily::Default;

    assert_eq!(
        items(
            &mut Itemizer::default(),
            &resolver,
            text,
            TextDirection::Ltr,
            &whole(text, &default)
        ),
        [
            ("नमस्ते ", 0, *b"Deva", DEVANAGARI_NAME.into(), 0),
            ("(", 0, *b"Deva", "Roboto".into(), 0),
            ("abc", 0, *b"Latn", "Roboto".into(), 0),
            (") ", 0, *b"Deva", "Roboto".into(), 0),
            ("नमस्ते", 0, *b"Deva", DEVANAGARI_NAME.into(), 0),
        ]
    );
}

#[test]
fn right_to_left_text_and_its_numbers_take_their_bidi_levels() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO), typeface(ARABIC_VF)]);
    let text = "abc مرحبا 123";
    let default = FontFamily::Default;

    // The space between the Arabic word and the digits resolves to the
    // right-to-left level; the digits, Arabic numbers after Arabic letters,
    // go two levels up.
    assert_eq!(
        items(
            &mut Itemizer::default(),
            &resolver,
            text,
            TextDirection::Ltr,
            &whole(text, &default)
        ),
        [
            ("abc ", 0, *b"Latn", "Roboto".into(), 0),
            ("مرحبا ", 1, *b"Arab", ARABIC_NAME.into(), 0),
            ("123", 2, *b"Arab", "Roboto".into(), 0),
        ]
    );
}

#[test]
fn each_paragraph_takes_its_own_direction_from_its_content() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO), typeface(ARABIC_VF)]);
    let mut itemizer = Itemizer::default();
    let default = FontFamily::Default;
    let text = "مرحبا\nabc";

    assert_eq!(
        items(
            &mut itemizer,
            &resolver,
            text,
            TextDirection::Content,
            &whole(text, &default)
        ),
        [
            ("مرحبا\n", 1, *b"Arab", ARABIC_NAME.into(), 0),
            ("abc", 0, *b"Latn", "Roboto".into(), 0),
        ]
    );
    assert_eq!(
        items(
            &mut itemizer,
            &resolver,
            "abc",
            TextDirection::Rtl,
            &whole("abc", &default)
        ),
        [("abc", 2, *b"Latn", "Roboto".into(), 0)]
    );
}

#[test]
fn style_spans_split_items() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO), typeface(DEJAVU)]);
    let roboto = FontFamily::Named("Roboto".into());
    let dejavu = FontFamily::Named("DejaVu Sans".into());

    assert_eq!(
        items(
            &mut Itemizer::default(),
            &resolver,
            "abcd",
            TextDirection::Ltr,
            &[span(0..2, &roboto, 400), span(2..4, &dejavu, 400)],
        ),
        [
            ("ab", 0, *b"Latn", "Roboto".into(), 0),
            ("cd", 0, *b"Latn", "DejaVu Sans".into(), 1),
        ]
    );
}
