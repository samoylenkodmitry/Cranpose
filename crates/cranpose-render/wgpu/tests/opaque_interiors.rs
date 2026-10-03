//! A pass without composites lays the interiors of its opaque fills down
//! front to back in a depth pre-pass, so the fills later records hide are
//! never shaded. The pre-pass must change no pixel: every scene here draws
//! the same with it and without it.

use cranpose_render_common::{
    Renderer,
    graph::{
        DrawCommandId, DrawRunNode, PrimitivePhase, ProjectiveTransform, RenderGraph, RenderNode,
    },
    style_shared::DrawPlacement,
};
use cranpose_ui_graphics::{
    Brush, Color, CornerRadii, DrawPrimitive, DrawScope, DrawScopeDefault, GraphicsLayer, Point,
    Rect, Size, Stroke, StrokeCap, StrokeJoin, TileMode,
};

use crate::{shared_test_support, support};

const FRAME: u32 = 200;
const NO_INTERIORS_FIRST: &str = "CRANPOSE_NO_INTERIORS_FIRST";

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn fill(area: Rect, color: Color, radius: f32, clip: Option<Rect>) -> RenderNode {
    support::draw_node(
        DrawPrimitive::RoundRect {
            rect: area,
            brush: Brush::solid(color),
            radii: CornerRadii::uniform(radius),
            stroke: None,
        },
        clip,
    )
}

fn layer(alpha: f32, children: Vec<RenderNode>) -> RenderNode {
    RenderNode::Layer(Box::new(shared_test_support::layer_node(
        rect(0.0, 0.0, FRAME as f32, FRAME as f32),
        ProjectiveTransform::identity(),
        GraphicsLayer {
            alpha,
            ..GraphicsLayer::default()
        },
        children,
    )))
}

fn frame_of(children: Vec<RenderNode>) -> RenderGraph {
    let mut nodes = vec![fill(
        rect(0.0, 0.0, FRAME as f32, FRAME as f32),
        Color::from_rgb_u8(20, 24, 40),
        0.0,
        None,
    )];
    nodes.extend(children);
    RenderGraph::new(shared_test_support::layer_node(
        rect(0.0, 0.0, FRAME as f32, FRAME as f32),
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        nodes,
    ))
}

/// Levels nested like replies in a thread, each inset from the last, with
/// a translucent veil and a stroked frame across them.
fn nested_levels() -> Vec<RenderNode> {
    let palette = [
        Color::from_rgb_u8(230, 80, 60),
        Color::from_rgb_u8(60, 170, 90),
        Color::from_rgb_u8(70, 110, 230),
    ];
    let mut nodes: Vec<RenderNode> = (0..24)
        .map(|level| {
            let inset = 3.0 * level as f32 + 0.25;
            fill(
                rect(
                    inset,
                    inset * 1.5,
                    FRAME as f32 - 2.0 * inset,
                    160.0 - inset,
                ),
                palette[level % palette.len()],
                4.0,
                None,
            )
        })
        .collect();
    nodes.push(fill(
        rect(30.5, 40.0, 120.0, 90.0),
        Color(0.9, 0.9, 0.2, 0.5),
        12.0,
        None,
    ));
    nodes.push(support::draw_node(
        DrawPrimitive::Rect {
            rect: rect(20.0, 20.0, 160.0, 160.0),
            brush: Brush::solid(Color::WHITE),
            stroke: Some(Stroke {
                width: 3.0,
                cap: StrokeCap::Butt,
                join: StrokeJoin::Miter,
            }),
        },
        None,
    ));
    nodes
}

/// A captured frame and the interior draws it made.
struct Capture {
    pixels: Vec<u8>,
    interior_draws: u32,
    depth_passes: u32,
}

/// Captures `graph` with the pre-pass and then without it.
fn capture_both_ways(graph: impl Fn() -> RenderGraph) -> Option<(Capture, Capture)> {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping opaque interiors: {err}");
            return None;
        }
    };
    let capture = |renderer: &mut support::LockedRenderer| {
        renderer.scene_mut().graph = Some(graph());
        let frame = renderer
            .capture_frame(FRAME, FRAME)
            .expect("capture should succeed");
        let stats = renderer.last_frame_stats().expect("stats");
        Capture {
            pixels: frame.pixels,
            interior_draws: stats.shape_interior_draws,
            depth_passes: stats.depth_passes,
        }
    };
    let first = capture(&mut renderer);
    cranpose_render_wgpu::set_debug_toggle(NO_INTERIORS_FIRST, Some("1"));
    let without = capture(&mut renderer);
    cranpose_render_wgpu::set_debug_toggle(NO_INTERIORS_FIRST, None);
    Some((first, without))
}

