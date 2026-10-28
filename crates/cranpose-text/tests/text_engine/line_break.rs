use cranpose_text::{FontFamilyResolver, LineBreak, ParagraphStyle, TextLayout, TextLayoutOptions};

use crate::fonts::{ROBOTO, lay_out, typeface};

fn layout(
    resolver: &FontFamilyResolver,
    text: &str,
    line_break: LineBreak,
    width: f32,
) -> TextLayout {
    let paragraph = ParagraphStyle {
        line_break,
        ..ParagraphStyle::default()
    };
    lay_out(
        resolver,
        text,
        &paragraph,
        width,
        TextLayoutOptions::default(),
    )
}

fn widths(laid: &TextLayout) -> Vec<f32> {
    (0..laid.line_count())
        .map(|line| laid.line_right(line) - laid.line_left(line))
        .collect()
}

fn spread(widths: &[f32]) -> f32 {
    let longest = widths.iter().copied().fold(f32::MIN, f32::max);
    let shortest = widths.iter().copied().fold(f32::MAX, f32::min);
    longest - shortest
}

fn text_of(laid: &TextLayout, text: &str) -> String {
    (0..laid.line_count())
        .map(|line| {
            text.get(laid.line_start(line)..laid.line_end(line))
                .expect("lines end on boundaries")
        })
        .collect()
}

const WORDS: &str = "aaa bbb ccc ddd eee fff ggg";

#[test]
fn a_heading_evens_out_its_lines_where_greedy_leaves_one_short() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let six = layout(
        &resolver,
        "aaa bbb ccc ddd eee fff",
        LineBreak::Simple,
        f32::INFINITY,
    )
    .width();

    let greedy = layout(&resolver, WORDS, LineBreak::Simple, six + 1.0);
    let heading = layout(&resolver, WORDS, LineBreak::Heading, six + 1.0);

    assert_eq!(greedy.line_count(), 2);
    assert_eq!(heading.line_count(), 2);
    assert!(
        spread(&widths(&heading)) < spread(&widths(&greedy)),
        "{:?} {:?}",
        widths(&heading),
        widths(&greedy)
    );
    assert_eq!(text_of(&heading, WORDS), WORDS);
}

#[test]
fn a_paragraph_fills_its_lines_before_the_last_more_evenly_than_greedy() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let text = "I am a fox the quick brown fox jumps over the lazy dogs again";
    let width = layout(
        &resolver,
        "the quick brown fox",
        LineBreak::Simple,
        f32::INFINITY,
    )
    .width()
        + 1.0;

    let greedy = layout(&resolver, text, LineBreak::Simple, width);
    let paragraph = layout(&resolver, text, LineBreak::Paragraph, width);

    let all_but_last = |laid: &TextLayout| {
        let mut widths = widths(laid);
        widths.pop();
        widths
    };
    assert!(paragraph.line_count() >= 3);
    assert!(spread(&all_but_last(&paragraph)) <= spread(&all_but_last(&greedy)));
    assert!(widths(&paragraph).iter().all(|&line| line <= width));
    assert_eq!(text_of(&paragraph, text), text);
}

#[test]
fn a_word_wider_than_the_line_breaks_greedily_under_even_breaking() {
    let resolver = FontFamilyResolver::new([typeface(ROBOTO)]);
    let text = "a Pneumonoultramicroscopic b\nc d";
    let width = layout(&resolver, "Pneum", LineBreak::Simple, f32::INFINITY).width();

    for line_break in [LineBreak::Paragraph, LineBreak::Heading] {
        let laid = layout(&resolver, text, line_break, width);
        assert!(laid.line_count() > 4);
        assert_eq!(text_of(&laid, text), text, "{line_break:?}");
    }
}
