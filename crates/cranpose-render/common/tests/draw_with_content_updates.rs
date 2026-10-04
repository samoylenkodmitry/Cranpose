use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::NodeId;
use cranpose_render_common::{
    SceneUpdates,
    graph::RenderGraph,
    scene_builder::{
        clear_command_recordings_for_tests, rebuild_graph_from_applier,
        update_graph_from_applier_report,
    },
};
use cranpose_ui::{
    Box, BoxSpec, Brush, Canvas, Color, GraphicsLayer, Modifier, RoundedCornerShape, Size,
    TestComposition, Text, TextStyle, run_test_composition,
};
use cranpose_ui_graphics::DrawScope;

const VIEWPORT: Size = Size::new(120.0, 80.0);

fn with_content_modifier(
    outer_wrapper: bool,
    draw_calls: Rc<Cell<usize>>,
    before: Rc<Cell<Color>>,
    after: Rc<Cell<Color>>,
) -> Modifier {
    let draw = move |scope: &mut dyn DrawScope| {
        draw_calls.set(draw_calls.get() + 1);
        scope.draw_rect(Brush::solid(before.get()));
        scope.draw_content();
        scope.draw_rect(Brush::solid(after.get()));
    };
    let layer = || GraphicsLayer {
        rotation_z: 17.0,
        ..GraphicsLayer::default()
    };
    let commands = Modifier::empty()
        .draw_behind(|_| {})
        .border(1.0, Color::BLACK, RoundedCornerShape::uniform(2.0))
        .draw_with_content(draw)
        .draw_with_content(|scope| scope.draw_content());
    if outer_wrapper {
        commands.graphics_layer(layer)
    } else {
        Modifier::empty().graphics_layer(layer).then(commands)
    }
}

fn initial_graph(composition: &mut TestComposition, root: NodeId) -> RenderGraph {
    crate::scene_probe::initial_graph(composition, root, VIEWPORT)
}

fn assert_callback_once_per_update(outer_wrapper: bool) {
    clear_command_recordings_for_tests();
    let calls = Rc::new(Cell::new(0));
    let before = Rc::new(Cell::new(Color::BLUE));
    let after = Rc::new(Cell::new(Color::WHITE));
    let parent_slot = Rc::new(Cell::new(None::<NodeId>));
    let draw_calls = Rc::clone(&calls);
    let draw_before = Rc::clone(&before);
    let draw_after = Rc::clone(&after);
    let parent_id = Rc::clone(&parent_slot);
    let mut composition = run_test_composition(move || {
        let parent = Box(
            with_content_modifier(
                outer_wrapper,
                Rc::clone(&draw_calls),
                Rc::clone(&draw_before),
                Rc::clone(&draw_after),
            )
            .size(VIEWPORT),
            BoxSpec::default(),
            || {
                Canvas(Modifier::empty().size(Size::new(40.0, 24.0)), |scope| {
                    scope.draw_rect(Brush::solid(Color::RED));
                });
                Text("stable child", Modifier::empty(), TextStyle::default());
            },
        );
        parent_id.set(Some(parent));
    });

    let root = composition.root().expect("root");
    let parent = parent_slot.get().expect("parent Box node");
    let mut graph = initial_graph(&mut composition, root);
    assert_eq!(calls.get(), 1, "initial scene records with-content once");
    let mut colors = Vec::new();
    crate::scene_probe::painted_solid_rect_colors(&graph.root, &mut colors);
    assert_eq!(colors, [Color::BLUE, Color::RED, Color::WHITE]);

    before.set(Color(0.0, 1.0, 1.0, 1.0));
    after.set(Color::GREEN);
    let runtime = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(runtime);
    let report = update_graph_from_applier_report(
        &applier,
        &mut graph,
        SceneUpdates::content(std::slice::from_ref(&parent)),
        1.0,
    );
    assert!(report.applied(), "content update applies to retained graph");
    assert_eq!(calls.get(), 2, "one content update records once");
    colors.clear();
    crate::scene_probe::painted_solid_rect_colors(&graph.root, &mut colors);
    assert_eq!(
        colors,
        [Color(0.0, 1.0, 1.0, 1.0), Color::RED, Color::GREEN]
    );

    before.set(Color(1.0, 1.0, 0.0, 1.0));
    after.set(Color(1.0, 0.0, 1.0, 1.0));
    graph = rebuild_graph_from_applier(&applier, root, 1.0, Some(graph))
        .expect("full scene update rebuilds graph");
    assert_eq!(calls.get(), 3, "one full rebuild records once");
    colors.clear();
    crate::scene_probe::painted_solid_rect_colors(&graph.root, &mut colors);
    assert_eq!(
        colors,
        [
            Color(1.0, 1.0, 0.0, 1.0),
            Color::RED,
            Color(1.0, 0.0, 1.0, 1.0)
        ]
    );
    applier.clear_runtime_handle();
}

#[test]
fn outer_with_content_draws_once_per_scene_update_and_keeps_child_order() {
    assert_callback_once_per_update(true);
}

