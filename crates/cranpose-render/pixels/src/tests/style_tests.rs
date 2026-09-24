use cranpose_ui::Brush;
use cranpose_ui_graphics::{
    Color, ColorFilter, CompositingStrategy, LayerShape, RenderEffect, RoundedCornerShape,
    TransformOrigin,
};

use super::*;

#[test]
fn combine_layers_clears_effects_without_new_layer() {
    let current = GraphicsLayer {
        alpha: 0.7,
        scale: 1.2,
        translation_x: 4.0,
        translation_y: 6.0,
        color_filter: None,
        render_effect: Some(RenderEffect::blur(4.0)),
        backdrop_effect: Some(RenderEffect::blur(2.0)),
        ..Default::default()
    };

    let combined = combine_layers(current.clone(), None);
    assert_eq!(combined.alpha, current.alpha);
    assert_eq!(combined.scale, current.scale);
    assert_eq!(combined.translation_x, current.translation_x);
    assert_eq!(combined.translation_y, current.translation_y);
    assert_eq!(combined.compositing_strategy, CompositingStrategy::Auto);
    assert_eq!(combined.blend_mode, BlendMode::SrcOver);
    assert!(combined.render_effect.is_none());
    assert!(combined.backdrop_effect.is_none());
}

#[test]
fn combine_layers_uses_local_effect_configuration() {
    let parent = GraphicsLayer {
        render_effect: Some(RenderEffect::blur(8.0)),
        ..Default::default()
    };
    let local = GraphicsLayer {
        render_effect: Some(RenderEffect::offset(5.0, 1.0)),
        backdrop_effect: Some(RenderEffect::blur(1.0)),
        ..Default::default()
    };

    let combined = combine_layers(parent, Some(local.clone()));
    assert_eq!(combined.render_effect, local.render_effect);
    assert_eq!(combined.backdrop_effect, local.backdrop_effect);
}

#[test]
fn combine_layers_composes_color_filters_in_order() {
    let parent_filter = ColorFilter::modulate(Color::from_rgba_u8(255, 128, 128, 255));
    let parent = GraphicsLayer {
        color_filter: Some(parent_filter),
        ..Default::default()
    };
    let local_filter = ColorFilter::tint(Color::from_rgba_u8(128, 255, 64, 128));
    let local = GraphicsLayer {
        color_filter: Some(local_filter),
        ..Default::default()
    };

    let combined = combine_layers(parent, Some(local));
    let filter = combined.color_filter.expect("composed filter");
    let source = [0.8, 0.5, 0.2, 0.75];
    let expected = local_filter.apply_rgba(parent_filter.apply_rgba(source));
    let observed = filter.apply_rgba(source);
    assert!((observed[0] - expected[0]).abs() < 1e-6);
    assert!((observed[1] - expected[1]).abs() < 1e-6);
    assert!((observed[2] - expected[2]).abs() < 1e-6);
    assert!((observed[3] - expected[3]).abs() < 1e-6);
}

#[test]
fn combine_layers_multiplies_axis_scales() {
    let parent = GraphicsLayer {
        scale: 1.2,
        scale_x: 1.1,
        scale_y: 0.9,
        ..Default::default()
    };
    let local = GraphicsLayer {
        scale: 0.5,
        scale_x: 0.8,
        scale_y: 1.5,
        ..Default::default()
    };

    let combined = combine_layers(parent, Some(local));
    assert!((combined.scale - 0.6).abs() < 1e-6);
    assert!((combined.scale_x - 0.88).abs() < 1e-6);
    assert!((combined.scale_y - 1.35).abs() < 1e-6);
}

#[test]
fn combine_layers_merges_rotation_clip_shape_and_shadow() {
    let parent = GraphicsLayer {
        rotation_x: 1.0,
        rotation_y: 2.0,
        rotation_z: 3.0,
        camera_distance: 8.0,
        transform_origin: TransformOrigin::CENTER,
        shadow_elevation: 0.0,
        ambient_shadow_color: Color::BLACK,
        spot_shadow_color: Color::BLACK,
        shape: LayerShape::Rectangle,
        clip: false,
        ..Default::default()
    };
    let local = GraphicsLayer {
        rotation_x: 4.0,
        rotation_y: 5.0,
        rotation_z: 6.0,
        camera_distance: 12.0,
        transform_origin: TransformOrigin::new(0.25, 0.75),
        shadow_elevation: 7.0,
        ambient_shadow_color: Color::from_rgba_u8(10, 20, 30, 255),
        spot_shadow_color: Color::from_rgba_u8(40, 50, 60, 255),
        shape: LayerShape::Rounded(RoundedCornerShape::uniform(8.0)),
        clip: true,
        ..Default::default()
    };

    let combined = combine_layers(parent, Some(local));
    assert!((combined.rotation_x - 5.0).abs() < 1e-6);
    assert!((combined.rotation_y - 7.0).abs() < 1e-6);
    assert!((combined.rotation_z - 9.0).abs() < 1e-6);
    assert!((combined.camera_distance - 12.0).abs() < 1e-6);
    assert_eq!(combined.transform_origin, TransformOrigin::new(0.25, 0.75));
    assert!((combined.shadow_elevation - 7.0).abs() < 1e-6);
    assert_eq!(
        combined.ambient_shadow_color,
        Color::from_rgba_u8(10, 20, 30, 255)
    );
    assert_eq!(
        combined.spot_shadow_color,
        Color::from_rgba_u8(40, 50, 60, 255)
    );
    assert_eq!(
        combined.shape,
        LayerShape::Rounded(RoundedCornerShape::uniform(8.0))
    );
    assert!(combined.clip);
}

#[test]
fn combine_layers_local_defaults_reset_parent_local_fields() {
    let parent = GraphicsLayer {
        camera_distance: 24.0,
        transform_origin: TransformOrigin::new(0.1, 0.9),
        shadow_elevation: 6.0,
        ambient_shadow_color: Color::from_rgba_u8(20, 40, 60, 255),
        spot_shadow_color: Color::from_rgba_u8(80, 100, 120, 255),
        shape: LayerShape::Rounded(RoundedCornerShape::uniform(9.0)),
        compositing_strategy: CompositingStrategy::Offscreen,
        blend_mode: BlendMode::DstOut,
        ..Default::default()
    };

    let combined = combine_layers(parent, Some(GraphicsLayer::default()));

    assert!((combined.camera_distance - 8.0).abs() < 1e-6);
    assert_eq!(combined.transform_origin, TransformOrigin::CENTER);
    assert!((combined.shadow_elevation - 0.0).abs() < 1e-6);
    assert_eq!(combined.ambient_shadow_color, Color::BLACK);
    assert_eq!(combined.spot_shadow_color, Color::BLACK);
    assert_eq!(combined.shape, LayerShape::Rectangle);
    assert_eq!(combined.compositing_strategy, CompositingStrategy::Auto);
    assert_eq!(combined.blend_mode, BlendMode::SrcOver);
}

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

    let shape = scene.shapes[0].shape.expect("rounded shape");
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
