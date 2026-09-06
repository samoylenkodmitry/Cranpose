mod support;

#[path = "../src/test_support.rs"]
mod shared_test_support;

use cranpose_render_common::{
    Renderer,
    graph::{
        DrawCommandId, DrawRunNode, PrimitivePhase, ProjectiveTransform, RenderGraph, RenderNode,
    },
    style_shared::DrawPlacement,
};
use cranpose_render_wgpu::{RenderStatsSnapshot, WgpuRenderer};
use cranpose_ui_graphics::{
    Brush, Color, CornerRadii, DrawScope, DrawScopeDefault, GraphicsLayer, Point, Rect,
    RenderEffect, Size, Stroke, TileMode,
};

const FRAME_WIDTH: u32 = 240;
const FRAME_HEIGHT: u32 = 160;
const LAYER: Rect = Rect {
    x: 8.0,
    y: 6.0,
    width: 224.0,
    height: 148.0,
};
const PREFIX_KIND: usize = 3;
const NO_FILL_CACHE: &str = "CRANPOSE_NO_FILL_CACHE";
const GPU_STATS: &str = "CRANPOSE_GPU_STATS";

#[derive(Clone, Copy, PartialEq)]
enum First {
    Radial,
    RadialFiveStops,
    Solid,
    OtherRadial,
    TranslucentStop,
    RepeatedTile,
    Stroked,
    Rounded,
    FractionalEdge,
    Circle,
}

#[derive(Clone, Copy)]
struct Spec {
    first: First,
    alpha: f32,
    effect_beneath: bool,
    covers_page: bool,
}

impl Spec {
    const fn of(first: First) -> Self {
        Self {
            first,
            alpha: 1.0,
            effect_beneath: false,
            covers_page: false,
        }
    }

    const fn covering_page(self) -> Self {
        Self {
            covers_page: true,
            ..self
        }
    }

    fn layer_rect(self) -> Rect {
        if self.covers_page {
            Rect {
                x: 0.0,
                y: 0.0,
                width: FRAME_WIDTH as f32,
                height: FRAME_HEIGHT as f32,
            }
        } else {
            LAYER
        }
    }
}

fn stops(last: Color) -> Vec<(f32, Color)> {
    vec![
        (0.0, Color::from_rgb_u8(24, 20, 46)),
        (0.55, Color::from_rgb_u8(11, 10, 26)),
        (1.0, last),
    ]
}

fn radial(stops: Vec<(f32, Color)>, tile_mode: TileMode) -> Brush {
    Brush::radial_gradient_stops(
        stops,
        Point::new(LAYER.width * 0.5, LAYER.height * 0.1),
        LAYER.width.max(LAYER.height) * 0.95,
        tile_mode,
    )
}

