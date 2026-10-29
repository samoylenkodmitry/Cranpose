use cranpose_text::{
    FontFamily, FontFamilyResolver, LayoutInput, ParagraphLengths, ParagraphStyle, PositionedGlyph,
    Shaper, TextLayout, TextLayoutOptions, TextLayouter,
};
use cranpose_ui_graphics::MemoryHeadroom;

use crate::{
    fonts::{ARABIC_VF, DEJAVU, DEVANAGARI_VF, NOTO_SANS_MERGED, ROBOTO, span, typeface},
    harfrust_oracle::{REGULAR, compare},
};

const CASES: [(&str, &str); 6] = [
    // Roboto kerns a space with the capital after it.
    (ROBOTO, "Frame To Tokyo, AV Yes. Type 1234 Tea"),
    // and a T with the space after it: those words shape with the next one.
    (ROBOTO, "AT Tea IT is TEST ŤŢ Т Т x"),
    (NOTO_SANS_MERGED, "The office affinity Ŧŧ ﬁ — fjord"),
    (DEJAVU, "AV To fi ff ffi Wa"),
    (ARABIC_VF, "مرحبا بالعالم سلام عليكم"),
    (DEVANAGARI_VF, "नमस्ते दुनिया क्ष त्र श्री"),
];

#[test]
fn words_shaped_apart_join_back_as_the_whole_run_shapes() {
    for (path, text) in CASES {
        let (split, oracle) = compare(&mut Shaper::default(), path, text, REGULAR);
        assert_eq!(split, oracle, "{path}: {text}");
    }
}

#[test]
fn kept_words_shape_new_text_as_the_whole_run_shapes() {
    let mut shaper = Shaper::default();
    // A word is kept the second time it is shaped.
    for _ in 0..2 {
        for (path, text) in CASES {
            compare(&mut shaper, path, text, REGULAR);
        }
    }
    for (path, text) in CASES {
        let reordered: String = text.split(' ').rev().collect::<Vec<_>>().join(" ");
        let (split, oracle) = compare(&mut shaper, path, &reordered, REGULAR);
        assert_eq!(split, oracle, "{path}: {reordered}");
    }
}

// More distinct words than the word cache first has room for.
fn many_words() -> String {
    (0..600)
        .map(|n| format!("w{n}x"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn words_kept_past_the_cache_s_first_room_shape_as_the_whole_run_shapes() {
    let text = many_words();
    let mut shaper = Shaper::default();
    // The second pass keeps every word, the rings wrap over the first kept,
    // and the third misses those, which grows the room.
    for _ in 0..4 {
        let (split, oracle) = compare(&mut shaper, ROBOTO, &text, REGULAR);
        assert_eq!(split, oracle);
    }
}

// Every glyph of `layout`, line by line, where it draws.
fn placed(layout: &TextLayout) -> Vec<PositionedGlyph> {
    (0..layout.line_count())
        .flat_map(|line| layout.glyph_runs(line).collect::<Vec<_>>())
        .flat_map(|run| run.positions().collect::<Vec<_>>())
        .collect()
}

#[test]
fn a_layouter_told_memory_is_tight_lays_text_out_as_with_ample_memory() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let text = many_words();
    let family = FontFamily::Default;
    let spans = [span(0..text.len(), &family, 400)];
    let paragraph = ParagraphStyle::default();
    let input = LayoutInput {
        text: &text,
        spans: &spans,
        paragraph: &paragraph,
        lengths: ParagraphLengths::default(),
        min_width: 0.0,
        max_width: 300.0,
        options: TextLayoutOptions::default(),
    };
    let mut layouter = TextLayouter::default();
    let mut lay_out = |headroom| {
        layouter.set_memory_headroom(headroom);
        placed(&layouter.layout(&input, &resolver))
    };
    let first = lay_out(MemoryHeadroom::Ample);
    for headroom in [
        MemoryHeadroom::Ample,
        MemoryHeadroom::Ample,
        MemoryHeadroom::Tight,
        MemoryHeadroom::Tight,
        MemoryHeadroom::Ample,
    ] {
        assert_eq!(lay_out(headroom), first, "{headroom:?}");
    }
}
