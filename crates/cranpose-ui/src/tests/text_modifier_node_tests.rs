use std::collections::hash_map::DefaultHasher;

use super::*;
use crate::text::TextUnit;

fn hash_of(element: &TextModifierElement) -> u64 {
    let mut hasher = DefaultHasher::new();
    element.hash(&mut hasher);
    hasher.finish()
}

// Runs `test` in an app context of its own, on a thread of its own.
fn in_app<R: Send + 'static>(test: impl FnOnce() -> R + Send + 'static) -> R {
    std::thread::spawn(move || crate::AppContext::new().enter(test))
        .join()
        .expect("the test thread finishes")
}

fn text_node(text: &str, style: TextStyle, options: TextLayoutOptions) -> TextModifierNode {
    TextModifierNode::new(
        Rc::new(AnnotatedString::from(text)),
        style,
        options,
        Density::new(1.0, 1.0),
    )
}

fn sized(font_size: f32) -> TextStyle {
    TextStyle {
        span_style: crate::text::SpanStyle {
            font_size: TextUnit::Sp(font_size),
            ..Default::default()
        },
        ..Default::default()
    }
}

// The text, left and right edge of each line of a layout.
type Lines = Vec<(String, f32, f32)>;

fn lines(layout: &crate::text::PreparedTextLayout) -> Lines {
    let text = &layout.text.text;
    (0..layout.layout.line_count())
        .map(|line| {
            (
                text[layout.layout.line_start(line)..layout.layout.line_visible_end(line)]
                    .to_owned(),
                layout.layout.line_left(line),
                layout.layout.line_right(line),
            )
        })
        .collect()
}

#[test]
fn hash_matches_for_equal_elements() {
    let style = TextStyle {
        span_style: crate::text::SpanStyle {
            font_size: TextUnit::Sp(14.0),
            letter_spacing: TextUnit::Em(0.1),
            ..Default::default()
        },
        ..Default::default()
    };
    let options = TextLayoutOptions::default();
    let text = Rc::new(AnnotatedString::from("Hash me"));
    let element_a = TextModifierElement::new(
        Rc::clone(&text),
        style.clone(),
        options,
        Density::new(1.0, 1.0),
    );
    let element_b = TextModifierElement::new(text, style, options, Density::new(1.0, 1.0));

    assert_eq!(element_a, element_b);
    assert_eq!(hash_of(&element_a), hash_of(&element_b));
}

#[test]
fn a_measured_node_exposes_the_layout_it_measured() {
    let (measured, prepared) = in_app(|| {
        let node = text_node("reuse", TextStyle::default(), TextLayoutOptions::default());
        let (measured, _) = node.layout.measure_layout((0.0, 120.0));
        let prepared = node
            .prepared_layout_handle()
            .measured_layout()
            .expect("measured node exposes its layout");
        (measured, (prepared.metrics.width, prepared.metrics.height))
    });
    assert_eq!((measured.width, measured.height), prepared);
}

#[test]
fn new_fonts_lay_the_text_out_again() {
    let (first, second) = in_app(|| {
        let node = text_node("fonts", sized(10.0), TextLayoutOptions::default());
        let first = node.measure_text_content((0.0, 160.0));
        crate::text::set_text_fonts(cranpose_text::FontFamilyResolver::new([]));
        let second = node.measure_text_content((0.0, 160.0));
        (first.width, second.width)
    });
    assert_eq!(first, 30.0, "five characters of 0.6 em at 10 px");
    assert_ne!(second, first, "the text measures with the new fonts");
}

#[test]
fn a_new_system_font_scale_lays_the_text_out_again() {
    let (first, second) = in_app(|| {
        let node = text_node("scale", sized(10.0), TextLayoutOptions::default());
        let first = node.measure_text_content((0.0, f32::INFINITY));
        crate::set_font_scale(1.5);
        let second = node.measure_text_content((0.0, f32::INFINITY));
        (first.height, second.height)
    });
    assert_eq!(first, 10.0);
    assert_eq!(second, 15.0);
}

#[test]
fn semantics_uses_source_text_for_scaled_overflow() {
    let node = TextModifierNode::new(
        Rc::new(AnnotatedString::from("Save Cranpose WebP")),
        TextStyle::default(),
        TextLayoutOptions {
            overflow: crate::text::TextOverflow::ScaleDown {
                min_font_size_sp: 9.0,
            },
            soft_wrap: false,
            max_lines: 1,
            min_lines: 1,
        },
        Density::new(1.0, 1.0),
    );
    let mut config = SemanticsConfiguration::default();

    node.merge_semantics(&mut config);

    assert_eq!(
        config.content_description.as_deref(),
        Some("Save Cranpose WebP")
    );
}

