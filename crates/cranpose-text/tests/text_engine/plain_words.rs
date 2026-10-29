// Words whose glyphs no lookup of the font touches skip shaping; they must
// come out as HarfRust shapes them.

use cranpose_text::{FontFeature, Shaper, font_feature_settings};

use crate::{
    fonts::{DEJAVU, DEVANAGARI_VF, MONASPACE_VF, NOTO_SANS_MERGED, ROBOTO},
    harfrust_oracle::{REGULAR, Style, compare},
};

const BOLD: Style<'static> = Style {
    weight: 700,
    features: &[],
};

const CASES: [(&str, Style<'static>, &str); 11] = [
    // No default lookup of Noto Sans starts at the digits but the zero,
    // which a variation selector after it turns slashed.
    (
        NOTO_SANS_MERGED,
        REGULAR,
        "Reading 42 1234567890 +3.14% x2 0\u{FE00}9 (78)",
    ),
    // DejaVu carries a kern table, which keeps its words shaping.
    (DEJAVU, REGULAR, "100 200 (300) 4.5 Tea"),
    // Roboto's short stroke overlay is no mark to its glyph classes and no
    // lookup starts at it, yet it joins the digit before it.
    (ROBOTO, REGULAR, "Frame 1234 Tea 56% 7\u{335}"),
    // Roboto kerns the space with these two letters, which no lookup
    // starts at.
    (ROBOTO, REGULAR, "\u{4B4} \u{4B4}\u{4B4} \u{A68A}"),
    // Monaspace's contextual alternates start at some letters only.
    (MONASPACE_VF, REGULAR, "ABC 2345 689 xyz {a} [b] mmm iii"),
    // A variable instance: the advances come from its variations. Digits
    // next to Devanagari take its script and shaper.
    (DEVANAGARI_VF, BOLD, "(0123) 456.789"),
    (DEVANAGARI_VF, BOLD, "(0123) 456.789 नमस्ते 42"),
    // A run without spaces does not split into words.
    (NOTO_SANS_MERGED, REGULAR, "1234567890"),
    // Ligatures that start at a glyph no other lookup starts at: tone
    // letters join into contours, and Monaspace joins dots and colons.
    (
        NOTO_SANS_MERGED,
        REGULAR,
        "\u{2E5}\u{2E9} \u{2E5}\u{2E5}\u{2E5} \u{2E7}",
    ),
    (MONASPACE_VF, REGULAR, "a.. :: x::y ... 2:3"),
    // Signs beyond ASCII stand alone; an enclosing circle and an accent
    // join the digit before them.
    (
        NOTO_SANS_MERGED,
        REGULAR,
        "\u{2116}5 \u{B1}3 \u{D7}2 \u{2030} \u{2014} 4\u{20DD} 7\u{301} \u{BD}",
    ),
];

#[test]
fn words_of_plain_glyphs_shape_as_the_whole_run_shapes() {
    let mut shaper = Shaper::default();
    // The second round shapes words the word cache keeps.
    for _ in 0..2 {
        for (path, style, text) in CASES {
            let (shaped, oracle) = compare(&mut shaper, path, text, style);
            assert_eq!(shaped, oracle, "{path}: {text}");
        }
    }
}

#[test]
fn a_feature_that_reaches_plain_glyphs_shapes_them() {
    let oldstyle: Vec<FontFeature> = font_feature_settings("onum").collect();
    let style = Style {
        weight: 400,
        features: &oldstyle,
    };
    let text = "2026 13:45";
    let mut shaper = Shaper::default();
    let (shaped, oracle) = compare(&mut shaper, NOTO_SANS_MERGED, text, style);
    assert_eq!(shaped, oracle);
    let (lining, _) = compare(&mut shaper, NOTO_SANS_MERGED, text, REGULAR);
    let ids = |glyphs: &[cranpose_text::ShapedGlyph]| -> Vec<u32> {
        glyphs.iter().map(|glyph| glyph.id.0).collect()
    };
    assert_ne!(
        ids(&shaped),
        ids(&lining),
        "oldstyle figures are other glyphs"
    );
}
