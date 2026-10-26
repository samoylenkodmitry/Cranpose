use cranpose_ui::Brush;
use cranpose_ui_graphics::{Color, DrawPlacement};

use super::*;

#[test]
fn apply_draw_commands_scales_round_rect_radii_with_uniform_axis_scale() {
    let command = DrawCommand::Behind(std::rc::Rc::new(
        |scope: &mut cranpose_ui_graphics::DrawScopeDefault| {
            scope.push_recorded(vec![DrawPrimitive::RoundRect {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 80.0,
                    height: 40.0,
                },
                brush: Brush::solid(Color::BLACK),
                radii: CornerRadii::uniform(10.0),
                stroke: None,
            }]);
        },
    ));

    let layer = GraphicsLayer {
        scale: 1.0,
        scale_x: 2.0,
        scale_y: 0.5,
        ..Default::default()
    };
    let mut scene = RasterScene::new();
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 80.0,
        height: 40.0,
    };
    apply_draw_commands(
        &[command],
        DrawPlacement::Behind,
        bounds,
        Size {
            width: 80.0,
            height: 40.0,
        },
        &layer,
        None,
        &mut scene,
    );

    let shape = scene.shapes[0].outline.rounded().expect("rounded shape");
    let radii = shape.radii();
    assert!((radii.top_left - 5.0).abs() < 1e-6);
    assert!((radii.top_right - 5.0).abs() < 1e-6);
    assert!((radii.bottom_right - 5.0).abs() < 1e-6);
    assert!((radii.bottom_left - 5.0).abs() < 1e-6);
}

#[test]
fn primitives_for_placement_uses_last_content_marker() {
    let command = DrawCommand::WithContent(std::rc::Rc::new(
        |scope: &mut cranpose_ui_graphics::DrawScopeDefault| {
            scope.push_recorded(vec![
                DrawPrimitive::Rect {
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 10.0,
                        height: 10.0,
                    },
                    brush: Brush::solid(Color::from_rgba_u8(255, 0, 0, 255)),
                    stroke: None,
                },
                DrawPrimitive::Content,
                DrawPrimitive::Rect {
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 10.0,
                        height: 10.0,
                    },
                    brush: Brush::solid(Color::from_rgba_u8(0, 255, 0, 255)),
                    stroke: None,
                },
                DrawPrimitive::Content,
                DrawPrimitive::Rect {
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 10.0,
                        height: 10.0,
                    },
                    brush: Brush::solid(Color::from_rgba_u8(0, 0, 255, 255)),
                    stroke: None,
                },
            ]);
        },
    ));

    let behind = primitives_for_placement(
        &command,
        DrawPlacement::Behind,
        Size {
            width: 10.0,
            height: 10.0,
        },
    );
    let overlay = primitives_for_placement(
        &command,
        DrawPlacement::Overlay,
        Size {
            width: 10.0,
            height: 10.0,
        },
    );

    assert_eq!(behind.len(), 2);
    assert_eq!(overlay.len(), 1);
}