fn assert_same_pixels(with: &[u8], without: &[u8]) {
    let differing = support::differing_pixels(FRAME, with, without);
    assert!(
        differing.is_empty(),
        "the pre-pass changed pixels: {}",
        support::describe_differing(&differing)
    );
}

#[test]
fn nested_opaque_fills_draw_the_same_pixels_with_their_interiors_laid_down_first() {
    let Some((with, without)) = capture_both_ways(|| frame_of(nested_levels())) else {
        return;
    };
    assert!(
        with.interior_draws > 0,
        "the nested fills lay interiors down first"
    );
    assert_eq!(
        without.interior_draws, 0,
        "turned off, no pass lays interiors down"
    );
    assert_same_pixels(&with.pixels, &without.pixels);
}

#[test]
fn a_clipped_opaque_fill_hides_only_what_its_clip_shows() {
    let red = Color::from_rgb_u8(220, 40, 40);
    let green = Color::from_rgb_u8(40, 200, 90);
    let graph = || {
        frame_of(vec![
            fill(rect(20.0, 20.0, 160.0, 160.0), red, 0.0, None),
            fill(
                rect(20.0, 20.0, 160.0, 160.0),
                green,
                0.0,
                Some(rect(20.0, 20.0, 80.0, 160.0)),
            ),
        ])
    };
    let Some((with, without)) = capture_both_ways(graph) else {
        return;
    };
    assert!(with.interior_draws > 0);
    assert_same_pixels(&with.pixels, &without.pixels);
    let pixel = |x: usize, y: usize| {
        let at = (y * FRAME as usize + x) * 4;
        with.pixels[at..at + 3].to_vec()
    };
    assert_eq!(
        pixel(60, 100),
        vec![40, 200, 90],
        "the clip shows the green"
    );
    assert_eq!(
        pixel(140, 100),
        vec![220, 40, 40],
        "past the clip the red shows"
    );
}

#[test]
fn a_fill_under_layer_alpha_hides_nothing_beneath_it() {
    let graph = || {
        frame_of(vec![
            fill(
                rect(20.0, 20.0, 160.0, 160.0),
                Color::from_rgb_u8(220, 40, 40),
                8.0,
                None,
            ),
            layer(
                0.5,
                vec![fill(
                    rect(40.0, 40.0, 120.0, 120.0),
                    Color::from_rgb_u8(40, 200, 90),
                    8.0,
                    None,
                )],
            ),
        ])
    };
    let Some((with, without)) = capture_both_ways(graph) else {
        return;
    };
    assert_same_pixels(&with.pixels, &without.pixels);
    let at = (100 * FRAME as usize + 100) * 4;
    assert!(
        with.pixels[at] > 100 && with.pixels[at + 1] > 100,
        "the red shows through the half-transparent green: {:?}",
        &with.pixels[at..at + 4]
    );
}

#[test]
fn a_pass_of_small_shapes_alone_lays_no_interiors_down() {
    let graph = || {
        let dots = (0..40)
            .map(|index| {
                let x = (index % 8) as f32 * 24.0 + 4.0;
                let y = (index / 8) as f32 * 24.0 + 4.0;
                fill(
                    rect(x, y, 16.0, 16.0),
                    Color::from_rgb_u8(240, 200, 80),
                    8.0,
                    None,
                )
            })
            .collect();
        RenderGraph::new(shared_test_support::layer_node(
            rect(0.0, 0.0, FRAME as f32, FRAME as f32),
            ProjectiveTransform::identity(),
            GraphicsLayer::default(),
            dots,
        ))
    };
    let Some((with, without)) = capture_both_ways(graph) else {
        return;
    };
    assert_eq!(
        with.interior_draws, 0,
        "a small circle's inset square is under the least an occluder takes"
    );
    assert_same_pixels(&with.pixels, &without.pixels);
}

