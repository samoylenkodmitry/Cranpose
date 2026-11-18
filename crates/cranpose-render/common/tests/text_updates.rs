//! A text whose string changes, and nothing else about it, as a ticker's
//! does, draws its new string after a scene update, as a fresh scene does.

use cranpose_core::MutableState;
use cranpose_render_common::{
    graph_scene::Scene,
    scene_store::{ItemRef, LayerRef},
    text_paint::TextPaint,
};
use cranpose_text::{PositionedGlyph, TextLayout};
use cranpose_ui::{
    Color, Column, ColumnSpec, Modifier, Size, TestComposition, Text, TextStyle,
    run_test_composition, text::SpanStyle,
};

use crate::scene_probe::{fresh_graph, initial_graph, painted_text, update_scene};

const VIEWPORT: Size = Size::new(240.0, 120.0);

fn painted(scene: &Scene) -> Vec<String> {
    let mut out = Vec::new();
    painted_text(scene.root_layer().expect("a scene"), &mut out);
    out
}

#[test]
fn a_text_that_changes_only_its_string_draws_each_new_string() {
    let text = std::rc::Rc::new(std::cell::Cell::new(None::<MutableState<&'static str>>));
    let slot = std::rc::Rc::clone(&text);
    let mut composition = run_test_composition(move || {
        let slot = std::rc::Rc::clone(&slot);
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            let price = cranpose_core::rememberMutableStateOf(|| "$100.00");
            slot.set(Some(price));
            Text(
                price.get(),
                Modifier::empty().padding(4.0),
                TextStyle::default(),
            );
            Text("static", Modifier::empty(), TextStyle::default());
        });
    });
    let root = composition.root().expect("root");
    let mut graph = initial_graph(&mut composition, root, VIEWPORT);
    let price = text.get().expect("price state");
    // Back to an earlier string too: the node must not keep the one before.
    for value in ["$101.50", "$100.00", "$99.25"] {
        price.set(value);
        update_scene(&mut composition, root, VIEWPORT, &mut graph);
        let drawn = painted(&graph);
        assert!(
            drawn.iter().any(|text| text == value),
            "the scene draws {value}: {drawn:?}"
        );
        assert_eq!(
            drawn,
            painted(&fresh_graph(&mut composition, root)),
            "the updated scene draws what a fresh one does"
        );
    }
}

fn text_paints(layer: LayerRef<'_>, out: &mut Vec<(String, TextPaint)>) {
    for item in layer.items() {
        match item {
            ItemRef::Layer(child) | ItemRef::Leaf(child) => text_paints(child, out),
            ItemRef::Text(text) => {
                assert_eq!(
                    text.paint().style_hash,
                    text.text_style.render_hash(),
                    "{} paints under its own style's hash",
                    text.render_text.text()
                );
                out.push((text.render_text.text().to_owned(), *text.paint()));
            }
            ItemRef::Draw(_) | ItemRef::Run(_) | ItemRef::Shape(_) => {}
        }
    }
}

fn painted_with_paint(scene: &Scene) -> Vec<(String, TextPaint)> {
    let mut out = Vec::new();
    text_paints(scene.root_layer().expect("a scene"), &mut out);
    out
}

#[test]
fn a_text_restyled_or_changed_in_place_paints_as_a_fresh_scene_does() {
    const RED: Color = Color(0.8, 0.1, 0.1, 1.0);
    const GREEN: Color = Color(0.1, 0.6, 0.2, 1.0);
    type Price = (MutableState<&'static str>, MutableState<Color>);
    let states = std::rc::Rc::new(std::cell::Cell::new(None::<Price>));
    let slot = std::rc::Rc::clone(&states);
    let mut composition = run_test_composition(move || {
        let slot = std::rc::Rc::clone(&slot);
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            let price = cranpose_core::rememberMutableStateOf(|| "$100.00");
            let color = cranpose_core::rememberMutableStateOf(|| RED);
            slot.set(Some((price, color)));
            Text(
                price.get(),
                Modifier::empty(),
                TextStyle::from_span_style(SpanStyle {
                    color: Some(color.get()),
                    ..SpanStyle::default()
                }),
            );
        });
    });
    let root = composition.root().expect("root");
    let mut graph = initial_graph(&mut composition, root, VIEWPORT);
    let (price, color) = states.get().expect("price states");
    for (value, tint) in [
        ("$100.00", GREEN),
        ("$99.25", GREEN),
        ("$98.00", RED),
        ("$98.00", GREEN),
    ] {
        price.set(value);
        color.set(tint);
        update_scene(&mut composition, root, VIEWPORT, &mut graph);
        let painted = painted_with_paint(&graph);
        assert_eq!(painted.len(), 1);
        assert_eq!(painted[0].0, value);
        assert_eq!(painted[0].1.color, tint, "{value}");
        assert_eq!(
            painted,
            painted_with_paint(&fresh_graph(&mut composition, root)),
            "the updated scene paints what a fresh one does"
        );
    }
}