fn primitives(spec: Spec, phase: u32) -> Vec<cranpose_ui_graphics::DrawPrimitive> {
    let layer = spec.layer_rect();
    let mut scope = DrawScopeDefault::new(Size::new(layer.width, layer.height));
    let dark = Color::from_rgb_u8(4, 4, 10);
    match spec.first {
        First::Radial => scope.draw_rect(radial(stops(dark), TileMode::Clamp)),
        First::OtherRadial => {
            scope.draw_rect(radial(stops(Color::from_rgb_u8(4, 12, 4)), TileMode::Clamp))
        }
        First::RadialFiveStops => scope.draw_rect(radial(
            vec![
                (0.0, Color::from_rgb_u8(40, 20, 60)),
                (0.2, Color::from_rgb_u8(30, 30, 70)),
                (0.5, Color::from_rgb_u8(20, 20, 40)),
                (0.8, Color::from_rgb_u8(10, 12, 24)),
                (1.0, dark),
            ],
            TileMode::Clamp,
        )),
        First::Solid => scope.draw_rect(Brush::solid(Color::from_rgb_u8(11, 10, 26))),
        First::TranslucentStop => scope.draw_rect(radial(
            vec![
                (0.0, Color::from_rgb_u8(24, 20, 46)),
                (1.0, Color::from_rgba_u8(4, 4, 10, 200)),
            ],
            TileMode::Clamp,
        )),
        First::RepeatedTile => scope.draw_rect(radial(stops(dark), TileMode::Repeated)),
        First::Stroked => scope.draw_rect_stroked(
            radial(stops(dark), TileMode::Clamp),
            Stroke {
                width: 6.0,
                ..Stroke::default()
            },
        ),
        First::Rounded => scope.draw_round_rect(
            radial(stops(dark), TileMode::Clamp),
            CornerRadii {
                top_left: 12.0,
                top_right: 12.0,
                bottom_right: 12.0,
                bottom_left: 12.0,
            },
        ),
        First::FractionalEdge => scope.draw_rect_at(
            Rect {
                x: 0.5,
                y: 0.0,
                width: LAYER.width - 0.5,
                height: LAYER.height,
            },
            radial(stops(dark), TileMode::Clamp),
        ),
        First::Circle => {
            scope.draw_circle(
                Brush::solid(Color::from_rgb_u8(200, 200, 220)),
                Point::new(20.0, 20.0),
                9.0,
            );
            scope.draw_rect(radial(stops(dark), TileMode::Clamp));
        }
    }
    for star in 0..40u32 {
        let drift = (phase * 7 + star * 3) as f32;
        let x = (star as f32 * 41.3 + drift) % layer.width;
        let y = (star as f32 * 23.7 + drift * 0.5) % layer.height;
        scope.draw_circle(
            Brush::solid(Color::from_rgba_u8(
                255,
                240,
                200,
                150 + (star % 4) as u8 * 25,
            )),
            Point::new(x, y),
            1.0 + (star % 4) as f32,
        );
    }
    scope.into_primitives()
}

fn graph(spec: Spec, phase: u32) -> RenderGraph {
    let run = DrawRunNode::for_command(
        PrimitivePhase::BeforeChildren,
        Some(DrawCommandId {
            node_id: 7,
            command_index: 0,
            placement: DrawPlacement::Behind,
        }),
        primitives(spec, phase),
    );
    let placed = spec.layer_rect();
    let layer = RenderNode::Layer(Box::new(shared_test_support::layer_node(
        Rect {
            x: 0.0,
            y: 0.0,
            width: placed.width,
            height: placed.height,
        },
        ProjectiveTransform::translation(placed.x, placed.y),
        GraphicsLayer {
            alpha: spec.alpha,
            ..GraphicsLayer::default()
        },
        vec![RenderNode::DrawRun(run)],
    )));
    let mut children = Vec::new();
    if spec.effect_beneath {
        children.push(RenderNode::Layer(Box::new(
            shared_test_support::layer_node(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 60.0,
                    height: 40.0,
                },
                ProjectiveTransform::translation(20.0, 16.0),
                GraphicsLayer {
                    render_effect: Some(RenderEffect::blur(4.0)),
                    ..GraphicsLayer::default()
                },
                vec![support::solid_rect(
                    Rect {
                        x: 8.0,
                        y: 8.0,
                        width: 44.0,
                        height: 24.0,
                    },
                    Color::from_rgb_u8(240, 120, 60),
                )],
            ),
        )));
    }
    children.push(layer);
    support::page_graph(FRAME_WIDTH, FRAME_HEIGHT, children)
}

struct Pair {
    cached: support::LockedRenderer,
    reference: WgpuRenderer,
}

impl Pair {
    fn new() -> Option<Self> {
        let cached = match support::headless_renderer() {
            Ok(renderer) => renderer,
            Err(err) => {
                eprintln!("skipping (headless WGPU init failed): {err}");
                return None;
            }
        };
        let reference = support::headless_renderer_beside_locked().expect("second renderer");
        cranpose_render_wgpu::set_debug_toggle(GPU_STATS, Some("1"));
        Some(Self { cached, reference })
    }

