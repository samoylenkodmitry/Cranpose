use cranpose_render_common::graph::{
    CachePolicy, DrawRunNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase, ProjectiveTransform,
    RenderGraph, RenderNode, TextPrimitiveNode,
};
use cranpose_ui::{
    TextLayoutOptions,
    text::{AnnotatedString, SpanStyle, TextStyle, TextUnit},
};
use cranpose_ui_graphics::{
    BlendMode, Brush, Color, CompositingStrategy, CornerRadii, DrawPrimitive, GraphicsLayer,
    LayerShape, Point, RUNTIME_SHADER_PRELUDE_WGSL, Rect, RenderEffect, RoundedCornerShape,
    RuntimeShader, ShadowPrimitive,
};
use support::{capture_graph_settled, draw_node, page_graph, solid_rect};

use crate::{shared_test_support, support};

const WIDTH: u32 = 160;
const HEIGHT: u32 = 80;
const CLIP: Rect = Rect {
    x: 0.0,
    y: 0.0,
    width: 120.0,
    height: 40.0,
};
const AT: Point = Point { x: 20.0, y: 20.0 };
const RADIUS: f32 = 12.0;
const PAGE: Color = Color(0.95, 0.95, 0.92, 1.0);

fn page_background() -> RenderNode {
    solid_rect(
        Rect {
            x: 0.0,
            y: 0.0,
            width: WIDTH as f32,
            height: HEIGHT as f32,
        },
        PAGE,
    )
}

/// A layer clipped to a rounded rect of `radius`, drawn in place, or
/// through a surface of its own and the composite's rounded mask when
/// `offscreen`.
fn rounded(radius: f32, offscreen: bool) -> GraphicsLayer {
    GraphicsLayer {
        clip: true,
        shape: LayerShape::Rounded(RoundedCornerShape::uniform(radius)),
        compositing_strategy: if offscreen {
            CompositingStrategy::Offscreen
        } else {
            CompositingStrategy::Auto
        },
        ..GraphicsLayer::default()
    }
}

/// The page under `layer`, placed at `at`.
fn on_page(
    bounds: Rect,
    at: Point,
    layer: GraphicsLayer,
    children: Vec<RenderNode>,
) -> RenderGraph {
    page_graph(
        WIDTH,
        HEIGHT,
        vec![
            page_background(),
            RenderNode::Layer(Box::new(shared_test_support::layer_node(
                bounds,
                ProjectiveTransform::translation(at.x, at.y),
                layer,
                children,
            ))),
        ],
    )
}

/// The rounded layer over `children`, drawn in place or `offscreen`.
fn clipped(children: Vec<RenderNode>, offscreen: bool) -> RenderGraph {
    on_page(CLIP, AT, rounded(RADIUS, offscreen), children)
}

/// Renders the layer both ways and returns the two frames' pixels and how
/// many layers each isolated.
fn both_ways(children: Vec<RenderNode>) -> Option<([Vec<u8>; 2], [u32; 2])> {
    both_graphs(|offscreen| clipped(children.clone(), offscreen))
}

/// Renders the graph `scene` builds drawn in place, then with its rounded
/// layer offscreen, and returns the two frames and their isolated counts.
fn both_graphs(scene: impl Fn(bool) -> RenderGraph) -> Option<([Vec<u8>; 2], [u32; 2])> {
    both_graphs_with_scale(scene, 1.0, WIDTH, HEIGHT)
}

fn both_graphs_with_scale(
    scene: impl Fn(bool) -> RenderGraph,
    root_scale: f32,
    width: u32,
    height: u32,
) -> Option<([Vec<u8>; 2], [u32; 2])> {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping rounded clip in place: {err}");
            return None;
        }
    };
    let mut frames = [Vec::new(), Vec::new()];
    let mut isolated = [0, 0];
    for (index, offscreen) in [false, true].into_iter().enumerate() {
        let captured = support::capture_settled(&mut renderer, |renderer| {
            support::capture_graph_with_scale(renderer, scene(offscreen), width, height, root_scale)
        });
        isolated[index] = renderer
            .last_frame_stats()
            .expect("frame statistics")
            .isolated_layer_renders;
        frames[index] = captured.pixels;
    }
    Some((frames, isolated))
}

