use std::rc::Rc;

use cranpose_ui::Brush;
use cranpose_ui_graphics::{Color, TransformOrigin};

use super::*;

#[test]
fn apply_layer_to_rect_rotates_around_center() {
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 40.0,
    };
    let layer = GraphicsLayer {
        rotation_z: 90.0,
        ..Default::default()
    };

    let transformed = apply_layer_to_rect(rect, rect, &layer);
    assert!((transformed.width - 40.0).abs() < 0.01);
    assert!((transformed.height - 100.0).abs() < 0.01);
}

#[test]
fn apply_layer_to_rect_honors_transform_origin() {
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 40.0,
    };
    let layer = GraphicsLayer {
        rotation_z: 90.0,
        transform_origin: TransformOrigin::new(0.0, 0.0),
        ..Default::default()
    };

    let transformed = apply_layer_to_rect(rect, rect, &layer);
    assert!((transformed.x + 40.0).abs() < 0.01);
    assert!((transformed.y - 0.0).abs() < 0.01);
}

#[test]
fn apply_layer_to_rect_camera_distance_changes_projection() {
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 60.0,
    };
    let near_camera = GraphicsLayer {
        rotation_y: 25.0,
        camera_distance: 8.0,
        ..Default::default()
    };
    let far_camera = GraphicsLayer {
        rotation_y: 25.0,
        camera_distance: 24.0,
        ..Default::default()
    };

    let near = apply_layer_to_rect(rect, rect, &near_camera);
    let far = apply_layer_to_rect(rect, rect, &far_camera);
    let delta = (near.x - far.x).abs()
        + (near.y - far.y).abs()
        + (near.width - far.width).abs()
        + (near.height - far.height).abs();
    assert!(delta > 0.05);
}

#[test]
fn apply_draw_commands_scales_round_rect_radii_with_uniform_axis_scale() {
    let command = DrawCommand::Behind(Rc::new(
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
        alpha: 0.5,
        ..Default::default()
    };
    let mut scene = CompositorScene::new();
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

    scene.flush_loose();
    let run = &scene.runs[0];
    let record = run.tables().shapes.get(0).unwrap();
    assert_eq!(record.radii, [10.0; 4], "the record keeps the app's radii");
    assert_eq!(
        run.placement.offset,
        cranpose_ui_graphics::Point::new(0.0, 0.0)
    );
    assert!(
        (record.color[3] - 0.5).abs() < 1e-6,
        "a loose primitive carries the layer's paint in its brush"
    );
}
