// Words whose glyphs no lookup of the font touches skip shaping; they must
// come out as HarfRust shapes them.

use cranpose_text::{FontFeature, Shaper, font_feature_settings};

use crate::{
    fonts::{
        DEJAVU, DEJAVU_MONO, DEVANAGARI_VF, MONASPACE_VF, NOTO_SANS_MERGED, ROBOTO, TEST_CONTEXTS,
        TEST_KERNS,
    },
    harfrust_oracle::{Placed, REGULAR, Style, compare},
};

const BOLD: Style<'static> = Style {
    weight: 700,
    features: &[],
};

const CASES: [(&str, Style<'static>, &str); 17] = [
    // No default lookup of Noto Sans starts at the digits but the zero,
    // which a variation selector after it turns slashed.
    (
        NOTO_SANS_MERGED,
        REGULAR,
        "Reading 42 1234567890 +3.14% x2 0\u{FE00}9 (78)",
    ),
    // Pair adjustments that move only the first glyph's advance kern plain
    // glyphs: letters, the period, the comma and the hyphen.
    (
        NOTO_SANS_MERGED,
        REGULAR,
        "AVATAR Toyota 123.45 -0.50% +1.25% T. V, Y: Layer 7 We",
    ),
    (ROBOTO, REGULAR, "AVA Tea Wo 1.5 -2,3 Yes T. P, F."),
    // Noto Sans drops the dot of an `i` or a `j` under an accent above:
    // the rule starts at them and reads the mark after them.
    (
        NOTO_SANS_MERGED,
        REGULAR,
        "Reading jinx minimum ij i\u{301} j\u{307}x i\u{328}\u{301} fi",
    ),
    // Characters whose code points share their low bits take turns in one
    // place of what a plan remembers: U+0410 and U+0490, `A` and `\u{C1}`;
    // a character that takes the place of the one before it pairs with it
    // once: U+0490 and U+0410 kern unlike two U+0410.
    (
        NOTO_SANS_MERGED,
        REGULAR,
        "\u{490}\u{410}\u{490} \u{410}\u{490}\u{410} A\u{C1}A \u{C1}AV \u{490}\u{410} \u{410}\u{410} F\u{C6} \u{C6}\u{C6} X\u{D8} \u{D8}\u{D8}",
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
    // Its texture healing widens a wide letter beside a narrow one: a
    // narrow letter before a wide one, after it, and a wide one beside
    // brackets and a period.
    (MONASPACE_VF, REGULAR, "mi im (m) m. Wi iW 1M lm fit"),
    // DejaVu Sans Mono's rules read classes of glyphs.
    (DEJAVU_MONO, REGULAR, "fi fl ffi 1/2 -> => != <= a.b"),
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
fn pairs_kerned_by_more_values_than_a_pair_cell_names_shape_as_the_whole_run_shapes() {
    // Cranpose Test Kerns kerns each pair of letters by a value of its own,
    // more values than the cells name: every pair, twice, the second round
    // reading the kerning the first kept.
    let letters: Vec<char> = ('a'..='z').chain('A'..='Z').collect();
    let text: String = letters
        .iter()
        .flat_map(|&first| letters.iter().flat_map(move |&second| [first, second, ' ']))
        .collect();
    let mut shaper = Shaper::default();
    for _ in 0..2 {
        let (shaped, oracle) = compare(&mut shaper, TEST_KERNS, &text, REGULAR);
        assert_eq!(shaped, oracle);
    }
}

#[test]
fn a_character_in_the_slot_of_another_kerns_as_itself() {
    // The quote and the greater-than sign take the slots of the Cyrillic Te
    // and o, a multiple of 128 code points above them; Te kerns with o, the
    // others not as Te and o do.
    let mut shaper = Shaper::default();
    for text in ["То", "\"о", "То", "Т>", "То"] {
        let (shaped, oracle) = compare(&mut shaper, NOTO_SANS_MERGED, text, REGULAR);
        assert_eq!(shaped, oracle, "{text}");
    }
}

#[test]
fn a_contextual_rule_shapes_the_glyphs_it_reads() {
    // Each rule of Cranpose Test Contexts gives the letters it reads wider
    // alternates: the words of each kind of rule, then words beside them
    // that no rule reads.
    // A rule that skips ligatures reads past the one U+FB01 maps to.
    let read = "ab cd ef g hi jk lm pn oq rs tu vw xy zA z\u{FB01}A Bc Z9";
    let unread = "ac ce fe hj kj ln pm qo tv vx xz B BC";
    let mut shaper = Shaper::default();
    for (text, takes_alternates) in [(read, true), (unread, false)] {
        let (shaped, oracle) = compare(&mut shaper, TEST_CONTEXTS, text, REGULAR);
        assert_eq!(shaped, oracle, "{text}");
        for word in text.split(' ') {
            let (shaped, oracle) = compare(&mut shaper, TEST_CONTEXTS, word, REGULAR);
            assert_eq!(shaped, oracle, "{word}");
            let widened = oracle.iter().any(|glyph| glyph.advance > 500);
            assert_eq!(widened, takes_alternates, "{word}");
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
    let ids = |glyphs: &[Placed]| -> Vec<u32> { glyphs.iter().map(|glyph| glyph.id).collect() };
    assert_ne!(
        ids(&shaped),
        ids(&lining),
        "oldstyle figures are other glyphs"
    );
}

#[test]
fn plans_shape_as_harfrust_after_plans_with_plain_glyphs_of_others_leave() {
    // A face keeps eight plans. With kerning off the first plan's plain
    // glyphs are its own; the plans of features Noto Sans lacks share the
    // default plan's. The ninth plan pushes the first out and the tenth,
    // kerning off again, the default one, and the plans left still kern as
    // HarfRust kerns.
    let settings: Vec<Vec<FontFeature>> = [
        "kern off", "", "zz01", "zz02", "zz03", "zz04", "zz05", "zz06", "zz07", "kern off",
    ]
    .iter()
    .map(|settings| font_feature_settings(settings).collect())
    .collect();
    let text = "AVATAR Toyota 42";
    let mut shaper = Shaper::default();
    for features in settings
        .iter()
        .chain(&settings[2..9])
        .chain(&settings[1..2])
    {
        let style = Style {
            weight: 400,
            features,
        };
        let (shaped, oracle) = compare(&mut shaper, NOTO_SANS_MERGED, text, style);
        assert_eq!(shaped, oracle, "{features:?}");
    }
}

#[test]
fn a_plan_whose_lookups_leave_other_glyphs_plain_keeps_plain_glyphs_of_its_own() {
    // Each second plan differs from the first in one way: oldstyle figures
    // start lookups at the digits, and with ligatures on and no other
    // lookup at their letters, a ligature reads pairs of plain glyphs.
    let cases = [
        (NOTO_SANS_MERGED, ["", "onum"], "2026 13:45"),
        (NOTO_SANS_MERGED, ["liga off", ""], "fi fl office"),
    ];
    for (path, settings, text) in cases {
        let mut shaper = Shaper::default();
        for settings in settings {
            let features: Vec<FontFeature> = font_feature_settings(settings).collect();
            let style = Style {
                weight: 400,
                features: &features,
            };
            let (shaped, oracle) = compare(&mut shaper, path, text, style);
            assert_eq!(shaped, oracle, "{path}: {settings}");
        }
    }
}
