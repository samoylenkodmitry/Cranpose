use std::rc::Rc;

use cranpose_render_common::{
    text_decoration::{visit_text_backgrounds, visit_text_lines},
    text_fonts::TextFonts,
};
use cranpose_ui::{
    PreparedTextLayout,
    text::{AnnotatedString, SpanStyle, TextDecoration, TextLayoutOptions, TextStyle, TextUnit},
};
use cranpose_ui_graphics::{Color, Rect};

fn fonts() -> TextFonts {
    TextFonts::from_fonts_or_default(&[
        include_bytes!("../assets/NotoSansMerged.ttf"),
        include_bytes!("fixtures/localization/Arabic.ttf"),
    ])
}

fn decorated(decoration: TextDecoration, letter_spacing: f32) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            font_size: TextUnit::Sp(20.0),
            letter_spacing: TextUnit::Sp(letter_spacing),
            text_decoration: Some(decoration),
            background: Some(Color::RED),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn prepare(text: &str, style: &TextStyle, max_width: Option<f32>) -> Rc<PreparedTextLayout> {
    cranpose_ui::AppContext::new().enter(|| {
        cranpose_ui::text::set_text_fonts(fonts().resolver());
        cranpose_ui::text::prepare_text_layout(
            &AnnotatedString::from(text),
            style,
            TextLayoutOptions::default(),
            max_width,
        )
    })
}

fn lines(prepared: &PreparedTextLayout, style: &TextStyle) -> Vec<Rect> {
    let mut rects = Vec::new();
    visit_text_lines(
        &prepared.layout,
        prepared.text.as_ref().into(),
        style,
        Color::WHITE,
        |rect, _| rects.push(rect),
    );
    rects
}

#[test]
fn each_line_of_a_text_gets_its_own_underline_under_its_glyphs() {
    let style = decorated(TextDecoration::UNDERLINE, 0.0);
    let prepared = prepare("One\nTwo\nThree", &style, None);
    let layout = &prepared.layout;

    let underlines = lines(&prepared, &style);

    assert_eq!(underlines.len(), 3, "{underlines:?}");
    for (line, underline) in underlines.iter().enumerate() {
        assert!(
            underline.y > layout.line_baseline(line) && underline.y < layout.line_bottom(line),
            "line {line}: {underline:?}"
        );
        assert!((underline.x - layout.line_left(line)).abs() < 0.01);
        assert!((underline.x + underline.width - layout.line_right(line)).abs() < 0.01);
    }
}

#[test]
fn a_line_through_crosses_its_glyphs_above_the_baseline() {
    let style = decorated(TextDecoration::LINE_THROUGH, 0.0);
    let prepared = prepare("Struck", &style, None);
    let layout = &prepared.layout;

    let [strike] = lines(&prepared, &style)[..] else {
        panic!("one line through");
    };

    assert!(strike.y < layout.line_baseline(0) && strike.y > layout.line_top(0));
}

#[test]
fn a_decoration_runs_unbroken_over_spaces_and_letter_spacing() {
    let style = decorated(
        TextDecoration::UNDERLINE.combine(TextDecoration::LINE_THROUGH),
        6.0,
    );
    let prepared = prepare("a b  c", &style, None);

    let rects = lines(&prepared, &style);

    assert_eq!(rects.len(), 2, "an underline and a line through: {rects:?}");
    for rect in rects {
        assert!((rect.x - prepared.layout.line_left(0)).abs() < 0.01);
        assert!((rect.x + rect.width - prepared.layout.line_right(0)).abs() < 0.01);
    }
}

#[test]
fn decorations_and_backgrounds_of_mixed_directions_stay_within_their_line() {
    let style = decorated(TextDecoration::UNDERLINE, 0.0);
    let prepared = prepare("abc مرحبا def بالعالم ghi", &style, Some(140.0));
    let layout = &prepared.layout;
    assert!(layout.line_count() > 1, "the text wraps");

    let mut backgrounds = Vec::new();
    visit_text_backgrounds(
        &prepared.layout,
        prepared.text.as_ref().into(),
        &style,
        |rect, _| backgrounds.push(rect),
    );
    let underlines = lines(&prepared, &style);

    // The rects on `line`, left to right: their left and right edges.
    let covered = |rects: &[Rect], line: usize| {
        let mut on_line: Vec<(f32, f32)> = rects
            .iter()
            .filter(|rect| rect.y >= layout.line_top(line) && rect.y < layout.line_bottom(line))
            .map(|rect| (rect.x, rect.x + rect.width))
            .collect();
        on_line.sort_by(|a, b| a.0.total_cmp(&b.0));
        on_line
    };
    for line in 0..layout.line_count() {
        for (name, rects) in [("underline", &underlines), ("background", &backgrounds)] {
            let spans = covered(rects, line);
            assert!(!spans.is_empty(), "line {line} has a {name}");
            for pair in spans.windows(2) {
                assert!(
                    (pair[1].0 - pair[0].1).abs() < 0.5,
                    "line {line}: the {name} has a gap: {spans:?}"
                );
            }
            let (left, right) = (spans[0].0, spans[spans.len() - 1].1);
            assert!(
                left <= layout.line_left(line) + 0.5
                    && right >= layout.line_right(line) - 0.5
                    && right <= layout.width() + 0.5,
                "line {line}: the {name} spans {left}..{right} for glyphs from {} to {}",
                layout.line_left(line),
                layout.line_right(line)
            );
        }
    }
}
