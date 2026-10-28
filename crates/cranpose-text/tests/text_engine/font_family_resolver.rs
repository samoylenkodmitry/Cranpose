use cranpose_text::{FontFamily, FontFamilyResolver, FontRequest, FontStyle, FontWeight, Typeface};

use crate::fonts::{
    DEJAVU, DEJAVU_MONO, DEVANAGARI, DEVANAGARI_VF, NOTO_SANS_LATIN, NOTO_SANS_MERGED, ROBOTO,
    ROBOTO_BOLD, ROBOTO_MEDIUM, TWEMOJI, bytes, typeface,
};

fn request(family: &FontFamily, weight: u16, style: FontStyle) -> FontRequest<'_> {
    FontRequest {
        family,
        weight: FontWeight::new(weight),
        style,
    }
}

fn plain(family: &FontFamily) -> FontRequest<'_> {
    request(family, 400, FontStyle::Normal)
}

// Each run of `text` with the first family name of its typeface.
fn runs<'a>(
    resolver: &FontFamilyResolver,
    text: &'a str,
    request: FontRequest<'_>,
) -> Vec<(&'a str, String)> {
    let mut runs = Vec::new();
    resolver.runs(text, request, |range, typeface| {
        runs.push((&text[range], typeface.family_names()[0].clone()));
    });
    runs
}

fn registered(family: &FontFamily, weight: u16, style: FontStyle) -> Typeface {
    Typeface::from_registered(family, FontWeight::new(weight), style, bytes(ROBOTO), &[])
        .expect("Roboto reads")
}

#[test]
fn weights_resolve_as_compose_matches_them() {
    let family = FontFamily::Named("Custom".into());
    let resolver = FontFamilyResolver::new(
        [300, 500, 700].map(|weight| registered(&family, weight, FontStyle::Normal)),
    );
    let resolved = |weight| {
        resolver
            .resolve(request(&family, weight, FontStyle::Normal))
            .map(|typeface| typeface.weight().value())
    };

    // Below 400 lighter weights come first, then heavier ones.
    assert_eq!(resolved(350), Some(300));
    assert_eq!(resolved(200), Some(300));
    // From 400 to 500 heavier weights up to 500 come first, then lighter.
    assert_eq!(resolved(450), Some(500));
    assert_eq!(resolved(400), Some(500));
    // Above 500 heavier weights come first, then lighter ones.
    assert_eq!(resolved(600), Some(700));
    assert_eq!(resolved(800), Some(700));
}

#[test]
fn between_400_and_500_lighter_weights_come_before_those_above_500() {
    let family = FontFamily::Named("Custom".into());
    let resolver = FontFamilyResolver::new(
        [300, 600].map(|weight| registered(&family, weight, FontStyle::Normal)),
    );

    let resolved = resolver.resolve(request(&family, 420, FontStyle::Normal));

    assert_eq!(
        resolved.map(|typeface| typeface.weight().value()),
        Some(300)
    );
}

#[test]
fn the_requested_style_wins_over_the_weight() {
    let family = FontFamily::Named("Custom".into());
    let resolver = FontFamilyResolver::new([
        registered(&family, 400, FontStyle::Normal),
        registered(&family, 700, FontStyle::Italic),
    ]);
    let style = |style| {
        resolver
            .resolve(request(&family, 400, style))
            .map(Typeface::style)
    };

    assert_eq!(style(FontStyle::Italic), Some(FontStyle::Italic));
    assert_eq!(style(FontStyle::Normal), Some(FontStyle::Normal));
}

#[test]
fn a_family_without_the_style_still_draws() {
    let family = FontFamily::Named("Custom".into());
    let resolver = FontFamilyResolver::new([registered(&family, 400, FontStyle::Normal)]);

    let resolved = resolver.resolve(request(&family, 700, FontStyle::Italic));

    assert_eq!(resolved.map(Typeface::style), Some(FontStyle::Normal));
}

#[test]
fn a_variable_font_matches_every_weight_on_its_axis() {
    let fixed = typeface(DEVANAGARI);
    let variable = typeface(DEVANAGARI_VF);
    let (fixed_hash, variable_hash) = (fixed.content_hash(), variable.content_hash());
    let resolver = FontFamilyResolver::new([fixed, variable]);
    let resolved = |weight| {
        resolver
            .resolve(request(&FontFamily::Default, weight, FontStyle::Normal))
            .map(Typeface::content_hash)
    };

    assert_eq!(resolved(400), Some(fixed_hash));
    assert_eq!(resolved(700), Some(variable_hash));
    assert_eq!(resolved(300), Some(variable_hash));
}