// The lines of `text` measured by one node at each of `widths` in turn, and
// of the text laid out afresh at each width.
fn lines_at(
    text: &'static str,
    options: TextLayoutOptions,
    widths: &'static [f32],
) -> Vec<(Lines, Lines)> {
    in_app(move || {
        let node = text_node(text, TextStyle::default(), options);
        widths
            .iter()
            .map(|width| {
                node.layout.measure_layout((0.0, *width));
                let measured = node.layout.measured_layout().expect("a measured layout");
                let fresh = in_app(move || {
                    let node = text_node(text, TextStyle::default(), options);
                    node.layout.measure_layout((0.0, *width));
                    lines(&node.layout.measured_layout().expect("a measured layout"))
                });
                (lines(&measured), fresh)
            })
            .collect()
    })
}

#[test]
fn a_node_measured_at_one_width_after_another_lays_out_as_a_fresh_node() {
    // At 14 px a character is 8.4 wide: "cell 123" is 67.2.
    for (text, options, widths) in [
        (
            "cell 123",
            TextLayoutOptions::default(),
            &[50.0, 43.0, 60.0, 66.0, 80.0, 30.0, 200.0][..],
        ),
        (
            "alpha beta gamma delta",
            TextLayoutOptions {
                overflow: crate::text::TextOverflow::Ellipsis,
                max_lines: 1,
                ..TextLayoutOptions::default()
            },
            &[60.0, 59.0, 63.0, 66.5, 67.0, 58.0][..],
        ),
        (
            "label",
            TextLayoutOptions::default(),
            &[120.0, 200.0, 43.0, 30.0][..],
        ),
    ] {
        for (at, (measured, fresh)) in widths.iter().zip(lines_at(text, options, widths)) {
            assert_eq!(measured, fresh, "{text:?} at {at}");
        }
    }
}

#[test]
fn a_text_node_never_makes_its_subtree_modal_or_hidden() {
    use cranpose_foundation::SemanticsNode as _;
    let node = TextModifierNode::new(
        Rc::new(AnnotatedString::from("Label")),
        TextStyle::default(),
        TextLayoutOptions::default(),
        Density::new(1.0, 1.0),
    );
    assert_eq!(node.reach(), cranpose_foundation::SemanticsReach::default());
}

#[test]
fn a_text_laid_out_as_written_shares_its_string_and_style_with_its_layout() {
    let (whole_shares, wrapped_shares, wrapped_lines, style_shared) = in_app(|| {
        let source = Rc::new(AnnotatedString::from("cell 123"));
        let node = TextModifierNode::new(
            Rc::clone(&source),
            TextStyle::default(),
            TextLayoutOptions::default(),
            Density::new(1.0, 1.0),
        );
        let layout_at = |width: f32| {
            node.layout.measure_layout((0.0, width));
            node.layout.measured_layout().expect("a measured layout")
        };
        // At 8.4 per character "cell 123" is 67.2 wide.
        let whole = layout_at(200.0);
        let wrapped = layout_at(50.0);
        (
            Rc::ptr_eq(&whole.text, &source),
            Rc::ptr_eq(&wrapped.text, &source),
            wrapped.layout.line_count(),
            Arc::ptr_eq(&whole.visual_style, &wrapped.visual_style),
        )
    });
    assert!(whole_shares, "a line that fits shares the node's string");
    assert!(wrapped_shares, "a wrapped text shares the node's string");
    assert_eq!(wrapped_lines, 2);
    assert!(style_shared, "both layouts share the node's style");
}

#[test]
fn an_update_to_an_equal_style_keeps_the_layout_and_a_new_style_lays_out_again() {
    let (equal, kept, differs, font_size) = in_app(|| {
        let text = Rc::new(AnnotatedString::from("cell 123"));
        let element = |style: TextStyle| {
            TextModifierElement::new(
                Rc::clone(&text),
                style,
                TextLayoutOptions::default(),
                Density::new(1.0, 1.0),
            )
        };
        let first = element(TextStyle::default());
        let mut node = first.create();
        let layout = |node: &TextModifierNode| {
            node.layout.measure_layout((0.0, 200.0));
            node.layout.measured_layout().expect("a measured layout")
        };
        let before = layout(&node);
        let same = element(TextStyle::default());
        let equal = same == first;
        same.update(&mut node);
        let kept = Rc::ptr_eq(&before, &layout(&node));
        let restyled = element(sized(24.0));
        let differs = restyled != first;
        restyled.update(&mut node);
        let after = layout(&node);
        (
            equal,
            kept,
            differs,
            after.visual_style.span_style.font_size,
        )
    });
    assert!(equal, "elements of equal styles are equal");
    assert!(kept, "an equal style keeps the prepared layout");
    assert!(differs, "an element of another style differs");
    assert_eq!(
        font_size,
        TextUnit::Sp(24.0),
        "the new style lays the text out again"
    );
}