    fn frame(
        &mut self,
        label: &str,
        spec: Spec,
        phase: u32,
        scale: f32,
    ) -> (RenderStatsSnapshot, RenderStatsSnapshot) {
        let cached = support::capture_graph_with_scale(
            &mut self.cached,
            graph(spec, phase),
            FRAME_WIDTH,
            FRAME_HEIGHT,
            scale,
        );
        let cached_stats = self.cached.last_frame_stats().expect("stats");
        cranpose_render_wgpu::set_debug_toggle(NO_FILL_CACHE, Some("1"));
        self.reference.scene_mut().graph = Some(graph(spec, phase));
        let reference = self
            .reference
            .capture_frame_with_scale(FRAME_WIDTH, FRAME_HEIGHT, scale)
            .expect("reference capture");
        cranpose_render_wgpu::set_debug_toggle(NO_FILL_CACHE, None);
        let reference_stats = self.reference.last_frame_stats().expect("stats");
        support::assert_same_bytes(
            &format!("{label}, frame {phase}"),
            FRAME_WIDTH,
            &cached.pixels,
            &reference.pixels,
        );
        (cached_stats, reference_stats)
    }
}

impl Drop for Pair {
    fn drop(&mut self) {
        cranpose_render_wgpu::set_debug_toggle(GPU_STATS, None);
        cranpose_render_wgpu::set_debug_toggle(NO_FILL_CACHE, None);
    }
}

fn prefix_hits(stats: &RenderStatsSnapshot) -> u32 {
    stats.layer_cache_hits_by_kind[PREFIX_KIND]
}

fn assert_cold_then_warm(pair: &mut Pair, label: &str, spec: Spec, scale: f32) {
    let (first, _) = pair.frame(label, spec, 0, scale);
    assert_eq!(
        first.prefix_admissions, 0,
        "{label}: the first frame is watched, not admitted"
    );
    assert_eq!(
        prefix_hits(&first),
        0,
        "{label}: nothing is cached on the first frame"
    );
    let (second, _) = pair.frame(label, spec, 1, scale);
    assert_eq!(
        second.prefix_admissions, 1,
        "{label}: the second same frame admits the prefix"
    );
    assert_eq!(
        prefix_hits(&second),
        0,
        "{label}: the admitting frame draws the prefix itself"
    );
    for phase in 2..5 {
        let (warm, reference) = pair.frame(label, spec, phase, scale);
        assert_eq!(
            prefix_hits(&warm),
            1,
            "{label}: frame {phase} serves the prefix from the cache"
        );
        assert_eq!(
            warm.prefix_admissions, 0,
            "{label}: frame {phase} admits nothing"
        );
        assert!(
            warm.shape_fill_pixels + 1000 < reference.shape_fill_pixels,
            "{label}: the warm frame must fill less than the reference that draws the prefix: \
             {} against {}",
            warm.shape_fill_pixels,
            reference.shape_fill_pixels
        );
        let copied = if spec.covers_page {
            (
                reference.copy_count + 1,
                reference.copy_pixels + u64::from(FRAME_WIDTH) * u64::from(FRAME_HEIGHT),
            )
        } else {
            (reference.copy_count, reference.copy_pixels)
        };
        assert_eq!(
            (warm.copy_count, warm.copy_pixels),
            copied,
            "{label}: frame {phase} copies a page-covering prefix into the page and composites \
             a partial one"
        );
    }
}

fn assert_never_admitted(pair: &mut Pair, label: &str, spec: Spec, scale: f32) {
    for phase in 0..4 {
        let (stats, _) = pair.frame(label, spec, phase, scale);
        assert_eq!(
            stats.prefix_admissions, 0,
            "{label}: frame {phase} must not admit"
        );
        assert_eq!(
            prefix_hits(&stats),
            0,
            "{label}: frame {phase} must not hit"
        );
    }
}

