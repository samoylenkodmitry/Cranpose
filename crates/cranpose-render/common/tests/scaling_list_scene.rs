#![cfg(feature = "embedded-default-font")]

use std::{cell::RefCell, rc::Rc};

use cranpose_foundation::lazy::LazyItems;
use cranpose_render_common::{
    HitTestTarget, RenderScene,
    graph::{LayerNode, PrimitiveNode, ProjectiveTransform, RenderNode},
    graph_scene::{HitGeometry, Scene},
    hit_graph::{HitGraphSink, collect_hits_from_graph},
    scene_builder::build_graph_from_applier,
};
use cranpose_ui::{
    Color, LayoutEngine, Modifier, Size, Text,
    round_scaling_list::CentreAnchor,
    widgets::{
        BoxSpec,
        wear::{
            WearColors, WearScalingLazyColumn, WearScalingLazyColumnSpec, WearTextStyle,
            rememberWearScalingListState,
        },
    },
};

const WATCH: f32 = 454.0;

const ROWS: [&str; 6] = ["Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot"];

fn colors() -> WearColors {
    WearColors {
        content: Color::WHITE,
        ..WearColors::default()
    }
}

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
                scope.items(
                    LazyItems::new(ROWS.len()).key(|index: usize| index as u64),
                    |index| {
                        Text(
                            ROWS[index].to_string(),
                            Modifier::empty().fill_max_width(),
                            WearTextStyle::BODY_LARGE.resolve(colors().content),
                        );
                    },
                );
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

const TAP_ROW: f32 = 52.0;
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
                scope.items(
                    LazyItems::new(count).key(|index: usize| index as u64),
                    move |index| {
                        let sink = Rc::clone(&sink);
                        cranpose_ui::widgets::Box(
                            Modifier::empty()
                                .fill_max_width()
                                .height(TAP_ROW)
                                .clickable(move |_| sink.borrow_mut().push(index)),
                            BoxSpec::default(),
                            || {},
                        );
                    },
                );
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
    let ids = scene
        .hits
        .iter()
        .filter(|hit| (hit.local_bounds.height - TAP_ROW).abs() < f32::EPSILON)
        .map(|hit| hit.node_id)
        .collect();
    scene.replace_graph(graph);
    (scene, ids, tapped)
}

fn row_at(scene: &Scene, x: f32, y: f32) -> Option<usize> {
    let node_ids: Vec<_> = scene
        .hits
        .iter()
        .filter(|hit| (hit.local_bounds.height - TAP_ROW).abs() < f32::EPSILON)
        .map(|hit| hit.node_id)
        .collect();
    let hits = scene.hit_test(x, y);
    hits.iter()
        .find_map(|hit| node_ids.iter().position(|id| *id == hit.node_id()))
}

#[test]
fn a_shrunken_row_is_only_tappable_where_it_is_drawn() {
    let (scene, ids, tapped) = tappable_list_scene(6);
    assert_eq!(ids.len(), 6, "every row is a hit target");

    const CENTRE_X: f32 = WATCH / 2.0;

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

    assert_eq!(
        row_at(&scene, 20.0, 210.0),
        Some(1),
        "an unscaled row keeps the whole width the list gave it"
    );

    let at = cranpose_ui_graphics::Point {
        x: CENTRE_X,
        y: 400.0,
    };
    for hit in scene.hit_test(at.x, at.y) {
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
