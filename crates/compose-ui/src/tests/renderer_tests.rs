use super::*;
use crate::modifier::{Brush, Color, Modifier};
use crate::primitives::{Column, ColumnSpec, SubcomposeLayout, Text};
use crate::{
    layout::LayoutEngine, Composition, Placement, SubcomposeLayoutScope, SubcomposeMeasureScope,
};
use compose_core::{location_key, MemoryApplier, SlotId};

fn compute_layout(composition: &mut Composition<MemoryApplier>, root: NodeId) -> LayoutTree {
    let handle = composition.runtime_handle();
    let layout = {
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        let result = applier
            .compute_layout(
                root,
                Size {
                    width: 200.0,
                    height: 200.0,
                },
            )
            .expect("layout");
        applier.clear_runtime_handle();
        result
    };
    layout
}

#[test]
fn renderer_emits_background_and_text() {
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, || {
            Text(
                "Hello".to_string(),
                Modifier::empty().background(Color(0.1, 0.2, 0.3, 1.0)),
            );
        })
        .expect("initial render");

    let root = composition.root().expect("text root");
    let layout = compute_layout(&mut composition, root);
    let renderer = HeadlessRenderer::new();
    let scene = renderer.render(&layout);

    assert_eq!(scene.operations().len(), 2);
    assert!(matches!(
        scene.operations()[0],
        RenderOp::Primitive {
            layer: PaintLayer::Behind,
            ..
        }
    ));
    match &scene.operations()[1] {
        RenderOp::Text { value, .. } => assert_eq!(value, "Hello"),
        other => panic!("unexpected op: {other:?}"),
    }
}

#[test]
fn renderer_honors_resolved_background_shape() {
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, || {
            Text(
                "Rounded".to_string(),
                Modifier::empty().background(Color(0.5, 0.2, 0.2, 1.0))
                    .then(Modifier::empty().rounded_corners(12.0)),
            );
        })
        .expect("initial render");

    let root = composition.root().expect("text root");
    let layout = compute_layout(&mut composition, root);
    let renderer = HeadlessRenderer::new();
    let scene = renderer.render(&layout);

    assert!(
        matches!(
            &scene.operations()[0],
            RenderOp::Primitive {
                primitive: DrawPrimitive::RoundRect { .. },
                ..
            }
        ),
        "expected rounded rect background primitive"
    );
}

#[test]
fn renderer_translates_draw_commands() {
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, || {
            Column(
                Modifier::empty().padding(10.0)
                    .then(Modifier::empty().background(Color(0.3, 0.3, 0.9, 1.0)))
                    .then(Modifier::empty().draw_behind(|scope| {
                        scope.draw_rect(Brush::solid(Color(0.8, 0.0, 0.0, 1.0)));
                    })),
                ColumnSpec::default(),
                || {
                    Text(
                        "Content".to_string(),
                        Modifier::empty().draw_behind(|scope| {
                            scope.draw_rect(Brush::solid(Color(0.2, 0.2, 0.2, 1.0)));
                        })
                        .then(Modifier::empty().draw_with_content(|scope| {
                            scope.draw_rect(Brush::solid(Color(0.0, 0.0, 0.0, 1.0)));
                        })),
                    );
                },
            );
        })
        .expect("initial render");

    let root = composition.root().expect("column root");
    let layout = compute_layout(&mut composition, root);
    let renderer = HeadlessRenderer::new();
    let scene = renderer.render(&layout);

    let behind: Vec<_> = scene.primitives_for(PaintLayer::Behind).collect();
    assert_eq!(behind.len(), 3); // column background + column draw_behind + text draw_behind
    let mut saw_translated = false;
    for primitive in behind {
        match primitive {
            DrawPrimitive::Rect { rect, .. } => {
                if rect.x >= 10.0 && rect.y >= 10.0 {
                    saw_translated = true;
                }
            }
            DrawPrimitive::RoundRect { rect, .. } => {
                if rect.x >= 10.0 && rect.y >= 10.0 {
                    saw_translated = true;
                }
            }
        }
    }
    assert!(
        saw_translated,
        "expected a translated primitive for padded text"
    );

    let overlay_ops: Vec<_> = scene
        .operations()
        .iter()
        .filter(|op| {
            matches!(
                op,
                RenderOp::Primitive {
                    layer: PaintLayer::Overlay,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(overlay_ops.len(), 1);
    if let RenderOp::Primitive { primitive, .. } = overlay_ops[0] {
        match primitive {
            DrawPrimitive::Rect { rect, .. } | DrawPrimitive::RoundRect { rect, .. } => {
                assert!(rect.x >= 10.0);
                assert!(rect.y >= 10.0);
            }
        }
    }
}

#[test]
fn renderer_renders_subcompose_background() {
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, || {
            SubcomposeLayout(
                Modifier::empty().background(Color(0.4, 0.4, 0.4, 1.0)),
                |scope, constraints| {
                    let children = scope.subcompose(SlotId::new(0), || {
                        Text("Subcomposed".to_string(), Modifier::empty());
                    });
                    let placements: Vec<_> = children
                        .into_iter()
                        .map(|child| Placement::new(child.node_id(), 0.0, 0.0, 0))
                        .collect();
                    let (width, height) = constraints.constrain(40.0, 20.0);
                    scope.layout(width, height, placements)
                },
            );
        })
        .expect("initial render");

    let root = composition.root().expect("subcompose root");
    let layout = compute_layout(&mut composition, root);
    let renderer = HeadlessRenderer::new();
    let scene = renderer.render(&layout);

    assert!(scene.operations().len() >= 2);
    match &scene.operations()[0] {
        RenderOp::Primitive { node_id, .. } => assert_eq!(*node_id, root),
        other => panic!("unexpected first op: {other:?}"),
    }
    assert!(scene
        .operations()
        .iter()
        .any(|op| matches!(op, RenderOp::Text { value, .. } if value == "Subcomposed")));
}
