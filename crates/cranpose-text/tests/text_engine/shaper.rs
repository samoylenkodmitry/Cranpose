use cranpose_text::{
    FontFamily, FontFamilyResolver, FontFeature, Itemizer, ShapedGlyph, Shaper, ShapingSpan,
    TextDirection, font_feature_settings,
};

use crate::fonts::{ARABIC_VF, DEJAVU, DEVANAGARI_VF, NOTO_SANS_MERGED, ROBOTO, span, typeface};

// How the spans of a test shape.
#[derive(Clone, Copy)]
struct Shaping<'a> {
    weight: u16,
    features: &'a [FontFeature],
    language: Option<&'a str>,
}

fn style<'a>(weight: u16, features: &'a [FontFeature], language: Option<&'a str>) -> Shaping<'a> {
    Shaping {
        weight,
        features,
        language,
    }
}

// The glyphs of `text` split into style spans at `cuts`, every span in
// `shaping`, item after item.
fn shape(
    resolver: &FontFamilyResolver,
    text: &str,
    cuts: &[usize],
    shaping: Shaping<'_>,
) -> Vec<ShapedGlyph> {
    let family = FontFamily::Default;
    let mut bounds = vec![0];
    bounds.extend_from_slice(cuts);
    bounds.push(text.len());
    let spans: Vec<ShapingSpan<'_>> = bounds
        .windows(2)
        .map(|pair| ShapingSpan {
            features: shaping.features,
            language: shaping.language,
            ..span(pair[0]..pair[1], &family, shaping.weight)
        })
        .collect();
    let mut shaper = Shaper::default();
    let mut glyphs = Vec::new();
    Itemizer::default().itemize(text, TextDirection::Content, &spans, resolver, |item| {
        shaper.shape(text, &item, &spans[item.span], 0..text.len(), &mut glyphs);
    });
    glyphs
}

fn ids(glyphs: &[ShapedGlyph]) -> Vec<u32> {
    glyphs.iter().map(|glyph| glyph.id.0).collect()
}

#[test]
fn a_ligature_forms_unless_its_feature_is_off() {
    let resolver = FontFamilyResolver::new([typeface(DEJAVU)]);
    let off: Vec<FontFeature> = font_feature_settings("liga 0").collect();

    assert_eq!(shape(&resolver, "fi", &[], style(400, &[], None)).len(), 1);
    assert_eq!(shape(&resolver, "fi", &[], style(400, &off, None)).len(), 2);
}

// How far the glyphs of `text` move the pen, in font units.
fn advance(resolver: &FontFamilyResolver, text: &str, features: &[FontFeature]) -> i32 {
    shape(resolver, text, &[], style(400, features, None))
        .iter()
        .map(|glyph| glyph.advance)
        .sum()
}

#[test]
fn a_kerned_pair_moves_the_pen_less_than_its_glyphs_apart_unless_kerning_is_off() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let off: Vec<FontFeature> = font_feature_settings("kern 0").collect();
    let apart = advance(&resolver, "A", &[]) + advance(&resolver, "V", &[]);

    assert!(advance(&resolver, "AV", &[]) < apart);
    assert_eq!(advance(&resolver, "AV", &off), apart);
    assert_eq!(
        advance(&resolver, "nn", &[]),
        2 * advance(&resolver, "n", &[]),
        "a pair the font does not kern"
    );
}

#[test]
fn arabic_joins_across_style_spans_and_runs_right_to_left() {
    let resolver = FontFamilyResolver::new([typeface(ARABIC_VF)]);
    let word = "سلام";
    let whole = shape(&resolver, word, &[], style(400, &[], None));
    let split = shape(&resolver, word, &["س".len()], style(400, &[], None));
    let by_cluster = |glyphs: &[ShapedGlyph]| {
        let mut pairs: Vec<(u32, u32)> = glyphs
            .iter()
            .map(|glyph| (glyph.cluster, glyph.id.0))
            .collect();
        pairs.sort_unstable();
        pairs
    };

    // Seen and lam take their joined forms across the span boundary between
    // them, from the shaping context.
    assert_eq!(by_cluster(&split), by_cluster(&whole));
    // Drawn from left to right, the last letter comes first.
    assert!(
        whole
            .windows(2)
            .all(|pair| pair[0].cluster > pair[1].cluster)
    );
    let isolated: Vec<u32> = word
        .chars()
        .flat_map(|letter| {
            ids(&shape(
                &resolver,
                &letter.to_string(),
                &[],
                style(400, &[], None),
            ))
        })
        .rev()
        .collect();
    assert_ne!(ids(&whole), isolated);
}

