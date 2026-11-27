// A fill that a later opaque fill of its pass hides in part draws only the
// frame around the hidden part. A fill that one hides whole draws nothing.
// The cut must change no pixel: every scene here draws the same with it and
// with it left out (`CRANPOSE_ABLATE=occlusion_cut`), with the depth
// pre-pass of opaque interiors and without it, as GL draws.

use cranpose_core::debug_toggles::set_debug_toggle;
use cranpose_render_common::{
    Renderer,
    graph::{LayerNode, ProjectiveTransform, RenderNode},
};
use cranpose_ui_graphics::{
    Brush, Color, CornerRadii, DrawPrimitive, GraphicsLayer, LayerShape, Rect, RenderEffect,
    RoundedCornerShape,
};

use crate::{shared_test_support, support, support::round_rect_fill as fill};

const FRAME: u32 = 200;
const ABLATE: &str = "CRANPOSE_ABLATE";
const GPU_STATS: &str = "CRANPOSE_GPU_STATS";
const NO_INTERIORS_FIRST: &str = "CRANPOSE_NO_INTERIORS_FIRST";
const RED: Color = Color::from_rgb_u8(220, 40, 40);
const GREEN: Color = Color::from_rgb_u8(40, 200, 90);
const BLUE: Color = Color::from_rgb_u8(40, 80, 220);

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn layer(
    graphics_layer: GraphicsLayer,
    transform: ProjectiveTransform,
    children: Vec<RenderNode>,
) -> RenderNode {
    RenderNode::Layer(Box::new(shared_test_support::layer_node(
        rect(0.0, 0.0, FRAME as f32, FRAME as f32),
        transform,
        graphics_layer,
        children,
    )))
}

fn page(children: Vec<RenderNode>) -> LayerNode {
    shared_test_support::layer_node(
        rect(0.0, 0.0, FRAME as f32, FRAME as f32),
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        children,
    )
}

// One frame of a scene: its pixels and the shape pixels that it rasterized.
struct Capture {
    pixels: Vec<u8>,
    fill_pixels: u64,
}

// A scene drawn with the cut and with it left out.
struct Pair {
    cut: Capture,
    whole: Capture,
}

fn capture(renderer: &mut support::LockedRenderer, graph: &LayerNode) -> Capture {
    renderer.scene_mut().describe(graph);
    let frame = renderer
        .capture_frame(FRAME, FRAME)
        .expect("capture should succeed");
    let stats = renderer.last_frame_stats().expect("stats");
    assert!(
        support::pipelines_settled(&stats),
        "every draw took its own pipeline"
    );
    Capture {
        pixels: frame.pixels,
        fill_pixels: stats.shape_fill_pixels,
    }
}

fn pair(renderer: &mut support::LockedRenderer, graph: &LayerNode) -> Pair {
    let cut = capture(renderer, graph);
    set_debug_toggle(ABLATE, Some("occlusion_cut"));
    let whole = capture(renderer, graph);
    set_debug_toggle(ABLATE, None);
    Pair { cut, whole }
}

// `graph` drawn with the cut and without it, first with the depth pre-pass
// and then without it.
fn pairs(graph: impl Fn() -> LayerNode) -> Option<[Pair; 2]> {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping occlusion cut: {err}");
            return None;
        }
    };
    let graph = graph();
    set_debug_toggle(GPU_STATS, Some("1"));
    let with_depth = pair(&mut renderer, &graph);
    set_debug_toggle(NO_INTERIORS_FIRST, Some("1"));
    let without_depth = pair(&mut renderer, &graph);
    set_debug_toggle(NO_INTERIORS_FIRST, None);
    set_debug_toggle(GPU_STATS, None);
    Some([with_depth, without_depth])
}

fn assert_same_pixels(pair: &Pair) {
    let differing = support::differing_pixels(FRAME, &pair.cut.pixels, &pair.whole.pixels);
    assert!(
        differing.is_empty(),
        "the cut changed pixels: {}",
        support::describe_differing(&differing)
    );
}

// The cut drew fewer shape pixels and the same picture.
fn assert_cut(pairs: &[Pair; 2]) {
    for pair in pairs {
        assert_same_pixels(pair);
        assert!(
            pair.cut.fill_pixels < pair.whole.fill_pixels,
            "the cut drew {} shape pixels, the whole fills {}",
            pair.cut.fill_pixels,
            pair.whole.fill_pixels
        );
    }
}

// The cut drew every fill whole.
fn assert_whole(pairs: &[Pair; 2]) {
    for pair in pairs {
        assert_same_pixels(pair);
        assert_eq!(
            pair.cut.fill_pixels, pair.whole.fill_pixels,
            "nothing is hidden, so every fill draws whole"
        );
    }
}

