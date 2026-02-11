use cranpose_ui::{
    collect_slices_from_modifier, Color, ColorFilter, GraphicsLayer, Modifier, RenderEffect,
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
fn lazy_graphics_layer_is_evaluated_on_slice_access() {
    let alpha = Rc::new(Cell::new(0.15f32));
    let modifier = Modifier::empty().graphics_layer_lazy({
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
        .graphics_layer_lazy(|| GraphicsLayer {
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
    let modifier = Modifier::empty()
        .tint(Color::from_rgba_u8(255, 128, 128, 255))
        .tint(Color::from_rgba_u8(128, 255, 64, 128));
    let slices = collect_slices_from_modifier(&modifier);
    let layer = slices.graphics_layer().expect("layer expected");

    let Some(ColorFilter::Tint(tint)) = layer.color_filter else {
        panic!("expected composed tint");
    };
    assert!((tint.r() - (128.0 / 255.0)).abs() < 1e-6);
    assert!((tint.g() - (128.0 / 255.0)).abs() < 1e-6);
    assert!((tint.b() - (64.0 / 255.0 * 128.0 / 255.0)).abs() < 1e-6);
    assert!((tint.a() - (128.0 / 255.0)).abs() < 1e-6);
}

#[test]
fn lazy_graphics_layer_state_writes_auto_request_render_invalidation() {
    let runtime =
        cranpose_core::runtime::Runtime::new(Arc::new(cranpose_core::runtime::DefaultScheduler));
    let x_state = cranpose_core::MutableState::with_runtime(10.0f32, runtime.handle());

    let modifier = Modifier::empty().graphics_layer_lazy({
        let x_state = x_state;
        move || GraphicsLayer {
            translation_x: x_state.get(),
            ..Default::default()
        }
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
