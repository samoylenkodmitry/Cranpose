mod support;

use cranpose_render_common::graph::{
    DrawPrimitiveNode, DrawRunNode, LayerNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase,
    RenderGraph, RenderNode,
};
use cranpose_ui_graphics::{DrawScope, DrawScopeDefault, Rect};
use support::{SIZE, record_mixed_scene, record_solid_scene};

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
    let specialized = support::settled_capture(&mut renderer, &graph);
    cranpose_render_wgpu::set_debug_toggle(TOGGLE, Some("0"));
    let general = support::settled_capture(&mut renderer, &graph);
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
