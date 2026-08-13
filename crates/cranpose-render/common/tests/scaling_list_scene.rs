//! The Wear scaling list, checked at the scene the renderer is handed.
//!
//! Every other test of these widgets stops at the `LayoutTree`, and a
//! `LayoutTree` is built from the `Placement`s a measure policy returns. The
//! scene the app actually draws is not: `build_graph_from_applier` walks the
//! RETAINED node state and drops any node whose `is_placed` is false. The two
//! disagreed once — the whole widget set laid out correctly in every assertion
//! and reached the device as an empty screen with a scroll indicator on it —
//! so what is asserted here is the scene, from a composition, through the same
//! applier walk the runtime uses.

#![cfg(feature = "embedded-default-font")]

use cranpose_render_common::graph::{LayerNode, PrimitiveNode, RenderNode};
use cranpose_render_common::scene_builder::build_graph_from_applier;
use cranpose_ui::round_scaling_list::CentreAnchor;
use cranpose_ui::widgets::wear::{
    rememberWearScalingListState, WearColors, WearScalingLazyColumn, WearScalingLazyColumnSpec,
    WearTextStyle,
};
use cranpose_ui::{Color, LayoutEngine, Modifier, Size, Text};

/// The watch these widgets are dimensioned for, in layout points.
const WATCH: f32 = 454.0;

const ROWS: [&str; 6] = ["Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot"];

fn colors() -> WearColors {
    WearColors {
        content: Color::WHITE,
        ..WearColors::default()
    }
}

/// Every text primitive in the scene, in tree order.
fn painted_text(layer: &LayerNode, out: &mut Vec<String>) {
    for child in &layer.children {
        match child {
            RenderNode::Primitive(primitive) => {
                if let PrimitiveNode::Text(text) = &primitive.node {
                    out.push(text.text.text.clone());
                }
            }
            RenderNode::Layer(child) => painted_text(child, out),
            RenderNode::DrawRun(_) => {}
        }
    }
}

/// Every layer the scaling ramp has hold of: one per list item, as
/// `(alpha, scale, pivot_y)`.
fn ramp_layers(layer: &LayerNode, out: &mut Vec<(f32, f32, f32)>) {
    for child in &layer.children {
        let RenderNode::Layer(child) = child else {
            continue;
        };
        let graphics_layer = &child.graphics_layer;
        if graphics_layer.alpha < 1.0 || graphics_layer.scale != 1.0 {
            out.push((
                graphics_layer.alpha,
                graphics_layer.scale,
                graphics_layer.transform_origin.pivot_fraction_y,
            ));
        }
        ramp_layers(child, out);
    }
}

fn scaling_list_scene() -> LayerNode {
    let mut composition = cranpose_ui::run_test_composition(|| {
        let state = rememberWearScalingListState(CentreAnchor::default());
        WearScalingLazyColumn(
            Modifier::empty().fill_max_size(),
            state,
            WearScalingLazyColumnSpec::default().content_padding(18.0, 34.0),
            |scope| {
                scope.items(ROWS.len(), |index| {
                    Text(
                        ROWS[index].to_string(),
                        Modifier::empty().fill_max_width(),
                        WearTextStyle::BODY_LARGE.resolve(colors().content),
                    );
                });
            },
        );
    });

    let root = composition.root().expect("list root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(
            root,
            Size {
                width: WATCH,
                height: WATCH,
            },
        )
        .expect("layout");
    let graph = build_graph_from_applier(&mut applier, root, 1.0).expect("render graph");
    applier.clear_runtime_handle();
    graph.root
}

#[test]
fn every_row_of_a_scaling_list_reaches_the_scene_as_text() {
    let root = scaling_list_scene();
    let mut painted = Vec::new();
    painted_text(&root, &mut painted);
    assert_eq!(
        painted,
        ROWS.iter().map(|row| row.to_string()).collect::<Vec<_>>(),
        "the scene the renderer is handed must carry every row's glyphs"
    );
}

#[test]
fn a_scaling_list_hands_the_scene_the_ramp_it_measured() {
    // The rows away from the centre line are the ones the ramp shrinks and
    // fades, and the layer that carries the scale is the ITEM's, so the values
    // have to survive into the scene rather than stopping at the layout tree.
    let root = scaling_list_scene();
    let mut ramp = Vec::new();
    ramp_layers(&root, &mut ramp);
    assert!(
        !ramp.is_empty(),
        "a six-row list on a 454pt watch has rows off the centre line for the ramp to take"
    );
    for (alpha, scale, pivot_y) in ramp {
        assert!(
            alpha > 0.0 && alpha <= 1.0,
            "a faded row still draws: alpha {alpha}"
        );
        assert!(
            scale > 0.0 && scale <= 1.0,
            "a shrunk row still has area: scale {scale}"
        );
        assert_eq!(
            pivot_y, 0.0,
            "Wear scales a row about its top edge, as AOSP does"
        );
    }
}