fn capture_graph_settled_with_scale(
    renderer: &mut support::LockedRenderer,
    graph: RenderGraph,
    width: u32,
    height: u32,
    root_scale: f32,
) -> cranpose_render_wgpu::CapturedFrame {
    support::capture_settled(renderer, |renderer| {
        support::capture_graph_with_scale(renderer, graph.clone(), width, height, root_scale)
    })
}

/// Whether the pixel at (x, y) lies in one of the clip's corner squares on
/// the page, where the clip's arcs are.
fn in_a_corner(x: u32, y: u32) -> bool {
    let (x, y) = (x as f32 + 0.5 - AT.x, y as f32 + 0.5 - AT.y);
    let near = |value: f32, extent: f32| value < RADIUS + 1.0 || value > extent - RADIUS - 1.0;
    near(x, CLIP.width) && near(y, CLIP.height) && x > -1.0 && y > -1.0
}

#[test]
fn a_fill_reaching_a_rounded_clips_corners_draws_in_place_as_its_surface_would() {
    let Some((frames, isolated)) = both_ways(vec![solid_rect(CLIP, Color(0.85, 0.15, 0.2, 1.0))])
    else {
        return;
    };
    assert_eq!(isolated, [0, 1], "in place, then through a surface");
    let differing = support::differing_pixels(WIDTH, &frames[0], &frames[1]);
    assert!(
        differing.is_empty(),
        "one fill takes the clip's coverage as the surface's mask does: {}",
        support::describe_differing(&differing)
    );
}

#[test]
fn a_bar_over_a_rounded_track_differs_from_its_surface_only_where_their_edges_meet_an_arc() {
    let track = DrawPrimitive::RoundRect {
        rect: CLIP,
        brush: Brush::solid(Color(0.85, 0.15, 0.2, 1.0)),
        radii: CornerRadii::uniform(RADIUS),
        stroke: None,
    };
    let Some((frames, isolated)) = both_ways(vec![
        draw_node(track, None),
        solid_rect(
            Rect {
                width: 70.0,
                ..CLIP
            },
            Color(0.1, 0.65, 0.3, 1.0),
        ),
    ]) else {
        return;
    };
    assert_eq!(isolated, [0, 1], "in place, then through a surface");
    let differing = support::differing_pixels(WIDTH, &frames[0], &frames[1]);
    let outside: Vec<_> = differing
        .iter()
        .filter(|(x, y, _, _)| !in_a_corner(*x as u32, *y as u32))
        .copied()
        .collect();
    assert!(
        outside.is_empty(),
        "away from the arcs, the records drawn in place match the surface: {}",
        support::describe_differing(&outside)
    );
}

/// The workspace port's bid/ask bar in miniature: a rounded clip at a
/// fractional place inside a panel that clips, `panel_width` wide.
fn in_panel(panel_width: f32, offscreen: bool) -> RenderGraph {
    let bar = Rect {
        x: 0.0,
        y: 0.0,
        width: 100.333_33,
        height: 6.0,
    };
    let panel = GraphicsLayer {
        clip: true,
        ..GraphicsLayer::default()
    };
    on_page(
        Rect {
            x: 0.0,
            y: 0.0,
            width: panel_width,
            height: 40.0,
        },
        Point::new(10.25, 10.5),
        panel,
        vec![RenderNode::Layer(Box::new(
            shared_test_support::layer_node(
                bar,
                ProjectiveTransform::translation(9.333_33, 7.666_67),
                rounded(3.0, offscreen),
                vec![solid_rect(bar, Color(0.1, 0.65, 0.3, 1.0))],
            ),
        ))],
    )
}