#[test]
fn an_opaque_gradient_ahead_of_moving_stars_is_drawn_once_and_reused_byte_for_byte() {
    let Some(mut pair) = Pair::new() else {
        return;
    };
    assert_cold_then_warm(&mut pair, "three-stop radial", Spec::of(First::Radial), 1.0);
}

#[test]
fn the_reuse_holds_at_fractional_and_integer_scales_and_past_the_page_edge() {
    let Some(mut pair) = Pair::new() else {
        return;
    };
    assert_cold_then_warm(&mut pair, "radial at 1.5x", Spec::of(First::Radial), 1.5);
    assert_cold_then_warm(&mut pair, "radial at 3x", Spec::of(First::Radial), 3.0);
}

#[test]
fn a_prefix_covering_the_page_is_copied_in_place_of_the_composite_at_every_scale() {
    let Some(mut pair) = Pair::new() else {
        return;
    };
    let spec = Spec::of(First::Radial).covering_page();
    assert_cold_then_warm(&mut pair, "covering at 1x", spec, 1.0);
    assert_cold_then_warm(&mut pair, "covering at 1.5x", spec, 1.5);
    assert_cold_then_warm(&mut pair, "covering at 3x", spec, 3.0);
}

#[test]
fn five_stop_and_solid_fills_are_reused_too() {
    let Some(mut pair) = Pair::new() else {
        return;
    };
    assert_cold_then_warm(
        &mut pair,
        "five stops",
        Spec::of(First::RadialFiveStops),
        1.0,
    );
    assert_cold_then_warm(&mut pair, "solid", Spec::of(First::Solid), 1.0);
}

#[test]
fn a_changed_stop_colour_misses_and_is_readmitted_with_the_new_bytes() {
    let Some(mut pair) = Pair::new() else {
        return;
    };
    assert_cold_then_warm(&mut pair, "before the change", Spec::of(First::Radial), 1.0);
    let changed = Spec::of(First::OtherRadial);
    let (miss, _) = pair.frame("changed colour", changed, 5, 1.0);
    assert_eq!(
        prefix_hits(&miss),
        0,
        "a changed stop colour must not hit the old entry"
    );
    assert!(
        miss.layer_cache_misses_by_kind[PREFIX_KIND] >= 1,
        "the changed prefix is a miss"
    );
    assert_eq!(
        miss.prefix_admissions, 0,
        "a key seen once is watched, not admitted"
    );
    let (readmit, _) = pair.frame("changed colour", changed, 6, 1.0);
    assert_eq!(
        readmit.prefix_admissions, 1,
        "the new key admits on its second frame"
    );
    let (warm, _) = pair.frame("changed colour", changed, 7, 1.0);
    assert_eq!(prefix_hits(&warm), 1, "the new bytes are served afterwards");
}

#[test]
fn fills_that_are_not_provably_opaque_plain_rects_on_whole_pixels_are_never_cached() {
    let Some(mut pair) = Pair::new() else {
        return;
    };
    assert_never_admitted(
        &mut pair,
        "translucent stop",
        Spec::of(First::TranslucentStop),
        1.0,
    );
    assert_never_admitted(
        &mut pair,
        "repeated tile",
        Spec::of(First::RepeatedTile),
        1.0,
    );
    assert_never_admitted(&mut pair, "stroked", Spec::of(First::Stroked), 1.0);
    assert_never_admitted(&mut pair, "rounded", Spec::of(First::Rounded), 1.0);
    assert_never_admitted(
        &mut pair,
        "fractional edge",
        Spec::of(First::FractionalEdge),
        1.0,
    );
    assert_never_admitted(
        &mut pair,
        "not the first record",
        Spec::of(First::Circle),
        1.0,
    );
    assert_never_admitted(
        &mut pair,
        "translucent layer",
        Spec {
            alpha: 0.9,
            ..Spec::of(First::Radial)
        },
        1.0,
    );
    assert_never_admitted(
        &mut pair,
        "an effect composited beneath the prefix",
        Spec {
            effect_beneath: true,
            ..Spec::of(First::Radial)
        },
        1.0,
    );
}
