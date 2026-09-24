use super::*;

#[test]
fn auto_alpha_triggers_isolation_with_composite_alpha() {
    let layer = GraphicsLayer {
        alpha: 0.5,
        compositing_strategy: CompositingStrategy::Auto,
        ..Default::default()
    };
    let isolation = effective_layer_isolation(&layer).expect("expected isolation");
    assert!(isolation.effect.is_none());
    assert!((isolation.composite_alpha - 127.0 / 255.0).abs() < 1e-6);

    let content = layer_for_content(&layer, Some(&isolation));
    assert!((content.alpha - 1.0).abs() < 1e-6);
}

#[test]
fn a_backend_without_an_offscreen_folds_the_composites_byte_not_the_float() {
    let layer = GraphicsLayer {
        alpha: 0.88,
        compositing_strategy: CompositingStrategy::Auto,
        ..Default::default()
    };
    let folded = local_content_layer(&layer);
    let (composite_alpha, _) = layer_composite_params(&layer).expect("expected isolation");
    assert!((folded.alpha - composite_alpha).abs() < 1e-6);
    assert!((folded.alpha - 224.0 / 255.0).abs() < 1e-6);

    let modulated = GraphicsLayer {
        alpha: 0.88,
        compositing_strategy: CompositingStrategy::ModulateAlpha,
        ..Default::default()
    };
    assert_eq!(local_content_layer(&modulated).alpha, 0.88);
}

#[test]
fn modulate_alpha_keeps_in_place_alpha_without_offscreen() {
    let layer = GraphicsLayer {
        alpha: 0.5,
        compositing_strategy: CompositingStrategy::ModulateAlpha,
        ..Default::default()
    };
    assert!(effective_layer_isolation(&layer).is_none());
}

#[test]
fn non_src_over_layer_blend_triggers_isolation() {
    let layer = GraphicsLayer {
        blend_mode: BlendMode::DstOut,
        compositing_strategy: CompositingStrategy::Auto,
        ..Default::default()
    };
    let isolation = effective_layer_isolation(&layer).expect("expected blend isolation");
    assert_eq!(isolation.blend_mode, BlendMode::DstOut);
    assert!((isolation.composite_alpha - 1.0).abs() < 1e-6);
}

#[test]
fn offscreen_isolation_has_no_effect_payload() {
    let layer = GraphicsLayer {
        alpha: 1.0,
        compositing_strategy: CompositingStrategy::Offscreen,
        ..Default::default()
    };
    let isolation = effective_layer_isolation(&layer).expect("expected isolation");
    assert!(isolation.effect.is_none());
    assert!((isolation.composite_alpha - 1.0).abs() < 1e-6);
}

#[test]
fn render_effect_forces_isolation_even_with_modulate_alpha() {
    let layer = GraphicsLayer {
        alpha: 0.4,
        compositing_strategy: CompositingStrategy::ModulateAlpha,
        render_effect: Some(RenderEffect::blur(4.0)),
        ..Default::default()
    };
    let isolation = effective_layer_isolation(&layer).expect("expected effect isolation");
    assert!(isolation.effect.is_some());
    assert!((isolation.composite_alpha - 1.0).abs() < 1e-6);

    let content = layer_for_content(&layer, Some(&isolation));
    assert!((content.alpha - layer.alpha).abs() < 1e-6);
}

#[test]
fn local_content_layer_keeps_only_local_alpha_and_color_filter() {
    let layer = GraphicsLayer {
        alpha: 0.25,
        color_filter: Some(cranpose_ui_graphics::ColorFilter::Tint(
            cranpose_ui_graphics::Color::RED,
        )),
        shadow_elevation: 6.0,
        translation_x: 14.0,
        clip: true,
        ..Default::default()
    };

    let local = local_content_layer(&layer);
    assert!((local.alpha - 63.0 / 255.0).abs() < 1e-6);
    assert_eq!(local.color_filter, layer.color_filter);
    assert_eq!(local.shadow_elevation, 0.0);
    assert_eq!(local.translation_x, 0.0);
    assert!(!local.clip);
}

#[test]
fn local_content_layer_for_matches_the_composed_form() {
    let filter = Some(cranpose_ui_graphics::ColorFilter::Tint(
        cranpose_ui_graphics::Color::RED,
    ));
    let cases = [
        GraphicsLayer::default(),
        GraphicsLayer {
            alpha: 0.5,
            color_filter: filter,
            compositing_strategy: CompositingStrategy::Auto,
            ..Default::default()
        },
        GraphicsLayer {
            alpha: 0.5,
            compositing_strategy: CompositingStrategy::ModulateAlpha,
            render_effect: Some(RenderEffect::blur(4.0)),
            ..Default::default()
        },
        GraphicsLayer {
            alpha: 0.5,
            compositing_strategy: CompositingStrategy::Offscreen,
            ..Default::default()
        },
        GraphicsLayer {
            blend_mode: BlendMode::DstOut,
            compositing_strategy: CompositingStrategy::Auto,
            ..Default::default()
        },
    ];

    for layer in cases {
        let isolation = effective_layer_isolation(&layer);
        let composed = local_content_layer(&layer_for_content(&layer, isolation.as_ref()));
        let direct = local_content_layer_for(&layer);
        assert!((composed.alpha - direct.alpha).abs() < 1e-6);
        assert_eq!(composed.color_filter, direct.color_filter);
    }
}

#[test]
fn layer_composite_params_match_the_isolation_it_replaces() {
    let cases = [
        GraphicsLayer::default(),
        GraphicsLayer {
            alpha: 0.25,
            compositing_strategy: CompositingStrategy::Auto,
            ..Default::default()
        },
        GraphicsLayer {
            alpha: 0.25,
            compositing_strategy: CompositingStrategy::ModulateAlpha,
            render_effect: Some(RenderEffect::blur(4.0)),
            ..Default::default()
        },
        GraphicsLayer {
            blend_mode: BlendMode::DstOut,
            compositing_strategy: CompositingStrategy::Auto,
            ..Default::default()
        },
    ];

    for layer in cases {
        let isolation = effective_layer_isolation(&layer);
        let expected = isolation.map(|isolation| (isolation.composite_alpha, isolation.blend_mode));
        assert_eq!(expected, layer_composite_params(&layer));
    }
}
