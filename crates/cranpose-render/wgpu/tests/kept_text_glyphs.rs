// A text drawn again while its record keeps its own draws takes its glyph
// quads from those kept with the record, and paints the pixels a first
// draw of it paints.

use cranpose_core::NodeId;
use cranpose_render_common::{
    Renderer,
    graph::{LayerNode, ProjectiveTransform, RenderNode},
};
use cranpose_ui_graphics::{Color, GraphicsLayer, Rect, Size};

use crate::{
    shared_test_support,
    support::{self, text_node},
};

const WIDTH: u32 = 240;
const HEIGHT: u32 = 80;
const LABELS: u32 = 6;

fn frame_rect() -> Rect {
    Rect::from_size(Size::new(WIDTH as f32, HEIGHT as f32))
}

// `LABELS` labels in two rows on a black page, each in a layer of its own,
// all inside one layer placed at `(x, y)` and drawn with `graphics`.
fn page(labels: [&str; LABELS as usize], place: (f32, f32), graphics: GraphicsLayer) -> LayerNode {
    page_in(labels, place, (graphics, frame_rect().size()))
}

// [`page`] with the layer of the labels `size` large: a layer that clips
// cuts the labels it does not hold.
fn page_in(
    labels: [&str; LABELS as usize],
    (x, y): (f32, f32),
    (graphics, size): (GraphicsLayer, Size),
) -> LayerNode {
    let layers = labels
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let rect = Rect {
                x: (index % 3) as f32 * 76.0 + 6.0,
                y: (index / 3) as f32 * 30.0 + 8.0,
                width: 70.0,
                height: 22.0,
            };
            // A rect shorter than the glyphs cuts them at the text's scissor.
            let cutting = Rect {
                height: 10.0,
                ..Rect::from_size(rect.size())
            };
            let text = text_node(index + 1, label, cutting, 14.0);
            let mut layer = shared_test_support::layer_node(
                rect,
                ProjectiveTransform::identity(),
                GraphicsLayer::default(),
                vec![text],
            );
            layer.node_id = Some(NodeId(index + 1));
            RenderNode::Layer(Box::new(layer))
        })
        .collect();
    let group = shared_test_support::layer_node(
        Rect {
            x,
            y,
            width: size.width,
            height: size.height,
        },
        ProjectiveTransform::identity(),
        graphics,
        layers,
    );
    shared_test_support::layer_node(
        frame_rect(),
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        vec![
            support::solid_rect(frame_rect(), Color::BLACK),
            RenderNode::Layer(Box::new(group)),
        ],
    )
}

const LABEL_TEXTS: [&str; LABELS as usize] = [
    "Alpha 1",
    "Bravo 22",
    "Charlie 3",
    "Delta 44",
    "Echo 5",
    "Foxtrot",
];

// What a frame drew: its pixels and how many texts drew from kept quads.
struct Drawn {
    pixels: Vec<u8>,
    kept: u32,
}

fn draw(renderer: &mut support::LockedRenderer, graph: &LayerNode) -> Drawn {
    renderer.scene_mut().describe(graph);
    let pixels = renderer
        .capture_frame_with_scale(WIDTH * 2, HEIGHT * 2, 2.0)
        .expect("frame renders")
        .pixels;
    let kept = renderer
        .last_frame_stats()
        .expect("frame stats")
        .text_glyph_runs_kept;
    Drawn { pixels, kept }
}

// Draws `graph` in `frames` frames in a row, as a screen at rest draws, and
// returns the last: a text cut at its scissor's edges keeps the instances it
// drew once it drew twice in a row at one place, and every frame after draws
// from them.
fn draw_at_rest(renderer: &mut support::LockedRenderer, graph: &LayerNode, frames: usize) -> Drawn {
    let first = draw(renderer, graph);
    (1..frames).fold(first, |before, _| {
        let drawn = draw(renderer, graph);
        assert!(
            before.pixels == drawn.pixels,
            "a frame at rest paints the pixels of the frame before"
        );
        drawn
    })
}

// The pixels a renderer that draws `graph` first paints. The GPU lock of
// the test is free when it returns, so a test asks for them before it
// takes its own renderer.
fn drawn_first(graph: &LayerNode) -> Option<Vec<u8>> {
    let mut renderer = renderer_or_skip("a first draw")?;
    Some(draw(&mut renderer, graph).pixels)
}

fn renderer_or_skip(test: &str) -> Option<support::LockedRenderer> {
    match support::headless_renderer_unencoded() {
        Ok(renderer) => Some(renderer),
        Err(err) => {
            eprintln!("skipping {test}: headless WGPU init failed: {err}");
            None
        }
    }
}

