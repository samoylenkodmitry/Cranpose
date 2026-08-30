use std::rc::Rc;

use cranpose_core::NodeId;
use cranpose_ui_graphics::{DrawPrimitive, Point, Size as ViewportSize, Size as PrimSize};

use super::*;
use crate::modifier::{Color, ModifierNodeSlices, PointerEvent, PointerEventKind};

const BIN_RED: Color = Color(1.0, 0.0, 0.0, 1.0);

fn compose_row(with_background: bool) -> TestComposition {
    run_test_composition(move || {
        let mut spec = SwipeToDismissSpec::default();
        if with_background {
            spec = spec.with_background(|_side| {
                Box(
                    Modifier::empty().fill_max_size().background(BIN_RED),
                    BoxSpec::new(),
                    || {},
                );
            });
        }
        SwipeToDismiss(
            Modifier::empty().fill_max_width().height(48.0),
            spec,
            || {},
            || {
                Text(
                    "CONTENT".to_string(),
                    Modifier::empty(),
                    TextStyle::default(),
                );
            },
        );
    })
}

fn compose_lazy_row_with_background() -> TestComposition {
    use cranpose_foundation::lazy::{LazyListScope, rememberLazyListState};
    run_test_composition(move || {
        let list_state = rememberLazyListState();
        LazyColumn(
            Modifier::empty().fill_max_size(),
            list_state,
            LazyColumnSpec::default(),
            |scope| {
                scope.items(1, |_index| {
                    let spec = SwipeToDismissSpec::default().with_background(|_side| {
                        Box(
                            Modifier::empty().fill_max_size().background(BIN_RED),
                            BoxSpec::new(),
                            || {},
                        );
                    });
                    SwipeToDismiss(
                        Modifier::empty().fill_max_width().height(48.0),
                        spec,
                        || {},
                        || {
                            Text(
                                "CONTENT".to_string(),
                                Modifier::empty(),
                                TextStyle::default(),
                            );
                        },
                    );
                });
            },
        );
    })
}

fn measure_row(composition: &mut TestComposition, root: NodeId) {
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    crate::measure_layout(
        &mut applier,
        root,
        ViewportSize {
            width: 320.0,
            height: 480.0,
        },
    )
    .expect("layout measurement");
    applier.clear_runtime_handle();
}

fn layout_tree(composition: &mut TestComposition, root: NodeId) -> crate::LayoutTree {
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let tree = crate::build_layout_tree_from_applier(&mut applier, root)
        .expect("layout tree build")
        .expect("layout tree present");
    applier.clear_runtime_handle();
    tree
}

fn pointer_handler(tree: &crate::LayoutTree) -> Rc<dyn Fn(PointerEvent)> {
    fn walk(node: &crate::LayoutBox) -> Option<Rc<dyn Fn(PointerEvent)>> {
        if let Some(handler) = node.node_data.modifier_slices().pointer_inputs().first() {
            return Some(Rc::clone(handler));
        }
        node.children.iter().find_map(walk)
    }
    walk(tree.root()).expect("swipe pointer handler")
}

fn content_layer_slices(tree: &crate::LayoutTree) -> Rc<ModifierNodeSlices> {
    fn walk(node: &crate::LayoutBox) -> Option<Rc<ModifierNodeSlices>> {
        if node.node_data.modifier_slices().graphics_layer().is_some() {
            return Some(Rc::clone(&node.node_data.modifier_slices));
        }
        node.children.iter().find_map(walk)
    }
    walk(tree.root()).expect("content graphics-layer node")
}

fn applied_translation_x(slices: &ModifierNodeSlices) -> f32 {
    slices
        .graphics_layer()
        .expect("content graphics layer")
        .translation_x
}

fn count_bin_primitives(tree: &crate::LayoutTree) -> usize {
    fn walk(node: &crate::LayoutBox, count: &mut usize) {
        let size = PrimSize {
            width: node.rect.width,
            height: node.rect.height,
        };
        for primitive in
            crate::execute_draw_commands(node.node_data.modifier_slices().draw_commands(), size)
        {
            if let DrawPrimitive::Rect { brush, .. } = primitive
                && format!("{brush:?}").contains("1.0, 0.0, 0.0")
            {
                *count += 1;
            }
        }
        for child in &node.children {
            walk(child, count);
        }
    }
    let mut count = 0;
    walk(tree.root(), &mut count);
    count
}

fn down(x: f32) -> PointerEvent {
    let p = Point { x, y: 24.0 };
    PointerEvent::new(PointerEventKind::Down, p, p)
}

fn move_to(x: f32) -> PointerEvent {
    let p = Point { x, y: 24.0 };
    PointerEvent::new(PointerEventKind::Move, p, p)
}

fn up(x: f32) -> PointerEvent {
    let p = Point { x, y: 24.0 };
    PointerEvent::new(PointerEventKind::Up, p, p)
}

fn root_height(composition: &mut TestComposition, root: NodeId) -> f32 {
    layout_tree(composition, root).root().rect.height
}

