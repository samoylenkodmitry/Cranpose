//! A text drawn again in a rect of another size takes its glyph run from the
//! cache: a run places its glyphs from the text's origin, whatever the size
//! of the rect around them.

use cranpose_render_common::{
    Renderer,
    graph::{PrimitiveNode, ProjectiveTransform, RenderGraph, RenderNode},
};
use cranpose_ui_graphics::{Color, GraphicsLayer, Rect, Size};

use crate::{
    shared_test_support,
    support::{self, text_node},
};

const WIDTH: u32 = 220;
const HEIGHT: u32 = 40;

fn page(texts: Vec<RenderNode>) -> RenderGraph {
    let frame = Rect::from_size(Size::new(WIDTH as f32, HEIGHT as f32));
    let mut children = vec![support::solid_rect(frame, Color::BLACK)];
    children.extend(texts);
    RenderGraph::new(shared_test_support::layer_node(
        frame,
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        children,
    ))
}

fn text_rect(width: f32) -> Rect {
    Rect {
        x: 8.0,
        y: 8.0,
        width,
        height: 24.0,
    }
}

/// `count` distinct labels in a grid of 2 by 2 point cells.
fn many_texts_frame(count: usize) -> RenderGraph {
    let columns = WIDTH as usize / 2;
    page(
        (0..count)
            .map(|index| {
                let rect = Rect {
                    x: (index % columns * 2) as f32,
                    y: (index / columns * 2) as f32,
                    width: 2.0,
                    height: 2.0,
                };
                text_node(index, &format!("{index}"), rect, 2.0)
            })
            .collect(),
    )
}

fn runs_collected(renderer: &mut support::LockedRenderer, graph: RenderGraph) -> u32 {
    renderer.scene_mut().graph = Some(graph);
    renderer
        .capture_frame_with_scale(WIDTH, HEIGHT, 1.0)
        .expect("frame renders");
    renderer
        .last_frame_stats()
        .expect("frame stats")
        .text_glyph_runs_collected
}

#[test]
fn a_text_drawn_again_in_a_narrower_rect_reuses_its_glyph_run() {
    let mut renderer = match support::headless_renderer_unencoded() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping glyph run reuse: headless WGPU init failed: {err}");
            return;
        }
    };
    let mut draw = |text: RenderNode| {
        renderer.scene_mut().graph = Some(page(vec![text]));
        let pixels = renderer
            .capture_frame_with_scale(WIDTH, HEIGHT, 1.0)
            .expect("frame renders")
            .pixels;
        let collected = renderer
            .last_frame_stats()
            .expect("frame stats")
            .text_glyph_runs_collected;
        (pixels, collected)
    };

    let wide = text_node(7, "Ticker 123.45", text_rect(200.0), 16.0);
    // The same layout in a narrower rect, as a text keeps its layout while
    // its width moves without breaking it otherwise.
    let mut narrow = wide.clone();
    if let RenderNode::Primitive(entry) = &mut narrow
        && let PrimitiveNode::Text(text) = &mut entry.node
    {
        text.rect = text_rect(150.0);
    }
    let (wide, collected) = draw(wide);
    assert_eq!(collected, 1, "the first frame lays the text's run out");
    let (narrow, collected) = draw(narrow);
    assert_eq!(collected, 0, "a narrower rect takes the run from the cache");
    assert!(wide == narrow, "the text paints the same pixels");
    let (_, collected) = draw(text_node(7, "Ticker 123.46", text_rect(150.0), 16.0));
    assert_eq!(collected, 1, "a changed text lays its run out again");
}

#[test]
fn a_frame_of_more_texts_than_the_cache_floor_keeps_every_run_for_the_next() {
    let mut renderer = match support::headless_renderer_unencoded() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping many text runs: headless WGPU init failed: {err}");
            return;
        }
    };
    // More texts than the cache's floor of 1,024 runs: the first frame
    // lays every run out, then sizes the cache to hold them.
    let frame = many_texts_frame(1_600);
    let first = runs_collected(&mut renderer, frame.clone());
    assert!(first > 1_024, "the first frame lays out its runs: {first}");
    let second = runs_collected(&mut renderer, frame.clone());
    let third = runs_collected(&mut renderer, frame);
    assert_eq!(
        (second, third),
        (0, 0),
        "later frames take every run from the cache"
    );
}
