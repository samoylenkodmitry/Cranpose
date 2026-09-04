mod support;

use cranpose_render_common::{
    Renderer,
    graph::{
        DrawPrimitiveNode, DrawRunNode, LayerNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase,
        RenderGraph, RenderNode,
    },
};
use cranpose_ui_graphics::{Brush, Color, DrawScope, DrawScopeDefault, Point, Rect};

const SIZE: u32 = 256;
const CENTER: f32 = 128.0;

fn record_solid_scene(scope: &mut DrawScopeDefault) {
    scope.draw_rect_at(
        Rect {
            x: 0.0,
            y: 0.0,
            width: SIZE as f32,
            height: SIZE as f32,
        },
        Brush::solid(Color(0.02, 0.02, 0.05, 1.0)),
    );
    for ring in 0..3u32 {
        let radius = 40.0 + ring as f32 * 28.0;
        let count = 48;
        for i in 0..count {
            let start = i as f32 * (std::f32::consts::TAU / count as f32) + ring as f32 * 0.11;
            scope.draw_annular_sector(
                Brush::solid(Color(0.3, 0.5 + (i % 5) as f32 * 0.08, 0.8, 1.0)),
                Point::new(CENTER, CENTER),
                radius - 6.0,
                radius,
                start,
                0.09,
            );
        }
    }
    for m in 0..7u32 {
        scope.draw_circle(
            Brush::solid(Color(1.0, 0.85, 0.4, 0.9)),
            Point::new(24.0 + m as f32 * 32.0, 20.0),
            5.5,
        );
    }
    scope.draw_round_rect_at(
        Rect {
            x: 30.0,
            y: 220.0,
            width: 80.0,
            height: 24.0,
        },
        Brush::solid(Color(0.8, 0.3, 0.4, 1.0)),
        cranpose_ui_graphics::CornerRadii::uniform(8.0),
    );
    scope.draw_rect_at_stroked(
        Rect {
            x: 140.0,
            y: 218.0,
            width: 84.0,
            height: 28.0,
        },
        Brush::solid(Color(0.4, 0.9, 0.6, 1.0)),
        cranpose_ui_graphics::Stroke {
            width: 3.0,
            ..Default::default()
        },
    );
}

fn record_mixed_scene(scope: &mut DrawScopeDefault) {
    record_solid_scene(scope);
    scope.draw_rect_at(
        Rect {
            x: 4.0,
            y: 120.0,
            width: 8.0,
            height: 8.0,
        },
        Brush::linear_gradient(vec![Color(1.0, 0.0, 0.0, 0.0), Color(0.0, 1.0, 0.0, 0.0)]),
    );
    scope.draw_rect_at(
        Rect {
            x: 150.0,
            y: 40.0,
            width: 90.0,
            height: 60.0,
        },
        Brush::radial_gradient(
            vec![Color(0.9, 0.2, 0.2, 1.0), Color(0.1, 0.1, 0.6, 0.2)],
            Point::new(195.0, 70.0),
            50.0,
        ),
    );
    scope.draw_rect_at_stroked(
        Rect {
            x: 60.0,
            y: 150.0,
            width: 120.0,
            height: 50.0,
        },
        Brush::linear_gradient(vec![Color(0.2, 0.9, 0.3, 1.0), Color(0.9, 0.9, 0.1, 1.0)]),
        cranpose_ui_graphics::Stroke {
            width: 4.0,
            ..Default::default()
        },
    );
}

fn graph_for(record: fn(&mut DrawScopeDefault), clip: Option<Rect>) -> RenderGraph {
    let mut scope =
        DrawScopeDefault::new(cranpose_ui_graphics::Size::new(SIZE as f32, SIZE as f32));
    record(&mut scope);
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: SIZE as f32,
        height: SIZE as f32,
    };
    let primitives = scope.into_primitives();
    let children = match clip {
        Some(clip) => primitives
            .into_iter()
            .map(|primitive| {
                RenderNode::Primitive(PrimitiveEntry {
                    phase: PrimitivePhase::BeforeChildren,
                    node: PrimitiveNode::Draw(DrawPrimitiveNode {
                        primitive,
                        clip: Some(clip),
                    }),
                })
            })
            .collect(),
        None => vec![RenderNode::DrawRun(DrawRunNode::new(
            PrimitivePhase::BeforeChildren,
            primitives,
        ))],
    };
    RenderGraph::new(LayerNode {
        local_bounds: bounds,
        children,
        ..LayerNode::default()
    })
}

fn render_arm(renderer: &mut support::LockedRenderer, graph: &RenderGraph) -> Vec<u8> {
    let mut passes = Vec::new();
    for _ in 0..3 {
        renderer.scene_mut().graph = Some(graph.clone());
        let captured = renderer
            .capture_frame(SIZE, SIZE)
            .unwrap_or_else(|err| panic!("capture failed: {err:?}"));
        assert_eq!((captured.width, captured.height), (SIZE, SIZE));
        passes.push(captured.pixels);
    }
    assert_eq!(
        passes[1], passes[2],
        "same-graph control passes must be byte-stable before the cross-arm compare"
    );
    passes.pop().unwrap()
}

const TOGGLE: &str = "CRANPOSE_SHAPE_VARIANTS";
const MAX_DIVERGING_BYTES: usize = 16;
const MAX_DIVERGING_LEVEL: u8 = 2;

fn assert_variants_match_general(name: &str, graph: RenderGraph) {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping shape variant parity ({name}): headless WGPU init failed: {err}");
            return;
        }
    };
    cranpose_render_wgpu::set_debug_toggle(TOGGLE, None);
    let specialized = render_arm(&mut renderer, &graph);
    cranpose_render_wgpu::set_debug_toggle(TOGGLE, Some("0"));
    let general = render_arm(&mut renderer, &graph);
    cranpose_render_wgpu::set_debug_toggle(TOGGLE, None);

    assert_eq!(specialized.len(), general.len());
    let distinct = support::distinct_colors(&specialized);
    assert!(
        distinct > 8,
        "{name}: the scene must draw something ({distinct} colors)"
    );
    let mut differing = 0usize;
    let mut worst = 0u8;
    for (a, b) in specialized.iter().zip(&general) {
        let diff = a.abs_diff(*b);
        if diff > 0 {
            differing += 1;
            worst = worst.max(diff);
        }
    }
    eprintln!("{name}: specialized-vs-general differing {differing} worst {worst}");
    assert!(
        differing <= MAX_DIVERGING_BYTES && worst <= MAX_DIVERGING_LEVEL,
        "{name}: {differing} bytes diverged (worst {worst}), over a bound of \
         {MAX_DIVERGING_BYTES} bytes and {MAX_DIVERGING_LEVEL} levels; a pipeline constant \
         may fold a branch the batch cannot take, never change what a record shades"
    );
}

#[test]
fn solid_batches_shade_as_the_general_pipeline_does() {
    assert_variants_match_general("solid", graph_for(record_solid_scene, None));
}

#[test]
fn gradient_and_stroke_batches_shade_as_the_general_pipeline_does() {
    assert_variants_match_general("mixed", graph_for(record_mixed_scene, None));
}

#[test]
fn clipped_batches_shade_as_the_general_pipeline_does() {
    let clip = Rect {
        x: 20.0,
        y: 30.0,
        width: 200.0,
        height: 190.0,
    };
    assert_variants_match_general("clipped", graph_for(record_mixed_scene, Some(clip)));
}