// Each text `layer` and the records beneath it draw: its string and its
// glyphs, in draw order.
fn drawn_glyphs(layer: LayerRef<'_>, out: &mut Vec<(String, Vec<PositionedGlyph>)>) {
    for item in layer.items() {
        match item {
            ItemRef::Layer(child) | ItemRef::Leaf(child) => drawn_glyphs(child, out),
            ItemRef::Text(text) => {
                out.push((text.render_text.text().to_owned(), glyphs(&text.layout)));
            }
            ItemRef::Draw(_) | ItemRef::Run(_) | ItemRef::Shape(_) => {}
        }
    }
}

fn glyphs(layout: &TextLayout) -> Vec<PositionedGlyph> {
    (0..layout.line_count())
        .flat_map(|line| layout.glyph_runs(line))
        .flat_map(|run| run.positions().collect::<Vec<_>>())
        .collect()
}

fn scene_glyphs(scene: &Scene) -> Vec<(String, Vec<PositionedGlyph>)> {
    let mut out = Vec::new();
    drawn_glyphs(scene.root_layer().expect("a scene"), &mut out);
    out
}

// What the texts show in `frame`: counters of one to six digits, each
// changing every third frame, a third of them each frame, then a long
// paragraph one word of which changes each frame.
fn texts_in_turn(frame: usize) -> Vec<String> {
    let mut texts: Vec<String> = (0..12)
        .map(|index| {
            let turn = (frame + index) / 3;
            (10usize.pow(u32::try_from(turn % 6).unwrap_or(0)) + index).to_string()
        })
        .collect();
    let words: Vec<String> = (0..300)
        .map(|word| format!("w{}", (word * 7) % 97))
        .collect();
    texts.push(format!(
        "{} frame{frame} {}",
        words[..150].join(" "),
        words[150..].join(" ")
    ));
    texts
}

// A column of `texts`, each a `Text` of its own.
fn text_column(texts: impl Fn() -> Vec<String> + 'static) -> TestComposition {
    run_test_composition(move || {
        let texts = texts();
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            for text in &texts {
                Text(text.clone(), Modifier::empty(), TextStyle::default());
            }
        });
    })
}

#[test]
fn texts_that_change_in_turn_draw_the_glyphs_of_their_new_strings() {
    const TALL: Size = Size::new(400.0, 4000.0);
    let frame = std::rc::Rc::new(std::cell::Cell::new(None::<MutableState<usize>>));
    let slot = std::rc::Rc::clone(&frame);
    let mut composition = run_test_composition(move || {
        let state = cranpose_core::rememberMutableStateOf(|| 0usize);
        slot.set(Some(state));
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            for text in texts_in_turn(state.get()) {
                Text(text, Modifier::empty(), TextStyle::default());
            }
        });
    });
    let root = composition.root().expect("root");
    let mut graph = initial_graph(&mut composition, root, TALL);
    let state = frame.get().expect("the frame state");
    // A copy of the paragraph's layout a reader holds across frames, as a
    // `TextLayoutResult` does: it keeps the glyphs it had.
    let mut held: Option<(TextLayout, Vec<PositionedGlyph>)> = None;
    for at in 1..=9 {
        state.set(at);
        update_scene(&mut composition, root, TALL, &mut graph);
        let drawn = scene_glyphs(&graph);
        let fresh = {
            let mut fresh = text_column(move || texts_in_turn(at));
            let fresh_root = fresh.root().expect("root");
            scene_glyphs(&initial_graph(&mut fresh, fresh_root, TALL))
        };
        assert_eq!(drawn.len(), 13, "frame {at} draws every text");
        assert!(
            drawn == fresh,
            "frame {at} draws the glyphs a fresh composition of its strings draws"
        );
        if at == 4 {
            let paragraph = graph_paragraph(&graph);
            let kept = glyphs(&paragraph);
            held = Some((paragraph, kept));
        }
        if let Some((layout, kept)) = &held {
            assert!(glyphs(layout) == *kept, "the held copy keeps its glyphs");
        }
    }
}

// A copy of the layout of the last text `scene` draws, the paragraph.
fn graph_paragraph(scene: &Scene) -> TextLayout {
    fn last(layer: LayerRef<'_>, found: &mut Option<TextLayout>) {
        for item in layer.items() {
            match item {
                ItemRef::Layer(child) | ItemRef::Leaf(child) => last(child, found),
                ItemRef::Text(text) => *found = Some(text.layout.clone()),
                ItemRef::Draw(_) | ItemRef::Run(_) | ItemRef::Shape(_) => {}
            }
        }
    }
    let mut found = None;
    last(scene.root_layer().expect("a scene"), &mut found);
    found.expect("the paragraph")
}