fn pixel(pixels: &[u8], x: usize, y: usize) -> [u8; 3] {
    let at = (y * FRAME as usize + x) * 4;
    [pixels[at], pixels[at + 1], pixels[at + 2]]
}

// Levels nested like the boxes of a deep tree, each a row lower and a
// pixel in from the last, as rounded opaque backgrounds.
fn nested_levels() -> LayerNode {
    let palette = [RED, GREEN, BLUE];
    page(
        (0..20)
            .map(|level| {
                let inset = level as f32;
                fill(
                    rect(
                        4.0 + inset,
                        4.0 + 8.0 * inset,
                        192.0 - 2.0 * inset,
                        190.0 - 9.0 * inset,
                    ),
                    palette[level % palette.len()],
                    3.0,
                    None,
                )
            })
            .collect(),
    )
}

#[test]
fn nested_opaque_backgrounds_draw_as_frames_with_the_same_pixels() {
    let Some(pairs) = pairs(nested_levels) else {
        return;
    };
    assert_cut(&pairs);
}

#[test]
fn a_translucent_fill_between_the_two_keeps_its_pixels() {
    let graph = || {
        page(vec![
            fill(rect(10.0, 10.0, 180.0, 180.0), RED, 6.0, None),
            fill(
                rect(30.0, 30.0, 140.0, 60.0),
                Color(0.9, 0.9, 0.2, 0.5),
                4.0,
                None,
            ),
            fill(rect(40.0, 40.0, 120.0, 120.0), BLUE, 6.0, None),
        ])
    };
    let Some(pairs) = pairs(graph) else {
        return;
    };
    assert_cut(&pairs);
    let veiled = pixel(&pairs[0].cut.pixels, 35, 35);
    assert!(
        veiled[0] > 200 && veiled[1] > 100,
        "the veil blends over the red outside the blue: {veiled:?}"
    );
    assert_eq!(
        pixel(&pairs[0].cut.pixels, 100, 60),
        [40, 80, 220],
        "the opaque fill covers the veil"
    );
}

#[test]
fn a_translucent_fill_under_an_opaque_one_draws_as_a_frame() {
    let graph = || {
        page(vec![
            fill(
                rect(10.0, 10.0, 180.0, 180.0),
                Color(0.2, 0.6, 0.9, 0.6),
                5.0,
                None,
            ),
            fill(rect(30.0, 30.0, 140.0, 140.0), GREEN, 5.0, None),
        ])
    };
    let Some(pairs) = pairs(graph) else {
        return;
    };
    assert_cut(&pairs);
}

// Both fills draw under one rounded clip, whose corners let the red show
// through the green's edge.
#[test]
fn an_opaque_fill_under_a_rounded_clip_hides_nothing() {
    let rounded = |children: Vec<RenderNode>| {
        layer(
            GraphicsLayer {
                clip: true,
                shape: LayerShape::Rounded(RoundedCornerShape::uniform(40.0)),
                ..GraphicsLayer::default()
            },
            ProjectiveTransform::identity(),
            children,
        )
    };
    let graph = || {
        page(vec![
            fill(rect(10.0, 10.0, 180.0, 180.0), BLUE, 0.0, None),
            rounded(vec![
                fill(rect(0.0, 0.0, 200.0, 200.0), RED, 0.0, None),
                fill(rect(0.0, 0.0, 200.0, 200.0), GREEN, 0.0, None),
            ]),
        ])
    };
    let Some(pairs) = pairs(graph) else {
        return;
    };
    assert_whole(&pairs);
    let pixels = &pairs[0].cut.pixels;
    assert_eq!(
        pixel(pixels, 10, 10),
        [40, 80, 220],
        "past the clip's corner the blue shows"
    );
    assert_eq!(
        pixel(pixels, 100, 100),
        [40, 200, 90],
        "within the clip the green shows"
    );
}

#[test]
fn a_turned_fill_neither_hides_nor_draws_as_a_frame() {
    let turned = |children: Vec<RenderNode>| {
        layer(
            GraphicsLayer::default(),
            support::turned_about_centre(rect(0.0, 0.0, FRAME as f32, FRAME as f32), 7.0),
            children,
        )
    };
    let over_turned = || {
        page(vec![
            turned(vec![fill(rect(30.0, 30.0, 140.0, 140.0), RED, 4.0, None)]),
            fill(rect(50.0, 50.0, 100.0, 100.0), GREEN, 4.0, None),
        ])
    };
    let under_turned = || {
        page(vec![
            fill(rect(30.0, 30.0, 140.0, 140.0), RED, 4.0, None),
            turned(vec![fill(rect(50.0, 50.0, 100.0, 100.0), GREEN, 4.0, None)]),
        ])
    };
    let (Some(over), Some(under)) = (pairs(over_turned), pairs(under_turned)) else {
        return;
    };
    assert_whole(&over);
    assert_whole(&under);
}