/// Whether the pixel at (x, y) lies within a pixel of the edge of the bar
/// `in_panel` places, at (19.583, 18.167) on the page.
fn on_the_bar_edge(x: usize, y: usize) -> bool {
    let (left, top) = (10.25 + 9.333_33, 10.5 + 7.666_67);
    let (right, bottom) = (left + 100.333_33, top + 6.0);
    let (x, y) = (x as f32 + 0.5, y as f32 + 0.5);
    let near = |value: f32, edge: f32| (value - edge).abs() < 1.0;
    let within = |value: f32, from: f32, to: f32| value > from - 1.0 && value < to + 1.0;
    (within(x, left, right) && (near(y, top) || near(y, bottom)))
        || (within(y, top, bottom) && (near(x, left) || near(x, right)))
}

#[test]
fn a_rounded_bar_at_a_fractional_place_inside_a_panel_that_holds_it_draws_in_place() {
    let Some((frames, isolated)) = both_graphs(|offscreen| in_panel(140.0, offscreen)) else {
        return;
    };
    assert_eq!(isolated, [0, 1], "in place, then through a surface");
    // Off the pixel grid the two differ only on the bar's edge: in place the
    // fill's own edge coverage meets the clip's, as Skia's analytic rounded
    // clip multiplies them, where the surface resamples a whole-pixel raster.
    let differing = support::differing_pixels(WIDTH, &frames[0], &frames[1]);
    let off_edge: Vec<_> = differing
        .iter()
        .filter(|(x, y, _, _)| !on_the_bar_edge(*x, *y))
        .copied()
        .collect();
    assert!(
        off_edge.is_empty(),
        "nothing leaks, moves or drops off the bar's edge: {}",
        support::describe_differing(&off_edge)
    );
}

#[test]
fn a_rounded_bar_a_panel_cuts_keeps_its_surface() {
    let Some((frames, isolated)) = both_graphs(|offscreen| in_panel(60.0, offscreen)) else {
        return;
    };
    assert_eq!(
        isolated,
        [1, 1],
        "a clip that cuts the rounded one leaves a surface"
    );
    let differing = support::differing_pixels(WIDTH, &frames[0], &frames[1]);
    assert!(
        differing.is_empty(),
        "both draw through the surface: {}",
        support::describe_differing(&differing)
    );
}

fn shadow_children(full_bleed: bool, recorded: bool, blend_depth: usize) -> Vec<RenderNode> {
    let fill = DrawPrimitive::Rect {
        rect: if full_bleed {
            CLIP
        } else {
            Rect {
                x: 30.0,
                y: 14.0,
                width: 60.0,
                height: 12.0,
            }
        },
        brush: Brush::solid(Color::WHITE),
        stroke: None,
    };
    let mut shadow = DrawPrimitive::Shadow(ShadowPrimitive::Drop {
        shape: Box::new(DrawPrimitive::Rect {
            rect: Rect {
                width: 20.0,
                height: 20.0,
                ..CLIP
            },
            brush: Brush::solid(Color::BLACK),
            stroke: None,
        }),
        cutout: None,
        blur_radius: 4.0,
        blend_mode: BlendMode::SrcOver,
    });
    for _ in 0..blend_depth {
        shadow = DrawPrimitive::Blend {
            primitive: Box::new(shadow),
            blend_mode: BlendMode::SrcOver,
        };
    }
    let primitives = vec![fill, shadow];
    if recorded {
        vec![RenderNode::DrawRun(DrawRunNode::new(
            PrimitivePhase::BeforeChildren,
            primitives,
        ))]
    } else {
        primitives
            .into_iter()
            .map(|primitive| draw_node(primitive, None))
            .collect()
    }
}

