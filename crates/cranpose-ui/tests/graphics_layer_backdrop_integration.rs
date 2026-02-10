use cranpose_ui::{collect_slices_from_modifier, GraphicsLayer, Modifier, RenderEffect};
use std::cell::Cell;
use std::rc::Rc;

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
