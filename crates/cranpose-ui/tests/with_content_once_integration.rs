use std::{cell::Cell, rc::Rc};

use cranpose_ui::{
    Box, BoxSpec, HeadlessRenderer, Modifier, PaintLayer, Size, Text, TextStyle,
    run_test_composition,
};
use cranpose_ui_graphics::{Brush, Color, DrawPrimitive, DrawScope};

#[derive(Clone, Copy)]
enum RenderPath {
    LayoutTree,
    Applier,
}

fn render_with_content(
    draw: impl Fn(&mut dyn DrawScope) + 'static,
    path: RenderPath,
) -> (Rc<Cell<usize>>, cranpose_ui::RecordedRenderScene) {
    let calls = Rc::new(Cell::new(0));
    let callback_calls = Rc::clone(&calls);
    let draw = Rc::new(draw);
    let callback_draw = Rc::clone(&draw);
    let mut composition = run_test_composition(move || {
        let calls = Rc::clone(&callback_calls);
        let draw = Rc::clone(&callback_draw);
        Box(
            Modifier::empty()
                .size(Size::new(40.0, 20.0))
                .draw_with_content(move |scope| {
                    calls.set(calls.get() + 1);
                    draw(scope);
                }),
            BoxSpec::default(),
            || {
                Text("child".to_string(), Modifier::empty(), TextStyle::default());
            },
        );
    });

    let root = composition.root().expect("the composition has a root");
    let runtime = composition.runtime_handle();
    let layout = {
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(runtime);
        let measured = cranpose_ui::measure_layout(&mut applier, root, Size::new(80.0, 80.0))
            .expect("the composition measures");
        applier.clear_runtime_handle();
        measured.into_layout_tree().expect("the tree is available")
    };
    let renderer = HeadlessRenderer::new();
    let scene = match path {
        RenderPath::LayoutTree => composition.with_app_context(|| renderer.render(&layout)),
        RenderPath::Applier => {
            let runtime = composition.runtime_handle();
            let mut applier = composition.applier_mut();
            applier.set_runtime_handle(runtime);
            let scene = renderer.render_from_applier(&mut applier, root);
            applier.clear_runtime_handle();
            scene
        }
    };
    (calls, scene)
}

fn solid_colors(primitives: impl Iterator<Item = DrawPrimitive>) -> Vec<Color> {
    primitives
        .map(|primitive| match primitive {
            DrawPrimitive::Rect {
                brush: Brush::Solid(color),
                ..
            } => color,
            other => panic!("expected a solid rectangle, got {other:?}"),
        })
        .collect()
}

#[test]
fn with_content_callback_runs_once_and_splits_all_markers() {
    let red = Color(1.0, 0.0, 0.0, 1.0);
    let green = Color(0.0, 1.0, 0.0, 1.0);
    let blue = Color(0.0, 0.0, 1.0, 1.0);
    let (calls, scene) = render_with_content(
        move |scope| {
            scope.draw_rect(Brush::solid(red));
            scope.draw_content();
            scope.draw_rect(Brush::solid(green));
            scope.draw_content();
            scope.draw_rect(Brush::solid(blue));
        },
        RenderPath::LayoutTree,
    );

    assert_eq!(calls.get(), 1, "the draw callback ran more than once");
    assert_eq!(
        solid_colors(scene.primitives_for(PaintLayer::Behind).cloned()),
        [red, green]
    );
    assert_eq!(
        solid_colors(scene.primitives_for(PaintLayer::Overlay).cloned()),
        [blue]
    );
    assert!(matches!(
        scene.operations().get(2),
        Some(cranpose_ui::RenderOp::Text { value, .. }) if value == "child"
    ));
}

#[test]
fn with_content_callback_runs_once_without_content_markers() {
    let red = Color(1.0, 0.0, 0.0, 1.0);
    let blue = Color(0.0, 0.0, 1.0, 1.0);
    let (calls, scene) = render_with_content(
        move |scope| {
            scope.draw_rect(Brush::solid(red));
            scope.draw_rect(Brush::solid(blue));
        },
        RenderPath::Applier,
    );

    assert_eq!(calls.get(), 1, "the draw callback ran more than once");
    assert!(scene.primitives_for(PaintLayer::Behind).next().is_none());
    assert_eq!(
        solid_colors(scene.primitives_for(PaintLayer::Overlay).cloned()),
        [red, blue]
    );
}