#[test]
fn shadows_in_recorded_and_loose_content_respect_rounded_clip_pixels() {
    let mut renderer = support::headless_renderer().expect("GPU renderer");
    for full_bleed in [false, true] {
        for (recorded, blend_depth) in [(false, 0), (true, 0), (true, 1), (true, 2)] {
            let children = shadow_children(full_bleed, recorded, blend_depth);
            let reference = capture_graph_settled(
                &mut renderer,
                clipped(children.clone(), true),
                WIDTH,
                HEIGHT,
            );
            let actual =
                capture_graph_settled(&mut renderer, clipped(children, false), WIDTH, HEIGHT);
            let label =
                format!("full_bleed={full_bleed}, recorded={recorded}, blend_depth={blend_depth}");
            support::assert_same_bytes(&label, WIDTH, &actual.pixels, &reference.pixels);
            let pixel = |x: u32, y: u32| {
                let start = ((y * WIDTH + x) * 4) as usize;
                &actual.pixels[start..start + 4]
            };
            assert_eq!(
                pixel(20, 20),
                pixel(0, 0),
                "{label}: clipped corner shows the page"
            );
            assert_ne!(
                pixel(30, 30),
                pixel(0, 0),
                "{label}: shadow is visible inside the arc"
            );
        }
    }
}

fn centered_shadow(inner: bool) -> DrawPrimitive {
    let fill = Rect {
        x: 46.0,
        y: 12.0,
        width: 28.0,
        height: 16.0,
    };
    let cutout = Rect {
        x: 51.0,
        y: 15.0,
        width: 18.0,
        height: 10.0,
    };
    let primitive = |rect, color| {
        Box::new(DrawPrimitive::Rect {
            rect,
            brush: Brush::solid(color),
            stroke: None,
        })
    };
    DrawPrimitive::Shadow(if inner {
        ShadowPrimitive::Inner {
            fill: primitive(fill, Color::BLACK),
            cutout: primitive(cutout, Color::WHITE),
            blur_radius: 4.0,
            blend_mode: BlendMode::SrcOver,
            clip_rect: fill,
        }
    } else {
        ShadowPrimitive::Drop {
            shape: primitive(fill, Color::BLACK),
            cutout: Some(primitive(cutout, Color::WHITE)),
            blur_radius: 4.0,
            blend_mode: BlendMode::SrcOver,
        }
    })
}

fn centered_shadow_children(
    inner: bool,
    recorded: bool,
    blend_depth: usize,
    full_bleed_fill: bool,
) -> Vec<RenderNode> {
    let mut shadow = centered_shadow(inner);
    for _ in 0..blend_depth {
        shadow = DrawPrimitive::Blend {
            primitive: Box::new(shadow),
            blend_mode: BlendMode::SrcOver,
        };
    }
    let primitives = vec![
        DrawPrimitive::Rect {
            rect: if full_bleed_fill {
                CLIP
            } else {
                Rect {
                    x: 30.0,
                    y: 8.0,
                    width: 60.0,
                    height: 24.0,
                }
            },
            brush: Brush::solid(Color(0.85, 0.15, 0.2, 1.0)),
            stroke: None,
        },
        shadow,
    ];
    if recorded {
        vec![RenderNode::DrawRun(DrawRunNode::new(
            PrimitivePhase::BeforeChildren,
            primitives,
        ))]
    } else {
        primitives
            .into_iter()
            .map(|primitive| draw_node(primitive, None))
            .collect()
    }
}

#[test]
fn shadows_clear_of_rounded_corners_draw_in_place_at_fractional_scale() {
    for root_scale in [1.0, 1.25] {
        let at = AT;
        let width = (WIDTH as f32 * root_scale) as u32;
        let height = (HEIGHT as f32 * root_scale) as u32;
        for inner in [false, true] {
            for (recorded, blend_depth) in [(false, 0), (false, 2), (true, 0), (true, 1), (true, 2)]
            {
                let children = centered_shadow_children(inner, recorded, blend_depth, true);
                let label = format!(
                    "inner={inner}, recorded={recorded}, blend_depth={blend_depth}, root_scale={root_scale}"
                );
                let Some((frames, isolated)) = both_graphs_with_scale(
                    |offscreen| on_page(CLIP, at, rounded(RADIUS, offscreen), children.clone()),
                    root_scale,
                    width,
                    height,
                ) else {
                    return;
                };
                assert_eq!(
                    isolated,
                    [0, 1],
                    "{label}: direct rounded draw, then isolated reference"
                );
                support::assert_bytes_within(
                    &label,
                    width,
                    &frames[0],
                    &frames[1],
                    u8::from(root_scale != 1.0),
                );
            }
        }
    }
}