#[test]
fn a_text_drawn_again_takes_its_kept_quads_and_paints_the_same_pixels() {
    let Some(mut renderer) = renderer_or_skip("kept text quads") else {
        return;
    };
    let graph = page(LABEL_TEXTS, (0.0, 0.0), GraphicsLayer::default());
    let first = draw(&mut renderer, &graph);
    let later = draw_at_rest(&mut renderer, &graph, 5);
    assert_eq!(first.kept, 0, "the first frame has no kept quads");
    assert_eq!(
        later.kept, LABELS,
        "later frames draw every text from kept quads"
    );
    assert!(
        first.pixels == later.pixels,
        "kept quads paint the pixels of the first draw"
    );
}

#[test]
fn a_changed_text_builds_its_quads_again_and_the_others_keep_theirs() {
    let graph = page(LABEL_TEXTS, (0.0, 0.0), GraphicsLayer::default());
    let mut labels = LABEL_TEXTS;
    labels[2] = "Charlie 4";
    let changed = page(labels, (0.0, 0.0), GraphicsLayer::default());
    let Some(fresh) = drawn_first(&changed) else {
        return;
    };
    let Some(mut renderer) = renderer_or_skip("changed text") else {
        return;
    };
    draw(&mut renderer, &graph);
    let after = draw(&mut renderer, &changed);
    assert_eq!(
        after.kept,
        LABELS - 1,
        "only the changed text builds its quads again"
    );
    assert!(
        after.pixels == fresh,
        "the changed frame paints the pixels of a first draw"
    );
    let again = draw(&mut renderer, &changed);
    assert_eq!(again.kept, LABELS, "the rebuilt quads are kept");
}

#[test]
fn a_text_placed_elsewhere_paints_where_a_first_draw_paints_it() {
    // Whole pixels, and a place that leaves some texts past the left edge.
    let places = [(5.0, 3.0), (-20.0, 0.0), (0.0, 0.0)];
    let pages = places.map(|place| page(LABEL_TEXTS, place, GraphicsLayer::default()));
    let Some(fresh) = pages
        .iter()
        .map(drawn_first)
        .collect::<Option<Vec<Vec<u8>>>>()
    else {
        return;
    };
    let Some(mut renderer) = renderer_or_skip("placed text") else {
        return;
    };
    // At rest first, so the texts keep their instances and move them.
    draw_at_rest(
        &mut renderer,
        &page(LABEL_TEXTS, (0.0, 0.0), GraphicsLayer::default()),
        3,
    );
    for ((place, graph), fresh) in places.iter().zip(&pages).zip(&fresh) {
        let after = draw_at_rest(&mut renderer, graph, 4);
        assert!(
            after.pixels == *fresh,
            "the page placed at {place:?} paints the pixels of a first draw"
        );
    }
}

#[test]
fn a_text_in_a_layer_whose_alpha_changes_paints_where_a_first_draw_paints_it() {
    let alphas = [0.5, 0.25, 1.0];
    let pages = alphas.map(|alpha| {
        page(
            LABEL_TEXTS,
            (0.0, 0.0),
            GraphicsLayer {
                alpha,
                ..Default::default()
            },
        )
    });
    let Some(fresh) = pages
        .iter()
        .map(drawn_first)
        .collect::<Option<Vec<Vec<u8>>>>()
    else {
        return;
    };
    let Some(mut renderer) = renderer_or_skip("faded text") else {
        return;
    };
    draw(
        &mut renderer,
        &page(LABEL_TEXTS, (0.0, 0.0), GraphicsLayer::default()),
    );
    for ((alpha, graph), fresh) in alphas.iter().zip(&pages).zip(&fresh) {
        let after = draw_at_rest(&mut renderer, graph, 4);
        assert!(
            after.pixels == *fresh,
            "the page at alpha {alpha} paints the pixels of a first draw"
        );
    }
}

#[test]
fn a_text_cut_by_its_layers_clip_paints_where_a_first_draw_paints_it() {
    let clipped = |width: f32, height: f32| {
        page_in(
            LABEL_TEXTS,
            (0.0, 0.0),
            (
                GraphicsLayer {
                    clip: true,
                    ..Default::default()
                },
                Size::new(width, height),
            ),
        )
    };
    // Clips through the middle of a column, between rows and through one.
    let pages = [
        clipped(100.0, 80.0),
        clipped(240.0, 38.0),
        clipped(240.0, 80.0),
    ];
    let Some(fresh) = pages
        .iter()
        .map(drawn_first)
        .collect::<Option<Vec<Vec<u8>>>>()
    else {
        return;
    };
    let Some(mut renderer) = renderer_or_skip("cut text") else {
        return;
    };
    draw(
        &mut renderer,
        &page(LABEL_TEXTS, (0.0, 0.0), GraphicsLayer::default()),
    );
    for (index, (graph, fresh)) in pages.iter().zip(&fresh).enumerate() {
        let after = draw_at_rest(&mut renderer, graph, 4);
        assert!(
            after.kept > 0,
            "the page cut as {index} draws texts from their kept quads"
        );
        assert!(
            after.pixels == *fresh,
            "the page cut as {index} paints the pixels of a first draw"
        );
    }
}
