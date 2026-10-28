use cranpose_text::{FontFamily, FontStyle, FontWeight, Typeface, TypefaceError};

use crate::fonts::{DEVANAGARI_VF, ROBOTO, ROBOTO_BOLD, TWEMOJI, bytes, typeface};

fn registered(weight: FontWeight, variations: &[([u8; 4], f32)]) -> Typeface {
    Typeface::from_registered(
        &FontFamily::Named("Hindi".into()),
        weight,
        FontStyle::Normal,
        bytes(DEVANAGARI_VF),
        variations,
    )
    .expect("the Devanagari font reads")
}

#[test]
fn a_font_read_without_a_declaration_takes_its_own_names_weight_and_style() {
    let bold = typeface(ROBOTO_BOLD);

    assert_eq!(bold.family_names(), ["Roboto"]);
    assert_eq!(bold.weight(), FontWeight::BOLD);
    assert_eq!(bold.style(), FontStyle::Normal);
    assert_eq!(bold.registered_family(), None);
    assert!(bold.variations().is_empty());
}

#[test]
fn bytes_that_are_no_font_fail() {
    assert_eq!(
        Typeface::from_bytes(b"not a font".to_vec()).err(),
        Some(TypefaceError::InvalidFont)
    );
}

#[test]
fn a_declared_weight_instances_the_weight_axis_within_its_range() {
    let regular = registered(FontWeight::NORMAL, &[]);
    let bold = registered(FontWeight::BOLD, &[]);
    let heaviest = registered(FontWeight::new(1000), &[]);
    let ka = regular.glyph('क').expect("the font has KA");

    assert_eq!(regular.variations(), [(*b"wght", 400.0)]);
    assert_eq!(bold.variations(), [(*b"wght", 700.0)]);
    assert_eq!(heaviest.variations(), [(*b"wght", 900.0)]);
    assert!(bold.advance(ka) > regular.advance(ka));
    assert_eq!(
        bold.registered_family(),
        Some(cranpose_text::FontFamilyKey::of(&FontFamily::Named(
            "Hindi".into()
        )))
    );
}

#[test]
fn an_explicit_variation_wins_over_the_declared_weight() {
    let declared = registered(FontWeight::BOLD, &[(*b"wght", 300.0)]);

    assert_eq!(declared.variations(), [(*b"wght", 300.0)]);
    assert_eq!(
        declared.content_hash(),
        registered(FontWeight::LIGHT, &[]).content_hash()
    );
}

#[test]
fn a_variation_the_font_cannot_take_fails() {
    for variation in [(*b"wdth", 100.0), (*b"wght", 950.0), (*b"wght", f32::NAN)] {
        let result = Typeface::from_registered(
            &FontFamily::Default,
            FontWeight::NORMAL,
            FontStyle::Normal,
            bytes(DEVANAGARI_VF),
            &[variation],
        );
        assert_eq!(
            result.err(),
            Some(TypefaceError::InvalidVariation { tag: variation.0 }),
            "{variation:?}"
        );
    }
}

#[test]
fn the_content_hash_tells_fonts_and_instances_apart() {
    assert_eq!(
        typeface(ROBOTO).content_hash(),
        typeface(ROBOTO).content_hash()
    );
    assert_ne!(
        typeface(ROBOTO).content_hash(),
        typeface(ROBOTO_BOLD).content_hash()
    );
    assert_ne!(
        registered(FontWeight::NORMAL, &[]).content_hash(),
        registered(FontWeight::BOLD, &[]).content_hash()
    );
}

#[test]
fn coverage_and_color_come_from_the_font() {
    let roboto = typeface(ROBOTO);
    let emoji = typeface(TWEMOJI);

    assert!(roboto.covers('A') && !roboto.covers('क') && !roboto.covers('😀'));
    assert!(emoji.covers('😀') && !emoji.covers('A'));
    assert!(emoji.is_color() && !roboto.is_color());
}

#[test]
fn line_metrics_come_from_the_horizontal_header() {
    let metrics = typeface(ROBOTO).line_metrics();

    assert!(metrics.ascent > 0.0 && metrics.descent < 0.0 && metrics.line_gap >= 0.0);
    assert_eq!(typeface(ROBOTO).units_per_em(), 2048);
}