#[test]
fn a_devanagari_conjunct_shapes_into_fewer_glyphs_than_characters() {
    let resolver = FontFamilyResolver::new([typeface(DEVANAGARI_VF)]);

    let glyphs = shape(&resolver, "क्ष", &[], style(400, &[], None));

    assert!(glyphs.len() < "क्ष".chars().count(), "{glyphs:?}");
}

#[test]
fn a_variable_font_read_from_bytes_shapes_at_the_requested_weight() {
    let resolver = FontFamilyResolver::new([typeface(DEVANAGARI_VF)]);
    let advance = |weight| {
        shape(&resolver, "क", &[], style(weight, &[], None))
            .iter()
            .map(|glyph| glyph.advance)
            .sum::<i32>()
    };

    assert!(advance(800) > advance(400));
    assert!(advance(400) > advance(100));
}

#[test]
fn the_language_picks_local_forms() {
    let resolver = FontFamilyResolver::new([typeface(NOTO_SANS_MERGED)]);

    // Serbian and Russian draw the small be differently.
    assert_ne!(
        ids(&shape(&resolver, "б", &[], style(400, &[], Some("sr")))),
        ids(&shape(&resolver, "б", &[], style(400, &[], Some("ru")))),
    );
}

// One item to shape: its text, the paragraph's direction, the language and
// the features.
type Turn<'a> = (&'a str, TextDirection, Option<&'a str>, &'a [FontFeature]);

// The glyphs of each text, shaped in turn by one shaper.
fn shape_in_turn(resolver: &FontFamilyResolver, turns: &[Turn<'_>]) -> Vec<Vec<ShapedGlyph>> {
    let family = FontFamily::Default;
    let mut shaper = Shaper::default();
    turns
        .iter()
        .map(|&(text, direction, language, features)| {
            let spans = [ShapingSpan {
                language,
                features,
                ..span(0..text.len(), &family, 400)
            }];
            let mut glyphs = Vec::new();
            Itemizer::default().itemize(text, direction, &spans, resolver, |item| {
                shaper.shape(text, &item, &spans[item.span], 0..text.len(), &mut glyphs);
            });
            glyphs
        })
        .collect()
}

#[test]
fn an_item_shapes_alike_whatever_its_shaper_shaped_before() {
    let no_ligatures: Vec<FontFeature> = font_feature_settings("liga 0").collect();
    let (ltr, rtl) = (TextDirection::Ltr, TextDirection::Rtl);
    // Pairs in one typeface that ask for other plans: another language,
    // direction or feature setting.
    let cases: [(&str, [Turn<'_>; 2]); 4] = [
        (
            NOTO_SANS_MERGED,
            [("б", ltr, None, &[]), ("б", ltr, Some("sr"), &[])],
        ),
        (ROBOTO, [("!?", rtl, None, &[]), ("!?", ltr, None, &[])]),
        (ROBOTO, [("!?", ltr, None, &[]), ("!?", rtl, None, &[])]),
        (
            DEJAVU,
            [("fi", ltr, None, &[]), ("fi", ltr, None, &no_ligatures)],
        ),
    ];
    for (path, turns) in cases {
        let resolver = FontFamilyResolver::new([typeface(path)]);
        let apart: Vec<_> = turns
            .iter()
            .flat_map(|turn| shape_in_turn(&resolver, &[*turn]))
            .collect();
        assert_ne!(apart[0], apart[1], "{path}: {turns:?}");

        assert_eq!(shape_in_turn(&resolver, &turns), apart, "{path}: {turns:?}");
    }
}