#[test]
fn non_full_bleed_fill_admits_a_clear_shadow_without_a_rounded_surface() {
    for root_scale in [1.0, 1.25] {
        let width = (WIDTH as f32 * root_scale) as u32;
        let height = (HEIGHT as f32 * root_scale) as u32;
        for inner in [false, true] {
            let children = centered_shadow_children(inner, true, 1, false);
            let label = format!("inner={inner}, root_scale={root_scale}");
            let Some((frames, isolated)) = both_graphs_with_scale(
                |offscreen| on_page(CLIP, AT, rounded(RADIUS, offscreen), children.clone()),
                root_scale,
                width,
                height,
            ) else {
                return;
            };
            assert_eq!(isolated, [0, 1], "{label}: admitted draw, then reference");
            support::assert_bytes_within(
                &label,
                width,
                &frames[0],
                &frames[1],
                u8::from(root_scale != 1.0),
            );
        }
    }
}

fn blank_page() -> RenderGraph {
    page_graph(WIDTH, HEIGHT, vec![page_background()])
}

fn nested_scaled_shadow(scale: f32, offscreen_card: bool) -> RenderGraph {
    let mut card = shared_test_support::layer_node(
        CLIP,
        ProjectiveTransform::identity(),
        rounded(RADIUS, offscreen_card),
        centered_shadow_children(false, true, 1, true),
    );
    card.node_id = Some(92_001);
    let mut parent = shared_test_support::layer_node(
        CLIP,
        ProjectiveTransform::uniform_scale(scale).then(ProjectiveTransform::translation(
            AT.x + CLIP.width * (1.0 - scale) * 0.5,
            AT.y + CLIP.height * (1.0 - scale) * 0.5,
        )),
        GraphicsLayer {
            scale,
            compositing_strategy: CompositingStrategy::Offscreen,
            ..GraphicsLayer::default()
        },
        vec![RenderNode::Layer(Box::new(card))],
    );
    parent.node_id = Some(91_001);
    parent.motion_context_animated = true;
    parent.cache_policy = CachePolicy::Auto;
    page_graph(
        WIDTH,
        HEIGHT,
        vec![page_background(), RenderNode::Layer(Box::new(parent))],
    )
}

#[test]
fn nested_animated_surface_keeps_clear_shadow_out_of_rounded_corners() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping rounded clip in place: {err}");
            return;
        }
    };
    let mut reference_renderer = match support::LockedRenderer::beside_locked() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping rounded clip in place: {err}");
            return;
        }
    };
    let page = capture_graph_settled_with_scale(&mut renderer, blank_page(), WIDTH, HEIGHT, 1.0);
    for scale in [1.0, 0.75, 0.62] {
        let actual = support::capture_graph_with_scale(
            &mut renderer,
            nested_scaled_shadow(scale, false),
            WIDTH,
            HEIGHT,
            1.0,
        );
        let actual_isolated = renderer
            .last_frame_stats()
            .expect("frame statistics")
            .isolated_layer_renders;
        let reference = support::capture_graph_with_scale(
            &mut reference_renderer,
            nested_scaled_shadow(scale, true),
            WIDTH,
            HEIGHT,
            1.0,
        );
        let reference_isolated = reference_renderer
            .last_frame_stats()
            .expect("frame statistics")
            .isolated_layer_renders;
        let label = format!("parent_scale={scale}");
        if scale == 1.0 {
            assert_eq!(
                actual_isolated, 1,
                "{label}: only the animated parent isolates"
            );
            assert_eq!(
                reference_isolated, 2,
                "{label}: rounded reference adds one surface"
            );
        }

        let corner_x = (AT.x + CLIP.width * 0.5 + (1.5 - CLIP.width * 0.5) * scale) as u32;
        let corner_y = (AT.y + CLIP.height * 0.5 + (1.5 - CLIP.height * 0.5) * scale) as u32;
        let corner = Rect {
            x: corner_x as f32,
            y: corner_y as f32,
            width: 1.0,
            height: 1.0,
        };
        assert_eq!(
            support::region_pixels(&actual, corner),
            support::region_pixels(&page, corner),
            "{label}: rounded corner stays transparent to the parent surface"
        );
        assert_eq!(
            support::region_pixels(&actual, corner),
            support::region_pixels(&reference, corner),
            "{label}: direct clip matches the nested rounded-surface corner"
        );
    }
}

