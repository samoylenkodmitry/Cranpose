//! Full-pipeline render tests for `SwipeToDismiss`.
//!
//! Regression coverage for two device-confirmed bugs:
//!
//! 1. **Live drag follow** — the content must translate under the finger while
//!    dragging, not stay put and snap on release. The offset is applied as a
//!    graphics-layer translation resolved on every draw pass, so pumping
//!    pointer-move events must move the applied content offset 1:1 with the
//!    accumulated drag (without any recomposition/re-measure).
//!
//! 2. **Background reveal gating** — the `with_background` content must not draw
//!    at rest (offset 0), so it cannot flash through content that fades in with
//!    alpha < 1 during an entrance animation. It must draw once displaced.

use super::*;
use crate::modifier::ModifierNodeSlices;
use crate::modifier::{Color, PointerEvent, PointerEventKind};
use cranpose_core::NodeId;
use cranpose_ui_graphics::{DrawPrimitive, Point, Size as ViewportSize, Size as PrimSize};
use std::rc::Rc;

const BIN_RED: Color = Color(1.0, 0.0, 0.0, 1.0);

fn compose_row(with_background: bool) -> TestComposition {
    run_test_composition(move || {
        let mut spec = SwipeToDismissSpec::default();
        if with_background {
            spec = spec.with_background(|| {
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

/// The single node carrying the content's graphics-layer resolver (the content
/// wrapper). Returned as an owned `Rc` so the resolver can be re-queried after
/// drags without rebuilding the tree.
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
            if let DrawPrimitive::Rect { brush, .. } = primitive {
                if format!("{brush:?}").contains("1.0, 0.0, 0.0") {
                    *count += 1;
                }
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

/// Bug 1: pumping pointer-move events moves the applied content offset 1:1 with
/// the accumulated drag, in real time, before release — with no recomposition.
#[test]
fn dragging_translates_content_with_the_finger() {
    let mut composition = compose_row(false);
    let root = composition.root().expect("root");
    measure_row(&mut composition, root);
    let tree = layout_tree(&mut composition, root);

    let handler = pointer_handler(&tree);
    let content = content_layer_slices(&tree);

    // At rest, nothing is translated.
    assert_eq!(applied_translation_x(&content), 0.0);

    handler(down(10.0));

    // Within the drag slop: no capture yet, still no translation.
    handler(move_to(14.0));
    assert_eq!(applied_translation_x(&content), 0.0);

    // Crossing the slop captures the gesture; the content follows immediately.
    handler(move_to(30.0));
    assert!(
        (applied_translation_x(&content) - 20.0).abs() < 1e-3,
        "content should track the finger (dx=20), got {}",
        applied_translation_x(&content)
    );

    // Each subsequent move tracks the accumulated drag 1:1.
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

/// Bug 2: the background produces no draw primitives at rest (offset 0) and
/// does once the row is displaced.
#[test]
fn background_only_draws_while_displaced() {
    let mut composition = compose_row(true);
    let root = composition.root().expect("root");
    measure_row(&mut composition, root);

    // At rest: the background is not composed, so it emits nothing — even
    // though the content behind/over it would (this is the entrance-flash bug).
    let tree = layout_tree(&mut composition, root);
    assert_eq!(
        count_bin_primitives(&tree),
        0,
        "background must not draw at offset 0"
    );

    // Displace the row past the slop.
    let handler = pointer_handler(&tree);
    handler(down(10.0));
    handler(move_to(80.0)); // dx = 70 -> revealed

    // Reading the reveal flag inside the subcompose subscribes the slot, so the
    // write recomposes it and the background is now composed and drawn.
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

/// Bug 3: after a dismiss settles nothing may linger — no red "move to bin"
/// strip and no empty row — regardless of when the host removes the item. The
/// background stops drawing and the row collapses to zero height.
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

    // Swipe past the dismiss threshold (0.5 * 320px width = 160px) and release.
    handler(down(10.0));
    handler(move_to(30.0)); // capture
    handler(move_to(210.0)); // dx = 200 >= threshold
    handler(up(210.0));

    // Settle the fling and run the collapse animation to completion.
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
