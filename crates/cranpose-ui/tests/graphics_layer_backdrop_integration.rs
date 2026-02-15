use cranpose_ui::{
    collect_slices_from_modifier, Color, ColorFilter, GraphicsLayer, LayerShape, Modifier,
    RenderEffect, RoundedCornerShape, TransformOrigin,
};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

#[test]
fn backdrop_effect_is_visible_in_modifier_slices() {
    let modifier = Modifier::empty().backdrop_effect(RenderEffect::blur(6.0));
    let slices = collect_slices_from_modifier(&modifier);
    let layer = slices
        .graphics_layer()
        .expect("backdrop effect should produce a graphics layer");

    assert!(layer.backdrop_effect.is_some());
}

#[test]
fn graphics_layer_is_evaluated_on_slice_access() {
    let alpha = Rc::new(Cell::new(0.15f32));
    let modifier = Modifier::empty().graphics_layer({
        let alpha = alpha.clone();
        move || GraphicsLayer {
            alpha: alpha.get(),
            ..Default::default()
        }
    });
    let slices = collect_slices_from_modifier(&modifier);

    assert!((slices.graphics_layer().expect("layer").alpha - 0.15).abs() < 1e-6);
    alpha.set(0.72);
    assert!((slices.graphics_layer().expect("layer").alpha - 0.72).abs() < 1e-6);
}

#[test]
fn stacked_lazy_translation_and_backdrop_effect_are_both_preserved() {
    let modifier = Modifier::empty()
        .graphics_layer(|| GraphicsLayer {
            translation_x: 64.0,
            translation_y: 48.0,
            ..Default::default()
        })
        .backdrop_effect(RenderEffect::blur(8.0));
    let slices = collect_slices_from_modifier(&modifier);
    let layer = slices.graphics_layer().expect("layer expected");

    assert!((layer.translation_x - 64.0).abs() < 1e-6);
    assert!((layer.translation_y - 48.0).abs() < 1e-6);
    assert!(layer.backdrop_effect.is_some());
}

#[test]
fn stacked_tint_modifiers_compose_in_graphics_layer() {
    let outer = Color::from_rgba_u8(255, 128, 128, 255);
    let inner = Color::from_rgba_u8(128, 255, 64, 128);
    let modifier = Modifier::empty().tint(outer).tint(inner);
    let slices = collect_slices_from_modifier(&modifier);
    let layer = slices.graphics_layer().expect("layer expected");

    let filter = layer.color_filter.expect("expected composed filter");
    let source = [0.6, 0.2, 0.8, 0.5];
    let expected = ColorFilter::tint(inner).apply_rgba(ColorFilter::tint(outer).apply_rgba(source));
    let observed = filter.apply_rgba(source);
    assert!((observed[0] - expected[0]).abs() < 1e-6);
    assert!((observed[1] - expected[1]).abs() < 1e-6);
    assert!((observed[2] - expected[2]).abs() < 1e-6);
    assert!((observed[3] - expected[3]).abs() < 1e-6);
}

#[test]
fn graphics_layer_state_writes_auto_request_render_invalidation() {
    let runtime =
        cranpose_core::runtime::Runtime::new(Arc::new(cranpose_core::runtime::DefaultScheduler));
    let x_state = cranpose_core::MutableState::with_runtime(10.0f32, runtime.handle());

    let modifier = Modifier::empty().graphics_layer(move || GraphicsLayer {
        translation_x: x_state.get(),
        ..Default::default()
    });
    let slices = collect_slices_from_modifier(&modifier);

    let _ = cranpose_ui::take_render_invalidation();
    let layer = slices.graphics_layer().expect("layer expected");
    assert!((layer.translation_x - 10.0).abs() < 1e-6);

    x_state.set(42.0);
    assert!(cranpose_ui::take_render_invalidation());
    let updated = slices.graphics_layer().expect("layer expected");
    assert!((updated.translation_x - 42.0).abs() < 1e-6);
}