fn shadow_corner_children(descendant: RenderNode) -> Vec<RenderNode> {
    vec![
        solid_rect(
            Rect {
                x: 20.0,
                y: 8.0,
                width: 80.0,
                height: 24.0,
            },
            Color(0.25, 0.45, 0.75, 1.0),
        ),
        descendant,
    ]
}

fn text_shadow_node() -> RenderNode {
    let shadow = cranpose_ui::text::Shadow {
        color: Color::BLACK,
        offset: Point::new(-28.0, -16.0),
        blur_radius: 8.0,
    };
    let style = SpanStyle {
        color: Some(Color::BLACK),
        font_size: TextUnit::Sp(18.0),
        shadow: Some(shadow),
        ..SpanStyle::default()
    };
    let annotated = AnnotatedString {
        text: "Shadow".to_owned(),
        ..AnnotatedString::default()
    };
    let render_text = std::sync::Arc::new(annotated.render_string());
    RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Text(Box::new(TextPrimitiveNode {
            node_id: 89_001,
            rect: Rect {
                x: 24.0,
                y: 14.0,
                width: 56.0,
                height: 20.0,
            },
            text: std::rc::Rc::new(annotated),
            render_text,
            text_style: std::sync::Arc::new(TextStyle::from_span_style(style)),
            font_size: 18.0,
            layout_options: TextLayoutOptions::default(),
            clip: None,
        })),
    })
}

fn assert_corner_matches_rounded_surface(descendant: RenderNode, label: &str) {
    let Some((frames, _)) =
        both_graphs(|offscreen| clipped(shadow_corner_children(descendant.clone()), offscreen))
    else {
        return;
    };
    let corner_index = (((AT.y as u32 + 1) * WIDTH + AT.x as u32 + 1) * 4) as usize;
    assert_eq!(
        &frames[0][corner_index..corner_index + 4],
        &frames[1][corner_index..corner_index + 4],
        "{label}: descendant output stays outside the rounded corner"
    );
}

#[test]
fn descendant_elevation_shadow_respects_rounded_ancestor_corners() {
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 10.0,
        height: 8.0,
    };
    let mut shadow_layer = shared_test_support::layer_node(
        bounds,
        ProjectiveTransform::translation(24.0, 14.0),
        GraphicsLayer {
            shadow_elevation: 40.0,
            ..GraphicsLayer::default()
        },
        vec![solid_rect(bounds, Color::WHITE)],
    );
    shadow_layer.node_id = Some(89_002);
    assert_corner_matches_rounded_surface(
        RenderNode::Layer(Box::new(shadow_layer)),
        "elevation shadow",
    );
}

#[test]
fn alpha_isolated_full_clip_child_respects_rounded_ancestor_corners() {
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 24.0,
        height: 24.0,
    };
    let mut child = shared_test_support::layer_node(
        bounds,
        ProjectiveTransform::identity(),
        GraphicsLayer {
            alpha: 0.5,
            ..GraphicsLayer::default()
        },
        vec![solid_rect(bounds, Color(0.1, 0.7, 0.2, 1.0))],
    );
    child.draws_within_bounds = true;
    assert_corner_matches_rounded_surface(
        RenderNode::Layer(Box::new(child)),
        "alpha-isolated full-clip child",
    );
}

