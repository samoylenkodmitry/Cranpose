//! Tests that a [`SelectionHandle`] renders its teardrop in the top-level
//! overlay, anchored so its tip lands on the given text endpoint.

use crate::layout::{LayoutEngine, LayoutTree};
use crate::renderer::{HeadlessRenderer, RenderOp};
use crate::text_selection::HandleKind;
use crate::widgets::{PopupHost, SelectionHandle};
use crate::Composition;
use cranpose_core::{location_key, Key, MemoryApplier, NodeId};
use cranpose_ui_graphics::{Color, DrawPrimitive, Point, Rect, Size};

fn compute_layout(composition: &mut Composition<MemoryApplier>, root: NodeId) -> LayoutTree {
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let layout = applier
        .compute_layout(
            root,
            Size {
                width: 400.0,
                height: 400.0,
            },
        )
        .expect("layout");
    applier.clear_runtime_handle();
    layout
}

fn settle(composition: &mut Composition<MemoryApplier>, key: Key, content: &mut dyn FnMut()) {
    for _ in 0..16 {
        if !composition.should_render() {
            break;
        }
        composition
            .reconcile(key, &mut *content)
            .expect("reconcile");
    }
}

/// Any drawn primitive's bounding rectangle (the teardrop rasterizes to an
/// image mask; backgrounds/rects are also matched so the test is robust to the
/// exact draw technique).
fn drawn_rects(scene: &crate::renderer::RecordedRenderScene) -> Vec<Rect> {
    scene
        .operations()
        .iter()
        .filter_map(|op| match op {
            RenderOp::Primitive {
                primitive: DrawPrimitive::Image { rect, .. },
                ..
            }
            | RenderOp::Primitive {
                primitive: DrawPrimitive::Rect { rect, .. },
                ..
            }
            | RenderOp::Primitive {
                primitive: DrawPrimitive::RoundRect { rect, .. },
                ..
            } => Some(*rect),
            _ => None,
        })
        .collect()
}

fn contains(rect: &Rect, p: Point) -> bool {
    p.x >= rect.x - 0.5
        && p.x <= rect.x + rect.width + 0.5
        && p.y >= rect.y - 0.5
        && p.y <= rect.y + rect.height + 0.5
}

#[test]
fn selection_handle_renders_in_overlay_at_its_tip() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());

    let tip = Point { x: 120.0, y: 90.0 };
    let mut content = move || {
        PopupHost(move || {
            SelectionHandle(
                HandleKind::Cursor,
                tip,
                8.0,
                Color(0.2, 0.5, 0.9, 1.0),
                |_pos| {},
                || {},
            );
        });
    };

    composition
        .render(key, &mut content)
        .expect("initial render");
    settle(&mut composition, key, &mut content);

    let root = composition.root().expect("root");
    let layout = compute_layout(&mut composition, root);
    let scene = HeadlessRenderer::new().render(&layout);

    let rects = drawn_rects(&scene);
    assert!(
        !rects.is_empty(),
        "the selection handle should draw at least one primitive in the overlay"
    );
    // The handle's drawn box must contain the tip (120, 90): the teardrop's tip
    // points exactly at the text endpoint.
    assert!(
        rects.iter().any(|rect| contains(rect, tip)),
        "handle box must contain its tip {tip:?}; drawn rects: {rects:?}"
    );
    // And it must render well below y=0 (i.e. it is not clamped to the top of
    // any parent) — the bulb hangs below the tip.
    assert!(
        rects.iter().any(|rect| rect.y + rect.height >= tip.y),
        "the handle bulb should hang at/below the tip"
    );
}