// A gradient joins the red fill's draw, whose pipeline then draws vertex
// gradients. A turned fill draws between the red fill and the green one
// that hides its middle. The red fill still draws as a frame with the
// same pixels.
#[test]
fn a_frame_beside_a_gradient_and_a_turned_fill_keeps_its_pixels() {
    let graph = || {
        page(vec![
            fill(rect(10.0, 10.0, 180.0, 180.0), RED, 4.0, None),
            support::draw_node(
                DrawPrimitive::RoundRect {
                    rect: rect(100.0, 20.0, 80.0, 150.0),
                    brush: Brush::linear_gradient(vec![GREEN, BLUE]),
                    radii: CornerRadii::uniform(0.0),
                    stroke: None,
                },
                None,
            ),
            layer(
                GraphicsLayer::default(),
                support::turned_about_centre(rect(20.0, 150.0, 40.0, 30.0), 9.0),
                vec![fill(rect(20.0, 150.0, 40.0, 30.0), BLUE, 0.0, None)],
            ),
            fill(rect(40.0, 40.0, 90.0, 100.0), GREEN, 4.0, None),
        ])
    };
    let Some(pairs) = pairs(graph) else {
        return;
    };
    assert_cut(&pairs);
}

// The opaque fill reaches past the fill under it and past the clip over
// both fills. It hides only what lies under it within the clip.
#[test]
fn an_opaque_fill_reaching_past_its_clip_hides_only_what_the_clip_shows() {
    let clip = rect(0.0, 0.0, FRAME as f32, 120.0);
    let graph = || {
        page(vec![
            fill(rect(20.0, 20.0, 120.0, 160.0), RED, 0.0, Some(clip)),
            fill(rect(60.0, 40.0, 140.0, 160.0), GREEN, 0.0, Some(clip)),
        ])
    };
    let Some(pairs) = pairs(graph) else {
        return;
    };
    assert_cut(&pairs);
    let pixels = &pairs[1].cut.pixels;
    assert_eq!(
        pixel(pixels, 100, 100),
        [40, 200, 90],
        "the green covers the red"
    );
    assert_eq!(
        pixel(pixels, 40, 100),
        [220, 40, 40],
        "left of the green the red shows"
    );
    assert_eq!(
        pixel(pixels, 100, 140),
        pixel(pixels, 10, 190),
        "below the clip neither draws"
    );
}

#[test]
fn an_opaque_fill_under_layer_alpha_hides_nothing() {
    let graph = || {
        page(vec![
            fill(rect(20.0, 20.0, 160.0, 160.0), RED, 8.0, None),
            layer(
                GraphicsLayer {
                    alpha: 0.5,
                    ..GraphicsLayer::default()
                },
                ProjectiveTransform::identity(),
                vec![fill(rect(40.0, 40.0, 120.0, 120.0), GREEN, 8.0, None)],
            ),
        ])
    };
    let Some(pairs) = pairs(graph) else {
        return;
    };
    assert_whole(&pairs);
    let through = pixel(&pairs[0].cut.pixels, 100, 100);
    assert!(
        through[0] > 100 && through[1] > 100,
        "the red shows through the half-transparent green: {through:?}"
    );
}

#[test]
fn a_backdrop_read_between_the_two_keeps_the_fill_whole() {
    let graph = || {
        let blurred = LayerNode {
            graphics_layer: GraphicsLayer {
                backdrop_effect: Some(RenderEffect::blur(12.0)),
                ..GraphicsLayer::default()
            }
            .into(),
            transform_to_parent: ProjectiveTransform::translation(60.0, 60.0),
            ..support::layer_node(None, 80.0, 80.0, vec![])
        };
        page(vec![
            fill(rect(10.0, 10.0, 180.0, 180.0), RED, 4.0, None),
            fill(
                rect(70.0, 20.0, 60.0, 160.0),
                Color(0.1, 0.2, 0.9, 0.5),
                0.0,
                None,
            ),
            RenderNode::Layer(Box::new(blurred)),
            fill(rect(30.0, 30.0, 140.0, 140.0), GREEN, 4.0, None),
        ])
    };
    let Some(pairs) = pairs(graph) else {
        return;
    };
    assert_whole(&pairs);
}

#[test]
fn a_fill_under_a_later_opaque_one_whole_draws_nothing() {
    let graph = || {
        page(vec![
            fill(
                rect(60.0, 60.0, 80.0, 80.0),
                Color(0.9, 0.9, 0.2, 0.5),
                4.0,
                None,
            ),
            fill(rect(20.0, 20.0, 160.0, 160.0), GREEN, 4.0, None),
        ])
    };
    let Some(pairs) = pairs(graph) else {
        return;
    };
    assert_cut(&pairs);
    assert_eq!(
        pixel(&pairs[0].cut.pixels, 100, 100),
        [40, 200, 90],
        "the opaque fill covers the veil under it"
    );
}