#[test]
fn effect_output_padding_respects_rounded_ancestor_corners() {
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 16.0,
        height: 12.0,
    };
    let source = format!(
        "{RUNTIME_SHADER_PRELUDE_WGSL}\n@fragment\nfn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{ return vec4<f32>(0.1, 0.8, 0.2, 1.0); }}\n"
    );
    let mut shader = RuntimeShader::new(&source);
    shader.set_output_padding(18.0);
    let mut child = shared_test_support::layer_node(
        bounds,
        ProjectiveTransform::translation(18.0, 12.0),
        GraphicsLayer {
            render_effect: Some(RenderEffect::runtime_shader(shader)),
            ..GraphicsLayer::default()
        },
        vec![RenderNode::DrawRun(DrawRunNode::new(
            PrimitivePhase::BeforeChildren,
            vec![DrawPrimitive::Rect {
                rect: bounds,
                brush: Brush::solid(Color::WHITE),
                stroke: None,
            }],
        ))],
    );
    child.node_id = Some(89_003);
    child.draws_within_bounds = child.content_draws_within_bounds();
    assert_corner_matches_rounded_surface(
        RenderNode::Layer(Box::new(child)),
        "effect output padding",
    );
}

#[test]
fn base_text_shadow_respects_rounded_ancestor_corners() {
    assert_corner_matches_rounded_surface(text_shadow_node(), "base text shadow");
}

#[test]
fn nested_contained_layers_keep_their_cumulative_draw_slack_visible() {
    let color = Color(0.15, 0.7, 0.35, 1.0);
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 10.0,
        height: 10.0,
    };
    let clip_bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 50.0,
        height: 20.0,
    };
    let make_graph = |nested: bool| {
        let content = if nested {
            let mut child = RenderNode::DrawRun(DrawRunNode::new(
                PrimitivePhase::BeforeChildren,
                vec![DrawPrimitive::Rect {
                    rect: bounds,
                    brush: Brush::solid(color),
                    stroke: None,
                }],
            ));
            for _ in 0..4 {
                let mut layer = shared_test_support::layer_node(
                    bounds,
                    ProjectiveTransform::translation(-1.0, 0.0),
                    GraphicsLayer::default(),
                    vec![child],
                );
                layer.draws_within_bounds = layer.content_draws_within_bounds();
                child = RenderNode::Layer(Box::new(layer));
            }
            let mut outer = shared_test_support::layer_node(
                bounds,
                ProjectiveTransform::translation(52.0, 4.0),
                GraphicsLayer::default(),
                vec![child],
            );
            outer.draws_within_bounds = outer.content_draws_within_bounds();
            RenderNode::Layer(Box::new(outer))
        } else {
            RenderNode::DrawRun(DrawRunNode::new(
                PrimitivePhase::BeforeChildren,
                vec![DrawPrimitive::Rect {
                    rect: Rect {
                        x: 48.0,
                        y: 4.0,
                        ..bounds
                    },
                    brush: Brush::solid(color),
                    stroke: None,
                }],
            ))
        };
        let mut clip = shared_test_support::layer_node(
            clip_bounds,
            ProjectiveTransform::identity(),
            GraphicsLayer {
                clip: true,
                ..GraphicsLayer::default()
            },
            vec![content],
        );
        clip.draws_within_bounds = clip.content_draws_within_bounds();
        page_graph(
            WIDTH,
            HEIGHT,
            vec![page_background(), RenderNode::Layer(Box::new(clip))],
        )
    };

    let Some((frames, _)) = both_graphs(make_graph) else {
        return;
    };
    let visible_pixel = ((8 * WIDTH + 48) * 4) as usize;
    let clipped_pixel = ((8 * WIDTH + 51) * 4) as usize;
    assert_eq!(
        &frames[0][visible_pixel..visible_pixel + 8],
        &frames[1][visible_pixel..visible_pixel + 8],
        "nested content matches the same rect drawn directly under the parent clip"
    );
    assert_ne!(
        &frames[0][visible_pixel..visible_pixel + 4],
        &frames[0][clipped_pixel..clipped_pixel + 4],
        "the reference rect visibly covers the pixels at the clip edge"
    );
}

