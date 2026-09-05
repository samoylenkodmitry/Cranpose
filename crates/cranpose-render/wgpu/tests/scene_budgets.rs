mod support;

#[path = "../src/test_support.rs"]
mod shared_test_support;

use cranpose_render_common::{
    Renderer,
    graph::{
        CachePolicy, DrawPrimitiveNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase,
        ProjectiveTransform, RenderGraph, RenderNode,
    },
};
use cranpose_render_wgpu::RenderStatsSnapshot;
use cranpose_ui_graphics::{
    Brush, Color, CornerRadii, DrawPrimitive, GraphicsLayer, Point, Rect, Stroke, StrokeCap,
    StrokeJoin,
};

const ARENA: u32 = 900;
const RINGS: usize = 20;
const BRICKS_PER_RING: usize = 60;
const DOTS_PER_RING: usize = 600;
const FRAMES: usize = 3;

/// The published renderer's third frame of the spinning arena: every ring's
/// surface comes from the layer cache and only the composites' transforms
/// reach the GPU, twenty blit blocks each padded to the device's uniform
/// offset alignment in the frame's one uniform write, over two submits.
const ARENA_MAX_PASSES: u32 = 23;
const ARENA_MAX_UPLOAD_BYTES: u64 = 2 * (20 * 256 + 1024);
fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn primitive(primitive: DrawPrimitive) -> RenderNode {
    RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Draw(DrawPrimitiveNode {
            primitive,
            clip: None,
        }),
    })
}

fn circle(center: Point, radius: f32, color: Color, stroke: Option<Stroke>) -> RenderNode {
    primitive(DrawPrimitive::RoundRect {
        rect: rect(
            center.x - radius,
            center.y - radius,
            2.0 * radius,
            2.0 * radius,
        ),
        brush: Brush::solid(color),
        radii: CornerRadii::uniform(radius),
        stroke,
    })
}

/// One spinning ring of the arena: a stroked rim, arc bricks and dot chains
/// in a layer sized to the ring, turned by `angle` about the arena's center.
fn ring(index: usize, angle: f32) -> RenderNode {
    let arena_center = Point::new(ARENA as f32 / 2.0, ARENA as f32 / 2.0);
    let radius = 60.0 + index as f32 * 19.0;
    let extent = radius + 8.0;
    let center = Point::new(extent, extent);
    let bounds = rect(0.0, 0.0, 2.0 * extent, 2.0 * extent);
    let mut children = vec![circle(
        center,
        radius,
        Color::from_rgb_u8(200, 120, 220),
        Some(Stroke {
            width: 5.0,
            cap: StrokeCap::Butt,
            join: StrokeJoin::Miter,
        }),
    )];
    for brick in 0..BRICKS_PER_RING {
        let start = brick as f32 * std::f32::consts::TAU / BRICKS_PER_RING as f32;
        children.push(primitive(DrawPrimitive::Arc {
            rect: rect(
                center.x - radius,
                center.y - radius,
                2.0 * radius,
                2.0 * radius,
            ),
            brush: Brush::solid(Color::from_rgb_u8(90, 200, 240)),
            center,
            radius: radius - 2.0,
            start_angle: start,
            sweep_angle: 0.08,
            stroke: None,
            inner_radius: radius - 14.0,
        }));
    }
    for dot in 0..DOTS_PER_RING {
        let theta = dot as f32 * std::f32::consts::TAU / DOTS_PER_RING as f32;
        let ring_radius = radius - 8.0;
        children.push(circle(
            Point::new(
                center.x + theta.cos() * ring_radius,
                center.y + theta.sin() * ring_radius,
            ),
            3.0,
            Color::from_rgb_u8(240, 200, 240),
            None,
        ));
    }
    let placed = rect(
        arena_center.x - extent,
        arena_center.y - extent,
        2.0 * extent,
        2.0 * extent,
    );
    let (sin, cos) = angle.sin_cos();
    let corner = |x: f32, y: f32| {
        let dx = x - arena_center.x;
        let dy = y - arena_center.y;
        [
            arena_center.x + dx * cos - dy * sin,
            arena_center.y + dx * sin + dy * cos,
        ]
    };
    let mut layer = shared_test_support::layer_node(
        bounds,
        ProjectiveTransform::from_rect_to_quad(
            bounds,
            [
                corner(placed.x, placed.y),
                corner(placed.x + placed.width, placed.y),
                corner(placed.x, placed.y + placed.height),
                corner(placed.x + placed.width, placed.y + placed.height),
            ],
        ),
        GraphicsLayer::default(),
        children,
    );
    layer.cache_policy = CachePolicy::Auto;
    RenderNode::Layer(Box::new(layer))
}

fn arena(frame: usize) -> RenderGraph {
    let mut children = vec![primitive(DrawPrimitive::Rect {
        rect: rect(0.0, 0.0, ARENA as f32, ARENA as f32),
        brush: Brush::solid(Color::from_rgb_u8(8, 8, 16)),
        stroke: None,
    })];
    for index in 0..RINGS {
        let speed = if index % 2 == 0 { 0.01 } else { -0.008 };
        children.push(ring(index, frame as f32 * speed * (index + 1) as f32));
    }
    RenderGraph::new(shared_test_support::layer_node(
        rect(0.0, 0.0, ARENA as f32, ARENA as f32),
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        children,
    ))
}

fn third_frame_stats(
    renderer: &mut support::LockedRenderer,
    width: u32,
    height: u32,
    graph: impl Fn(usize) -> RenderGraph,
) -> RenderStatsSnapshot {
    for frame in 0..FRAMES {
        renderer.scene_mut().graph = Some(graph(frame));
        renderer
            .capture_frame(width, height)
            .expect("capture should succeed");
    }
    renderer.last_frame_stats().expect("stats")
}

fn report(scene: &str, stats: &RenderStatsSnapshot) {
    eprintln!(
        "BUDGET scene={scene} passes={} pass_px={} uploads={} draws={} isolated={} blur={} composites={} cache_hits={} cache_misses={}",
        stats.pass_count,
        stats.pass_pixels,
        stats.upload_bytes,
        stats.draw_calls,
        stats.isolated_layer_renders,
        stats.blur_passes,
        stats.composite_passes,
        stats.layer_cache_hits,
        stats.layer_cache_misses
    );
}

#[test]
fn a_spinning_arena_of_thirteen_thousand_shapes_stays_within_budget() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping scene budgets: {err}");
            return;
        }
    };
    let stats = third_frame_stats(&mut renderer, ARENA, ARENA, arena);
    report("arena", &stats);
    assert!(
        stats.pass_count <= ARENA_MAX_PASSES,
        "arena passes {} over {ARENA_MAX_PASSES}: {stats:?}",
        stats.pass_count
    );
    assert_eq!(
        stats.isolated_layer_renders, 0,
        "a ring whose content did not change must come from the layer cache: {stats:?}"
    );
    assert!(
        stats.upload_bytes <= ARENA_MAX_UPLOAD_BYTES,
        "arena uploads {} over {ARENA_MAX_UPLOAD_BYTES}: {stats:?}",
        stats.upload_bytes
    );
}