#[test]
fn a_named_family_comes_first_and_an_unknown_one_draws_with_the_first() {
    let resolver = FontFamilyResolver::new([
        typeface(DEJAVU),
        typeface(ROBOTO),
        typeface(ROBOTO_MEDIUM),
        typeface(ROBOTO_BOLD),
    ]);
    let family = |family: FontFamily| {
        resolver
            .resolve(plain(&family))
            .map(|typeface| typeface.family_names()[0].clone())
    };

    assert_eq!(
        family(FontFamily::Named("roboto".into())).as_deref(),
        Some("Roboto")
    );
    assert_eq!(
        family(FontFamily::Named("Unknown".into())).as_deref(),
        Some("DejaVu Sans")
    );
    assert_eq!(family(FontFamily::Default).as_deref(), Some("DejaVu Sans"));
    assert_eq!(
        runs(&resolver, "a❤", plain(&FontFamily::Named("Roboto".into()))),
        [("a", "Roboto".into()), ("❤", "DejaVu Sans".into())]
    );
}

#[test]
fn a_registered_generic_family_resolves_to_its_typefaces() {
    let resolver = FontFamilyResolver::new([
        typeface(ROBOTO),
        Typeface::from_registered(
            &FontFamily::Monospace,
            FontWeight::NORMAL,
            FontStyle::Normal,
            bytes(DEJAVU_MONO),
            &[],
        )
        .expect("DejaVu Sans Mono reads"),
    ]);
    let family = |family: FontFamily| {
        resolver
            .resolve(plain(&family))
            .map(|typeface| typeface.family_names()[0].clone())
    };

    assert_eq!(
        family(FontFamily::Monospace).as_deref(),
        Some("DejaVu Sans Mono")
    );
    assert_eq!(family(FontFamily::SansSerif).as_deref(), Some("Roboto"));
}

#[test]
fn text_falls_back_per_grapheme_and_punctuation_stays_with_its_word() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO), typeface(DEVANAGARI_VF)]);

    assert_eq!(
        runs(&resolver, "Hi नमस्ते, ok", plain(&FontFamily::Default)),
        [
            ("Hi ", "Roboto".into()),
            ("नमस्ते,", "CranScan UI Devanagari".into()),
            (" ok", "Roboto".into()),
        ]
    );
    assert_eq!(
        runs(&resolver, "plain text", plain(&FontFamily::Default)),
        [("plain text", "Roboto".into())]
    );
}

#[test]
fn characters_without_a_glyph_of_their_own_keep_the_run() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO), typeface(DEVANAGARI_VF)]);

    // A line break and a character no typeface maps (Thai KO KAI).
    assert_eq!(
        runs(&resolver, "नम\nस्\u{0E01}ते", plain(&FontFamily::Default)),
        [("नम\nस्\u{0E01}ते", "CranScan UI Devanagari".into())]
    );
    assert_eq!(
        runs(&resolver, "\u{0E01}a", plain(&FontFamily::Default)),
        [("\u{0E01}a", "Roboto".into())]
    );
}

#[test]
fn emoji_go_to_color_typefaces_and_text_to_plain_ones() {
    let resolver = FontFamilyResolver::new([typeface(DEJAVU), typeface(TWEMOJI)]);
    let face = |text| runs(&resolver, text, plain(&FontFamily::Default));

    // DejaVu Sans maps both, and comes first.
    assert_eq!(
        face("a😀"),
        [
            ("a", "DejaVu Sans".into()),
            ("😀", "Twemoji Mozilla".into())
        ]
    );
    assert_eq!(face("❤"), [("❤", "DejaVu Sans".into())]);
    assert_eq!(face("❤\u{FE0F}"), [("❤\u{FE0F}", "Twemoji Mozilla".into())]);
    assert_eq!(face("😀\u{FE0E}"), [("😀\u{FE0E}", "DejaVu Sans".into())]);
}

#[test]
fn a_color_typeface_given_first_draws_only_emoji() {
    let resolver = FontFamilyResolver::new([typeface(TWEMOJI), typeface(DEJAVU)]);
    let face = |text| runs(&resolver, text, plain(&FontFamily::Default));

    assert_eq!(face("1"), [("1", "DejaVu Sans".into())]);
    assert_eq!(
        face("1\u{FE0F}\u{20E3}2"),
        [
            ("1\u{FE0F}\u{20E3}", "Twemoji Mozilla".into()),
            ("2", "DejaVu Sans".into())
        ]
    );
}

#[test]
fn typefaces_that_tie_in_one_family_split_the_text_by_coverage() {
    let latin = typeface(NOTO_SANS_LATIN);
    let merged = typeface(NOTO_SANS_MERGED);
    let (latin_hash, merged_hash) = (latin.content_hash(), merged.content_hash());
    let resolver = FontFamilyResolver::new([latin, merged, typeface(DEJAVU)]);
    let mut runs = Vec::new();

    resolver.runs("a❤, b", plain(&FontFamily::Default), |range, typeface| {
        runs.push((range, typeface.content_hash()));
    });

    // Both are Noto Sans at 400; only the second maps the heart, and the
    // comma stays with it.
    assert_eq!(
        runs,
        [(0..1, latin_hash), (1..5, merged_hash), (5..7, latin_hash)]
    );
}
