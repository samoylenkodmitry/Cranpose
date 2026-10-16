//! A box whose size animates, and nothing else about it, as a progress
//! bar's does, draws at its new size after a scene update, as a fresh scene
//! does.

use cranpose_core::MutableState;
use cranpose_render_common::graph::{
    LayerNode, PrimitiveNode, ProjectiveTransform, RenderGraph, RenderNode,
};
use cranpose_ui::{Box, BoxSpec, Color, Column, ColumnSpec, Modifier, Size, run_test_composition};
use cranpose_ui_graphics::{Brush, DrawPrimitive, Rect};

use crate::scene_probe::{fresh_graph, initial_graph, update_scene};

const VIEWPORT: Size = Size::new(240.0, 120.0);
const TRACK: Color = Color(0.2, 0.2, 0.3, 1.0);
const BAR: Color = Color(0.4, 0.6, 1.0, 1.0);

/// Every rectangle the layer and its descendants draw, in draw order, with
/// its bounds in the scene.
fn drawn(layer: &LayerNode, parent: ProjectiveTransform, out: &mut Vec<(Rect, DrawPrimitive)>) {
    let transform = layer.transform_to_parent.then(parent);
    let push = |out: &mut Vec<_>, primitive: DrawPrimitive| {
        if let DrawPrimitive::Rect { rect, .. } | DrawPrimitive::RoundRect { rect, .. } = &primitive
        {
            out.push((transform.bounds_for_rect(*rect), primitive));
        }
    };
    for child in &layer.children {
        match child {
            RenderNode::Layer(child) => drawn(child, transform, out),
            RenderNode::DrawRun(run) => run.primitives().for_each(|primitive| push(out, primitive)),
            RenderNode::Primitive(entry) => {
                if let PrimitiveNode::Draw(draw) = &entry.node {
                    push(out, draw.primitive.clone());
                }
            }
        }
    }
}

fn drawn_rects(graph: &RenderGraph) -> Vec<(Rect, DrawPrimitive)> {
    let mut out = Vec::new();
    drawn(&graph.root, ProjectiveTransform::identity(), &mut out);
    out
}

#[test]
fn a_box_whose_fill_and_height_change_draws_as_a_fresh_scene_does() {
    type Bar = (MutableState<f32>, MutableState<f32>);
    let states = std::rc::Rc::new(std::cell::Cell::new(None::<Bar>));
    let slot = std::rc::Rc::clone(&states);
    let mut composition = run_test_composition(move || {
        let slot = std::rc::Rc::clone(&slot);
        Column(
            Modifier::empty().padding(10.0),
            ColumnSpec::default(),
            move || {
                let progress = cranpose_core::rememberMutableStateOf(|| 0.5_f32);
                let height = cranpose_core::rememberMutableStateOf(|| 16.0_f32);
                slot.set(Some((progress, height)));
                Box(
                    Modifier::empty()
                        .fill_max_width()
                        .height(height.get())
                        .background(TRACK)
                        .rounded_corners(8.0),
                    BoxSpec::default(),
                    move || {
                        Box(
                            Modifier::empty()
                                .fill_max_width_fraction(progress.get())
                                .height(height.get())
                                .background(BAR)
                                .rounded_corners(8.0),
                            BoxSpec::default(),
                            || {},
                        );
                    },
                );
            },
        );
    });
    let root = composition.root().expect("root");
    let mut graph = initial_graph(&mut composition, root, VIEWPORT);
    let (progress, height) = states.get().expect("bar states");
    // Back to an earlier size too: the node must not keep the one before.
    for (fraction, tall) in [(0.75, 16.0), (0.25, 16.0), (0.25, 24.0), (0.5, 16.0)] {
        progress.set(fraction);
        height.set(tall);
        update_scene(&mut composition, root, VIEWPORT, &mut graph);
        let rects = drawn_rects(&graph);
        let bar = rects
            .iter()
            .find(|(_, primitive)| match primitive {
                DrawPrimitive::Rect { brush, .. } | DrawPrimitive::RoundRect { brush, .. } => {
                    *brush == Brush::Solid(BAR)
                }
                _ => false,
            })
            .map(|(bounds, _)| *bounds)
            .expect("the bar draws");
        assert_eq!(
            (bar.width, bar.height),
            ((VIEWPORT.width - 20.0) * fraction, tall),
            "the bar at {fraction} by {tall}"
        );
        assert_eq!(
            rects,
            drawn_rects(&fresh_graph(&mut composition, root)),
            "the updated scene draws what a fresh one does"
        );
    }
}