#[test]
fn stacked_render_effects_chain_inner_then_outer() {
    let outer = RenderEffect::offset(5.0, -2.0);
    let inner = RenderEffect::blur(6.0);
    let modifier = Modifier::empty()
        .graphics_layer_value(GraphicsLayer {
            render_effect: Some(outer.clone()),
            ..Default::default()
        })
        .graphics_layer_value(GraphicsLayer {
            render_effect: Some(inner.clone()),
            ..Default::default()
        });
    let slices = collect_slices_from_modifier(&modifier);
    let layer = slices.graphics_layer().expect("layer expected");

    match layer.render_effect {
        Some(RenderEffect::Chain { first, second }) => {
            assert_eq!(*first, inner);
            assert_eq!(*second, outer);
        }
        other => panic!("expected chained effect, got {other:?}"),
    }
}

#[test]
fn stacked_render_effects_keep_existing_when_inner_unset() {
    let outer = RenderEffect::blur(9.0);
    let modifier = Modifier::empty()
        .graphics_layer_value(GraphicsLayer {
            render_effect: Some(outer.clone()),
            ..Default::default()
        })
        .graphics_layer_value(GraphicsLayer::default());
    let slices = collect_slices_from_modifier(&modifier);
    let layer = slices.graphics_layer().expect("layer expected");

    assert_eq!(layer.render_effect, Some(outer));
}

#[test]
fn stacked_graphics_layers_merge_new_transform_fields() {
    let modifier = Modifier::empty()
        .graphics_layer_value(GraphicsLayer {
            rotation_z: 12.0,
            camera_distance: 8.0,
            transform_origin: TransformOrigin::CENTER,
            shadow_elevation: 0.0,
            shape: LayerShape::Rectangle,
            clip: false,
            ..Default::default()
        })
        .graphics_layer_value(GraphicsLayer {
            rotation_z: 8.0,
            camera_distance: 14.0,
            transform_origin: TransformOrigin::new(0.2, 0.9),
            shadow_elevation: 6.0,
            shape: LayerShape::Rounded(RoundedCornerShape::uniform(9.0)),
            clip: true,
            ..Default::default()
        });

    let slices = collect_slices_from_modifier(&modifier);
    let layer = slices.graphics_layer().expect("layer expected");

    assert!((layer.rotation_z - 20.0).abs() < 1e-6);
    assert!((layer.camera_distance - 14.0).abs() < 1e-6);
    assert_eq!(layer.transform_origin, TransformOrigin::new(0.2, 0.9));
    assert!((layer.shadow_elevation - 6.0).abs() < 1e-6);
    assert_eq!(
        layer.shape,
        LayerShape::Rounded(RoundedCornerShape::uniform(9.0))
    );
    assert!(layer.clip);
}

#[test]
fn inner_default_graphics_layer_resets_parent_local_fields() {
    let modifier = Modifier::empty()
        .graphics_layer_value(GraphicsLayer {
            camera_distance: 24.0,
            transform_origin: TransformOrigin::new(0.1, 0.9),
            shadow_elevation: 7.0,
            ambient_shadow_color: Color::from_rgba_u8(32, 48, 64, 255),
            spot_shadow_color: Color::from_rgba_u8(96, 112, 128, 255),
            shape: LayerShape::Rounded(RoundedCornerShape::uniform(10.0)),
            compositing_strategy: cranpose_ui::CompositingStrategy::Offscreen,
            blend_mode: cranpose_ui::BlendMode::DstOut,
            ..Default::default()
        })
        .graphics_layer_value(GraphicsLayer::default());

    let slices = collect_slices_from_modifier(&modifier);
    let layer = slices.graphics_layer().expect("layer expected");

    assert!((layer.camera_distance - 8.0).abs() < 1e-6);
    assert_eq!(layer.transform_origin, TransformOrigin::CENTER);
    assert!((layer.shadow_elevation - 0.0).abs() < 1e-6);
    assert_eq!(layer.ambient_shadow_color, Color::BLACK);
    assert_eq!(layer.spot_shadow_color, Color::BLACK);
    assert_eq!(layer.shape, LayerShape::Rectangle);
    assert_eq!(
        layer.compositing_strategy,
        cranpose_ui::CompositingStrategy::Auto
    );
    assert_eq!(layer.blend_mode, cranpose_ui::BlendMode::SrcOver);
}
