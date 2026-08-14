use cranpose_ui_graphics::{BlendMode, CompositingStrategy, GraphicsLayer, RenderEffect};

#[derive(Clone)]
pub struct LayerIsolation {
    pub effect: Option<RenderEffect>,
    pub blend_mode: BlendMode,
    pub composite_alpha: f32,
}

pub fn layer_requires_isolation(layer: &GraphicsLayer) -> bool {
    let has_effect = layer.render_effect.is_some();
    let has_layer_blend = layer.blend_mode != BlendMode::SrcOver;
    match layer.compositing_strategy {
        CompositingStrategy::Offscreen => true,
        CompositingStrategy::Auto => has_effect || has_layer_blend || layer.alpha < 1.0,
        CompositingStrategy::ModulateAlpha => has_effect || has_layer_blend,
    }
}

fn isolation_composite_alpha(layer: &GraphicsLayer) -> f32 {
    if layer.compositing_strategy == CompositingStrategy::ModulateAlpha {
        // The float-fold branch, which HWUI takes when nothing overlaps: the
        // alpha never becomes a byte there, so nothing to snap.
        1.0
    } else {
        GraphicsLayer::composite_alpha_8bit(layer.alpha)
    }
}

/// The alpha a layer's own contents carry when the backend has no offscreen to
/// isolate them in and folds the layer's alpha into them instead.
///
/// Folding is an approximation — it fades a subtree's overlapping parts
/// separately where a real layer fades the composited result — but the alpha
/// it folds is not a matter of taste: for a layer that would have been
/// isolated it is the composite's byte, so the two backends land on the same
/// pixel wherever the approximation is exact at all.
fn folded_layer_alpha(layer: &GraphicsLayer) -> f32 {
    if layer.compositing_strategy != CompositingStrategy::ModulateAlpha
        && layer_requires_isolation(layer)
    {
        GraphicsLayer::composite_alpha_8bit(layer.alpha)
    } else {
        layer.alpha
    }
}

pub fn effective_layer_isolation(layer: &GraphicsLayer) -> Option<LayerIsolation> {
    layer_requires_isolation(layer).then(|| LayerIsolation {
        effect: layer.render_effect.clone(),
        blend_mode: layer.blend_mode,
        composite_alpha: isolation_composite_alpha(layer),
    })
}

/// The composite alpha and blend mode an isolated layer contributes at its
/// parent, for callers that never read the isolation's render effect and would
/// otherwise deep-clone it once per layer per frame.
pub fn layer_composite_params(layer: &GraphicsLayer) -> Option<(f32, BlendMode)> {
    layer_requires_isolation(layer).then(|| (isolation_composite_alpha(layer), layer.blend_mode))
}

pub fn layer_for_content(
    layer: &GraphicsLayer,
    isolation: Option<&LayerIsolation>,
) -> GraphicsLayer {
    let mut content = layer.clone();
    if isolation.is_some() && layer.compositing_strategy != CompositingStrategy::ModulateAlpha {
        content.alpha = 1.0;
    }
    content
}

pub fn local_content_layer(layer: &GraphicsLayer) -> GraphicsLayer {
    GraphicsLayer {
        alpha: folded_layer_alpha(layer),
        color_filter: layer.color_filter,
        ..GraphicsLayer::default()
    }
}

/// `local_content_layer(&layer_for_content(layer, isolation))` without building
/// either intermediate. The content layer differs from `layer` only in the
/// alpha the isolation moves to the composite step, and the local layer keeps
/// just that alpha and the colour filter, so the two clones the composed form
/// performs — one `GraphicsLayer`, one `RenderEffect` — are pure waste.
pub fn local_content_layer_for(layer: &GraphicsLayer) -> GraphicsLayer {
    let alpha = if layer.compositing_strategy != CompositingStrategy::ModulateAlpha
        && layer_requires_isolation(layer)
    {
        1.0
    } else {
        layer.alpha
    };
    GraphicsLayer {
        alpha,
        color_filter: layer.color_filter,
        ..GraphicsLayer::default()
    }
}

#[cfg(test)]
mod tests {
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
        // A half composites at 127/255, not at a half: `(int)(0.5f * 255)`.
        assert!((isolation.composite_alpha - 127.0 / 255.0).abs() < 1e-6);

        let content = layer_for_content(&layer, Some(&isolation));
        assert!((content.alpha - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_backend_without_an_offscreen_folds_the_composites_byte_not_the_float() {
        // The software path has no offscreen, so it folds the layer's alpha
        // into the contents. What it folds has to be the alpha the isolating
        // path composites at, or the two backends part company by a level.
        let layer = GraphicsLayer {
            alpha: 0.88,
            compositing_strategy: CompositingStrategy::Auto,
            ..Default::default()
        };
        let folded = local_content_layer(&layer);
        let (composite_alpha, _) = layer_composite_params(&layer).expect("expected isolation");
        assert!((folded.alpha - composite_alpha).abs() < 1e-6);
        assert!((folded.alpha - 224.0 / 255.0).abs() < 1e-6);

        // A layer that names ModulateAlpha is asking for the float fold, which
        // is the branch HWUI takes when nothing overlaps, so it keeps it.
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
            let expected =
                isolation.map(|isolation| (isolation.composite_alpha, isolation.blend_mode));
            assert_eq!(expected, layer_composite_params(&layer));
        }
    }
}