#[test]
fn a_shadow_at_an_arbitrary_fractional_phase_stays_inside_the_rounded_clip() {
    let root_scale = 1.25;
    let width = (WIDTH as f32 * root_scale) as u32;
    let height = (HEIGHT as f32 * root_scale) as u32;
    let at = Point::new(20.25, 20.5);
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping rounded clip in place: {err}");
            return;
        }
    };
    let page =
        capture_graph_settled_with_scale(&mut renderer, blank_page(), width, height, root_scale);
    let card = |children| on_page(CLIP, at, rounded(RADIUS, false), children);
    let fill = capture_graph_settled_with_scale(
        &mut renderer,
        card(vec![solid_rect(CLIP, Color(0.85, 0.15, 0.2, 1.0))]),
        width,
        height,
        root_scale,
    );
    for inner in [false, true] {
        let actual = capture_graph_settled_with_scale(
            &mut renderer,
            card(centered_shadow_children(inner, true, 1, true)),
            width,
            height,
            root_scale,
        );
        let corner_x = ((at.x + 1.0) * root_scale) as u32;
        let corner_y = ((at.y + 1.0) * root_scale) as u32;
        let corner = Rect {
            x: corner_x as f32,
            y: corner_y as f32,
            width: 1.0,
            height: 1.0,
        };
        let label = format!("inner={inner}");
        assert_eq!(
            support::region_pixels(&actual, corner),
            support::region_pixels(&fill, corner),
            "{label}: the shadow does not reach the clipped corner"
        );
        assert_eq!(
            support::region_pixels(&actual, corner),
            support::region_pixels(&page, corner),
            "{label}: the rounded clip leaves the page visible in its corner"
        );

        let differing = support::differing_pixels(width, &fill.pixels, &actual.pixels);
        let center_left = ((at.x + 35.0) * root_scale) as usize;
        let center_top = ((at.y + 2.0) * root_scale) as usize;
        let center_right = ((at.x + 85.0) * root_scale) as usize;
        let center_bottom = ((at.y + 38.0) * root_scale) as usize;
        assert!(
            differing.iter().any(|(x, y, _, _)| {
                *x >= center_left && *x < center_right && *y >= center_top && *y < center_bottom
            }),
            "{label}: the shadow remains visible inside the rounded clip"
        );
    }
}

fn backdrop_over_rounded_fill(radius: f32) -> RenderGraph {
    let mut graph = on_page(
        CLIP,
        AT,
        rounded(radius, false),
        vec![solid_rect(CLIP, Color(0.85, 0.15, 0.2, 1.0))],
    );
    let mut backdrop = shared_test_support::layer_node(
        CLIP,
        ProjectiveTransform::translation(AT.x, AT.y),
        GraphicsLayer {
            backdrop_effect: Some(RenderEffect::blur(4.0)),
            clip: true,
            ..GraphicsLayer::default()
        },
        Vec::new(),
    );
    backdrop.node_id = Some(901);
    graph
        .root
        .children
        .push(RenderNode::Layer(Box::new(backdrop)));
    graph
}

#[test]
fn a_backdrop_updates_when_only_the_captured_clip_radius_changes() {
    let mut renderer = support::headless_renderer().expect("GPU renderer");
    let mut fresh = support::LockedRenderer::beside_locked().expect("reference GPU renderer");
    let initial = capture_graph_settled(
        &mut renderer,
        backdrop_over_rounded_fill(4.0),
        WIDTH,
        HEIGHT,
    );
    for _ in 0..3 {
        support::capture_graph(
            &mut renderer,
            backdrop_over_rounded_fill(4.0),
            WIDTH,
            HEIGHT,
        );
    }
    let expected =
        capture_graph_settled(&mut fresh, backdrop_over_rounded_fill(20.0), WIDTH, HEIGHT);
    assert_ne!(
        initial.pixels, expected.pixels,
        "the radius change must alter visible pixels"
    );
    for frame in 0..3 {
        let actual = support::capture_graph(
            &mut renderer,
            backdrop_over_rounded_fill(20.0),
            WIDTH,
            HEIGHT,
        );
        support::assert_same_bytes(
            &format!("radius change, frame {frame}"),
            WIDTH,
            &actual.pixels,
            &expected.pixels,
        );
    }
}