#[test]
fn dragging_translates_content_with_the_finger() {
    let mut composition = compose_row(false);
    let root = composition.root().expect("root");
    measure_row(&mut composition, root);
    let tree = layout_tree(&mut composition, root);

    let handler = pointer_handler(&tree);
    let content = content_layer_slices(&tree);

    assert_eq!(applied_translation_x(&content), 0.0);

    handler(down(10.0));

    handler(move_to(14.0));
    assert_eq!(applied_translation_x(&content), 0.0);

    handler(move_to(30.0));
    assert!(
        (applied_translation_x(&content) - 20.0).abs() < 1e-3,
        "content should track the finger (dx=20), got {}",
        applied_translation_x(&content)
    );

    handler(move_to(130.0));
    assert!(
        (applied_translation_x(&content) - 120.0).abs() < 1e-3,
        "content should track the finger (dx=120), got {}",
        applied_translation_x(&content)
    );

    handler(move_to(90.0));
    assert!(
        (applied_translation_x(&content) - 80.0).abs() < 1e-3,
        "content should track the finger back (dx=80), got {}",
        applied_translation_x(&content)
    );
}

#[test]
fn background_only_draws_while_displaced() {
    let mut composition = compose_row(true);
    let root = composition.root().expect("root");
    measure_row(&mut composition, root);

    let tree = layout_tree(&mut composition, root);
    assert_eq!(
        count_bin_primitives(&tree),
        0,
        "background must not draw at offset 0"
    );

    let handler = pointer_handler(&tree);
    handler(down(10.0));
    handler(move_to(80.0));

    composition
        .process_invalid_scopes()
        .expect("recompose after reveal");
    measure_row(&mut composition, root);
    let displaced = layout_tree(&mut composition, root);
    assert!(
        count_bin_primitives(&displaced) > 0,
        "background must draw while the row is displaced"
    );
}

#[test]
fn dismissed_row_hides_background_and_collapses_to_zero_height() {
    let mut composition = compose_row(true);
    let root = composition.root().expect("root");
    measure_row(&mut composition, root);
    assert!(
        (root_height(&mut composition, root) - 48.0).abs() < 1e-3,
        "row starts at its natural 48px height"
    );

    let tree = layout_tree(&mut composition, root);
    let handler = pointer_handler(&tree);

    handler(down(10.0));
    handler(move_to(30.0));
    handler(move_to(210.0));
    handler(up(210.0));

    let handle = composition.runtime_handle();
    let mut frame_time = 0u64;
    for _ in 0..600 {
        frame_time += 16_666_667;
        composition.with_app_context(|| handle.drain_frame_callbacks(frame_time));
    }
    composition
        .process_invalid_scopes()
        .expect("recompose after the dismiss settles");
    measure_row(&mut composition, root);

    let settled = layout_tree(&mut composition, root);
    assert_eq!(
        count_bin_primitives(&settled),
        0,
        "no red background strip may linger after a dismiss"
    );
    assert!(
        settled.root().rect.height <= 0.5,
        "the dismissed row must collapse to zero height, got {}",
        settled.root().rect.height
    );
}

#[test]
fn dismissed_row_inside_lazy_column_leaves_no_lingering_strip() {
    let mut composition = compose_lazy_row_with_background();
    let root = composition.root().expect("root");
    measure_row(&mut composition, root);

    let tree = layout_tree(&mut composition, root);
    let handler = deepest_pointer_handler(&tree);

    handler(down(10.0));
    handler(move_to(30.0));
    handler(move_to(210.0));

    composition
        .process_invalid_scopes()
        .expect("recompose after capture");
    measure_row(&mut composition, root);
    assert!(
        count_bin_primitives(&layout_tree(&mut composition, root)) > 0,
        "the swipe must reveal the bin background mid-drag"
    );

    handler(up(210.0));

    let handle = composition.runtime_handle();
    let mut frame_time = 0u64;
    for _ in 0..120 {
        for _ in 0..8 {
            frame_time += 16_666_667;
            composition.with_app_context(|| handle.drain_frame_callbacks(frame_time));
        }
        composition
            .process_invalid_scopes()
            .expect("recompose during collapse");
        measure_row(&mut composition, root);
    }

    let settled = layout_tree(&mut composition, root);
    assert_eq!(
        count_bin_primitives(&settled),
        0,
        "no red background strip may linger after a dismiss inside a LazyColumn"
    );
    let item_height = outer_item_height(&settled).expect("collapsing item present in tree");
    assert!(
        item_height <= 0.5,
        "the dismissed row inside a LazyColumn must collapse to zero height, got {item_height}"
    );
}

type DepthTaggedHandler = (usize, Rc<dyn Fn(PointerEvent)>);

fn deepest_pointer_handler(tree: &crate::LayoutTree) -> Rc<dyn Fn(PointerEvent)> {
    fn walk(node: &crate::LayoutBox, best: &mut Option<DepthTaggedHandler>, depth: usize) {
        if let Some(handler) = node.node_data.modifier_slices().pointer_inputs().first()
            && best.as_ref().is_none_or(|(d, _)| depth >= *d)
        {
            *best = Some((depth, Rc::clone(handler)));
        }
        for child in &node.children {
            walk(child, best, depth + 1);
        }
    }
    let mut best = None;
    walk(tree.root(), &mut best, 0);
    best.expect("swipe pointer handler").1
}

fn outer_item_height(tree: &crate::LayoutTree) -> Option<f32> {
    fn walk(node: &crate::LayoutBox) -> Option<f32> {
        for child in &node.children {
            if !child
                .node_data
                .modifier_slices()
                .pointer_inputs()
                .is_empty()
                && (node.rect.height - 480.0).abs() > 1.0
            {
                return Some(node.rect.height);
            }
            if let Some(h) = walk(child) {
                return Some(h);
            }
        }
        None
    }
    walk(tree.root())
}