#[test]
fn ordinary_with_content_draws_once_per_scene_update_and_keeps_child_order() {
    assert_callback_once_per_update(false);
}

#[test]
fn nested_scene_build_does_not_replace_parent_with_content_recording() {
    clear_command_recordings_for_tests();
    let nested_calls = Rc::new(Cell::new(0));
    let nested_before = Rc::new(Cell::new(Color(1.0, 1.0, 0.0, 1.0)));
    let nested_after = Rc::new(Cell::new(Color(1.0, 0.0, 1.0, 1.0)));
    let nested_parent_slot = Rc::new(Cell::new(None::<NodeId>));
    let build_nested = Rc::new(Cell::new(false));
    let nested_calls_for_comp = Rc::clone(&nested_calls);
    let nested_before_for_comp = Rc::clone(&nested_before);
    let nested_after_for_comp = Rc::clone(&nested_after);
    let nested_parent_for_comp = Rc::clone(&nested_parent_slot);
    let nested_composition = run_test_composition(move || {
        let calls = Rc::clone(&nested_calls_for_comp);
        let before = Rc::clone(&nested_before_for_comp);
        let after = Rc::clone(&nested_after_for_comp);
        let parent = Box(
            with_content_modifier(false, calls, before, after).size(VIEWPORT),
            BoxSpec::default(),
            || {
                Canvas(Modifier::empty().size(Size::new(40.0, 24.0)), |scope| {
                    scope.draw_rect(Brush::solid(Color(1.0, 0.5, 0.0, 1.0)));
                });
            },
        );
        nested_parent_for_comp.set(Some(parent));
    });

    let parent_calls = Rc::new(Cell::new(0));
    let parent_before = Rc::new(Cell::new(Color::BLUE));
    let parent_after = Rc::new(Cell::new(Color::WHITE));
    let parent_slot = Rc::new(Cell::new(None::<NodeId>));
    let nested_for_parent = Rc::new(RefCell::new(nested_composition));
    let build_nested_for_parent = Rc::clone(&build_nested);
    let parent_calls_for_comp = Rc::clone(&parent_calls);
    let parent_before_for_comp = Rc::clone(&parent_before);
    let parent_after_for_comp = Rc::clone(&parent_after);
    let parent_id_for_comp = Rc::clone(&parent_slot);
    let mut composition = run_test_composition(move || {
        let calls = Rc::clone(&parent_calls_for_comp);
        let before = Rc::clone(&parent_before_for_comp);
        let after = Rc::clone(&parent_after_for_comp);
        let nested = Rc::clone(&nested_for_parent);
        let should_build_nested = Rc::clone(&build_nested_for_parent);
        let parent = Box(
            with_content_modifier(false, calls, before, after).size(VIEWPORT),
            BoxSpec::default(),
            move || {
                let nested = Rc::clone(&nested);
                let should_build_nested = Rc::clone(&should_build_nested);
                Canvas(
                    Modifier::empty().size(Size::new(40.0, 24.0)),
                    move |scope| {
                        if should_build_nested.replace(false) {
                            let mut nested = nested.borrow_mut();
                            let root = nested.root().expect("nested composition root");
                            let _ = initial_graph(&mut nested, root);
                        }
                        scope.draw_rect(Brush::solid(Color::RED));
                    },
                );
            },
        );
        parent_id_for_comp.set(Some(parent));
    });

    let root = composition.root().expect("root");
    let parent = parent_slot.get().expect("parent Box node");
    let nested_parent = nested_parent_slot.get().expect("nested Box node");
    assert_eq!(
        parent, nested_parent,
        "independent appliers reuse local node IDs"
    );

    build_nested.set(true);
    let mut graph = initial_graph(&mut composition, root);
    assert_eq!(parent_calls.get(), 1, "parent command runs once");
    assert_eq!(nested_calls.get(), 1, "nested scene was built once");
    let mut colors = Vec::new();
    crate::scene_probe::painted_solid_rect_colors(&graph.root, &mut colors);
    assert_eq!(colors, [Color::BLUE, Color::RED, Color::WHITE]);

    parent_before.set(Color(0.0, 1.0, 1.0, 1.0));
    parent_after.set(Color::GREEN);
    nested_before.set(Color(0.5, 0.0, 0.5, 1.0));
    nested_after.set(Color(1.0, 0.0, 1.0, 1.0));
    build_nested.set(true);
    let runtime = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(runtime);
    graph = rebuild_graph_from_applier(&applier, root, 1.0, Some(graph))
        .expect("full scene rebuild after nested build");
    assert_eq!(parent_calls.get(), 2, "parent command runs once on rebuild");
    assert_eq!(nested_calls.get(), 2, "nested scene rebuilds once");
    colors.clear();
    crate::scene_probe::painted_solid_rect_colors(&graph.root, &mut colors);
    assert_eq!(
        colors,
        [Color(0.0, 1.0, 1.0, 1.0), Color::RED, Color::GREEN]
    );
    applier.clear_runtime_handle();
}
