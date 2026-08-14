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

use cranpose_render_common::graph::{LayerNode, PrimitiveNode, ProjectiveTransform, RenderNode};
use cranpose_render_common::graph_scene::{HitGeometry, Scene};
use cranpose_render_common::hit_graph::{collect_hits_from_graph, HitGraphSink};
use cranpose_render_common::scene_builder::build_graph_from_applier;
use cranpose_render_common::{HitTestTarget, RenderScene};
use cranpose_ui::round_scaling_list::CentreAnchor;
use cranpose_ui::widgets::wear::{
    rememberWearScalingListState, WearColors, WearScalingLazyColumn, WearScalingLazyColumnSpec,
    WearTextStyle,
};
use cranpose_ui::widgets::BoxSpec;
use cranpose_ui::{Color, LayoutEngine, Modifier, Size, Text};
use std::cell::RefCell;
use std::rc::Rc;

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

// ── A shrunken row's touch target ───────────────────────────────────────────

/// The height of every row in the tappable list below, in layout points.
///
/// Fixed rather than text-measured so the geometry asserted here is the
/// scaling ramp's and not the text measurer's.
const TAP_ROW: f32 = 52.0;
/// A scene of `count` full-width tappable rows, and the hits collected from it.
///
/// This is the hit path the app runs, not a model of it: `AppShell` calls
/// `Scene::hit_test` on a scene whose regions came from exactly this
/// `collect_hits_from_graph` walk (`pipeline::render_from_applier`), and the
/// graph came from exactly this `build_graph_from_applier`.
fn tappable_list_scene(count: usize) -> (Scene, Vec<usize>, Rc<RefCell<Vec<usize>>>) {
    let tapped: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&tapped);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let sink = Rc::clone(&sink);
        let state = rememberWearScalingListState(CentreAnchor::default());
        WearScalingLazyColumn(
            Modifier::empty().fill_max_size(),
            state,
            WearScalingLazyColumnSpec::default().content_padding(18.0, 34.0),
            move |scope| {
                scope.items(count, move |index| {
                    let sink = Rc::clone(&sink);
                    cranpose_ui::widgets::Box(
                        Modifier::empty()
                            .fill_max_width()
                            .height(TAP_ROW)
                            .clickable(move |_| sink.borrow_mut().push(index)),
                        BoxSpec::default(),
                        || {},
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

    struct SceneSink<'a> {
        scene: &'a mut Scene,
    }
    impl HitGraphSink for SceneSink<'_> {
        fn push_hit(
            &mut self,
            node_id: cranpose_core::NodeId,
            capture_path: &[cranpose_core::NodeId],
            geometry: HitGeometry,
            shape: Option<cranpose_ui_graphics::RoundedCornerShape>,
            click_actions: &[Rc<dyn Fn(cranpose_ui_graphics::Point)>],
            pointer_inputs: &[Rc<dyn Fn(cranpose_foundation::PointerEvent)>],
        ) {
            self.scene.push_hit(
                node_id,
                capture_path.to_vec(),
                geometry,
                shape,
                click_actions
                    .iter()
                    .cloned()
                    .map(cranpose_render_common::graph_scene::ClickAction::WithPoint)
                    .collect(),
                pointer_inputs.to_vec(),
            );
        }
    }

    let mut scene = Scene::default();
    collect_hits_from_graph(
        &graph.root,
        ProjectiveTransform::identity(),
        &mut SceneSink { scene: &mut scene },
        None,
    );
    // The rows are pushed in tree order, so a hit's index in `hits` is the
    // item's index in the list. Keep the ids so a hit can be named.
    let ids = (0..scene.hits.len()).collect();
    scene.replace_graph(graph);
    (scene, ids, tapped)
}

/// Which row a point lands on, or `None` for a point that hits nothing.
fn row_at(scene: &Scene, x: f32, y: f32) -> Option<usize> {
    let node_ids: Vec<_> = scene.hits.iter().map(|hit| hit.node_id).collect();
    let hits = scene.hit_test(x, y);
    hits.first()
        .and_then(|hit| node_ids.iter().position(|id| *id == hit.node_id()))
}

#[test]
fn a_shrunken_row_is_only_tappable_where_it_is_drawn() {
    // The claim this test exists to settle was that a scaled row's touch target
    // stays its UNSCALED box — that the layer shrinks what is drawn and hit
    // testing never hears about it, so a tap in the widening gap between two
    // shrunken rows lands on one of them anyway.
    //
    // It does not. Hit testing in this engine is done on the render graph, and
    // a layer's `transform_to_parent` is built by `layer_transform_to_parent`
    // from the SAME resolved `GraphicsLayer` the renderer draws with — scale
    // about `transform_origin` included. `HitRegion::contains` then inverts
    // that transform and tests the point against the node's local bounds, so
    // the target is the drawn quad and nothing else.
    //
    // The numbers below are the ramp's own, for six 52pt rows on a 454pt watch
    // with 18/34 content padding, anchored on item 1, and they are the drawn
    // boxes rather than the `LazyColumn`'s slots — the list stacks each row
    // against the SCALED size of the one between it and the centre, which is
    // what `ScalingLazyListState.layoutInfo` does. Consecutive drawn boxes are
    // therefore always the 4pt gap apart; what a shrinking row gives up is
    // taken out of its own span, not added to the gap.
    //
    //   row 3   drawn 312.50..363.85
    //   row 4   drawn 367.50..412.60
    //   row 5   drawn 416.50..453.95
    let (scene, ids, tapped) = tappable_list_scene(6);
    assert_eq!(ids.len(), 6, "every row is a hit target");

    // The centre column, so the horizontal shrink is not what is being tested
    // yet.
    const CENTRE_X: f32 = WATCH / 2.0;

    // Row 4 is drawn 367.5..412.60. Its unscaled box would run to 419.5.
    assert_eq!(
        row_at(&scene, CENTRE_X, 400.0),
        Some(4),
        "a tap inside a shrunken row still reaches it"
    );
    assert_eq!(
        row_at(&scene, CENTRE_X, 415.0),
        None,
        "y=415 is inside row 4's UNSCALED box (367.5..419.5) and below the row \
         as drawn (367.5..412.60); an unscaled hit rectangle would swallow it"
    );
    assert_eq!(
        row_at(&scene, CENTRE_X, 430.0),
        Some(5),
        "the next row starts at 416.5 and is reachable from its own top edge"
    );

    // The same shrink applies across. Row 5 is drawn x = 71.45..382.55; its
    // unscaled box is the list's full content width, x = 18..436.
    assert_eq!(
        row_at(&scene, 60.0, 430.0),
        None,
        "a row narrows as well as shortens, and its touch target narrows with it"
    );
    assert_eq!(
        row_at(&scene, 100.0, 430.0),
        Some(5),
        "inside the drawn width it is still the same row"
    );

    // Rows on the centre line are at scale 1 and unchanged: row 1 is the
    // anchored item and spans the full content width.
    assert_eq!(
        row_at(&scene, 20.0, 210.0),
        Some(1),
        "an unscaled row keeps the whole width the list gave it"
    );

    // And the hit actually fires the row's click handler, so this is the
    // dispatchable target and not merely a rectangle that agrees.
    let at = cranpose_ui_graphics::Point {
        x: CENTRE_X,
        y: 400.0,
    };
    for hit in scene.hit_test(at.x, at.y) {
        // `clickable` confirms on release, so a click is a Down then an Up.
        for kind in [
            cranpose_foundation::PointerEventKind::Down,
            cranpose_foundation::PointerEventKind::Up,
        ] {
            let mut event = cranpose_foundation::PointerEvent::new(kind, at, at);
            event.buttons = cranpose_foundation::PointerButtons::new()
                .with(cranpose_foundation::PointerButton::Primary);
            hit.dispatch(event);
        }
    }
    assert_eq!(
        *tapped.borrow(),
        vec![4],
        "the tap ran row 4's onClick and nobody else's"
    );
}