#[test]
fn opaque_circles_lay_their_inset_squares_down_and_draw_the_same_pixels() {
    // Stripes beneath show through any pixel of a circle's inset square
    // that its fill does not cover whole: laying the square down first
    // would hide them there.
    let graph = || {
        let stripes = (0..10u8).map(|band| {
            fill(
                rect(0.0, f32::from(band) * 20.0, FRAME as f32, 10.0),
                Color::from_rgb_u8(200, 60 + band * 15, 90),
                0.0,
                None,
            )
        });
        let circles = [50.0f32, 63.3, 80.7, 97.1, 120.0]
            .into_iter()
            .enumerate()
            .map(|(index, diameter)| {
                let offset = index as f32 * 13.37;
                fill(
                    rect(
                        offset % 80.0 + 0.3,
                        (offset * 1.7) % 80.0 + 0.6,
                        diameter,
                        diameter,
                    ),
                    Color::from_rgb_u8(40, 180, 220),
                    diameter / 2.0,
                    None,
                )
            });
        frame_of(stripes.chain(circles).collect())
    };
    let Some((with, without)) = capture_both_ways(graph) else {
        return;
    };
    assert!(
        with.interior_draws > 0,
        "the circles lay their inset squares down first"
    );
    assert_same_pixels(&with.pixels, &without.pixels);
}

/// A transform turning `area` by `degrees` about its centre.
fn turned_about_centre(area: Rect, degrees: f32) -> ProjectiveTransform {
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (cx, cy) = (area.x + area.width / 2.0, area.y + area.height / 2.0);
    let turn = |x: f32, y: f32| {
        let (dx, dy) = (x - cx, y - cy);
        [cx + dx * cos - dy * sin, cy + dx * sin + dy * cos]
    };
    ProjectiveTransform::from_rect_to_quad(
        area,
        [
            turn(area.x, area.y),
            turn(area.x + area.width, area.y),
            turn(area.x, area.y + area.height),
            turn(area.x + area.width, area.y + area.height),
        ],
    )
}

/// Levels nested like replies in a thread, each turned a little against the
/// one around it, so every level draws in place under a transform.
fn turned_levels(level: usize) -> RenderNode {
    let palette = [
        Color::from_rgb_u8(230, 80, 60),
        Color::from_rgb_u8(60, 170, 90),
        Color::from_rgb_u8(70, 110, 230),
    ];
    let frame = rect(0.0, 0.0, FRAME as f32, FRAME as f32);
    let inset = 5.0 * level as f32 + 10.25;
    let mut children = vec![fill(
        rect(
            inset,
            inset,
            FRAME as f32 - 2.0 * inset,
            FRAME as f32 - 2.0 * inset,
        ),
        palette[level % palette.len()],
        4.0,
        None,
    )];
    if level < 12 {
        children.push(turned_levels(level + 1));
    }
    let degrees = if level.is_multiple_of(2) { 2.5 } else { -1.5 };
    RenderNode::Layer(Box::new(shared_test_support::layer_node(
        frame,
        turned_about_centre(frame, degrees),
        GraphicsLayer::default(),
        children,
    )))
}

#[test]
fn turned_opaque_fills_lay_their_interiors_down_and_draw_the_same_pixels() {
    let alone = || {
        RenderGraph::new(shared_test_support::layer_node(
            rect(0.0, 0.0, FRAME as f32, FRAME as f32),
            ProjectiveTransform::identity(),
            GraphicsLayer::default(),
            vec![turned_levels(0)],
        ))
    };
    let Some((alone, _)) = capture_both_ways(alone) else {
        return;
    };
    assert_eq!(
        alone.interior_draws, 1,
        "the turned levels lay their interiors down, in the one draw they share"
    );
    let Some((with, without)) = capture_both_ways(|| frame_of(vec![turned_levels(0)])) else {
        return;
    };
    assert_eq!(
        with.interior_draws, 2,
        "the flat page and the turned levels, turned but once, keep their own pipelines"
    );
    assert_eq!(without.interior_draws, 0);
    assert_same_pixels(&with.pixels, &without.pixels);
}

/// Cells in rows, every other one turned a little in place, or only the
/// flat ones when `turned` is false: enough changes between flat and turned
/// for the pass to draw both with the pipeline telling them apart.
fn alternating_cells(turned: bool) -> RenderGraph {
    let palette = [
        Color::from_rgb_u8(230, 80, 60),
        Color::from_rgb_u8(60, 170, 90),
    ];
    let cells = (0..CELLS)
        .filter(|index| turned || index % 2 == 0)
        .map(|index| {
            let area = cell_area(index);
            let cell = fill(area, palette[index % 2], 3.0, None);
            if index % 2 == 0 {
                return cell;
            }
            RenderNode::Layer(Box::new(shared_test_support::layer_node(
                rect(0.0, 0.0, FRAME as f32, FRAME as f32),
                turned_about_centre(area, 3.0),
                GraphicsLayer::default(),
                vec![cell],
            )))
        })
        .collect();
    frame_of(cells)
}

const CELLS: usize = 16;

fn cell_area(index: usize) -> Rect {
    rect(
        10.25 + 24.0 * (index % 8) as f32,
        40.5 + 32.0 * (index / 8) as f32,
        16.0,
        16.0,
    )
}

#[test]
fn turned_cells_among_flat_ones_add_no_draws_and_leave_the_flat_pixels_alone() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping mixed turns: {err}");
            return;
        }
    };
    let mut capture = |turned: bool| {
        renderer.scene_mut().graph = Some(alternating_cells(turned));
        let frame = renderer
            .capture_frame(FRAME, FRAME)
            .expect("capture should succeed");
        let stats = renderer.last_frame_stats().expect("stats");
        (frame.pixels, stats.draw_calls)
    };
    let (mixed, mixed_draws) = capture(true);
    let (flat, flat_draws) = capture(false);
    assert_eq!(
        mixed_draws, flat_draws,
        "turned records share the flat ones' pipeline, so interleaving them splits no draw"
    );
    let row = FRAME as usize * 4;
    for index in (0..CELLS).step_by(2) {
        let area = cell_area(index);
        let left = area.x as usize - 1;
        let top = area.y as usize - 1;
        for y in top..top + 20 {
            let span = y * row + left * 4..y * row + (left + 19) * 4;
            assert_eq!(
                mixed[span.clone()],
                flat[span],
                "flat cell {index} draws as it does alone, at row {y}"
            );
        }
    }
}

fn cleared_page(draw: impl Fn(&mut DrawScopeDefault, f32)) -> RenderGraph {
    let size = FRAME as f32;
    let mut scope = DrawScopeDefault::new(Size::new(size, size));
    scope.draw_rect(Brush::solid(Color::from_rgb_u8(20, 24, 40)));
    draw(&mut scope, size);
    RenderGraph::new(shared_test_support::layer_node(
        rect(0.0, 0.0, size, size),
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        vec![RenderNode::DrawRun(DrawRunNode::for_command(
            PrimitivePhase::BeforeChildren,
            Some(DrawCommandId {
                node_id: 7_500,
                command_index: 0,
                placement: DrawPlacement::Behind,
            }),
            scope.into_primitives(),
        ))],
    ))
}

fn assert_cleared_without_depth(graph: impl Fn() -> RenderGraph) {
    let Some((with, without)) = capture_both_ways(graph) else {
        return;
    };
    assert_eq!(
        with.depth_passes, 0,
        "the background is the pass's clear, so nothing is left to lay down"
    );
    assert_same_pixels(&with.pixels, &without.pixels);
}

#[test]
fn a_background_the_pass_clears_to_leaves_the_pass_without_a_depth_buffer() {
    assert_cleared_without_depth(|| {
        cleared_page(|scope, size| {
            scope.draw_circle(
                Brush::radial_gradient_tiled(
                    vec![Color(1.0, 0.8, 0.3, 0.9), Color(0.2, 0.4, 1.0, 0.0)],
                    Point::new(60.0, 60.0),
                    60.0,
                    TileMode::Clamp,
                ),
                Point::new(size * 0.5, size * 0.5),
                60.0,
            );
        })
    });
}

#[test]
fn a_cleared_background_sharing_its_segment_with_translucent_fills_takes_no_depth_buffer() {
    assert_cleared_without_depth(|| {
        cleared_page(|scope, size| {
            scope.draw_rect_at(
                rect(size * 0.25, size * 0.25, size * 0.5, size * 0.5),
                Brush::solid(Color(0.9, 0.5, 0.2, 0.6)),
            );
        })
    });
}

#[test]
fn an_opaque_card_after_a_cleared_background_in_its_segment_still_lays_its_interior_down() {
    let Some((with, without)) = capture_both_ways(|| {
        cleared_page(|scope, size| {
            scope.draw_rect_at(
                rect(size * 0.1, size * 0.1, size * 0.3, size * 0.3),
                Brush::solid(Color(0.9, 0.5, 0.2, 0.6)),
            );
            scope.draw_round_rect_at(
                rect(size * 0.2, size * 0.2, size * 0.6, size * 0.6),
                Brush::solid(Color::from_rgb_u8(200, 210, 230)),
                CornerRadii::uniform(12.0),
            );
        })
    }) else {
        return;
    };
    assert_eq!(with.depth_passes, 1, "the card occludes what it covers");
    assert!(with.interior_draws > 0);
    assert_same_pixels(&with.pixels, &without.pixels);
}
