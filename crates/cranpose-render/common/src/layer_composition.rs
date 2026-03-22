use cranpose_ui_graphics::{BlendMode, CompositingStrategy, GraphicsLayer, RenderEffect};

#[derive(Clone)]
pub struct LayerIsolation {
    pub effect: Option<RenderEffect>,
    pub blend_mode: BlendMode,
    pub composite_alpha: f32,
}

pub fn effective_layer_isolation(layer: &GraphicsLayer) -> Option<LayerIsolation> {
    let has_effect = layer.render_effect.is_some();
    let has_layer_blend = layer.blend_mode != BlendMode::SrcOver;
    let requires_isolation = match layer.compositing_strategy {
        CompositingStrategy::Offscreen => true,
        CompositingStrategy::Auto => has_effect || has_layer_blend || layer.alpha < 1.0,
        CompositingStrategy::ModulateAlpha => has_effect || has_layer_blend,
    };

    if !requires_isolation {
        return None;
    }

    let composite_alpha = if layer.compositing_strategy == CompositingStrategy::ModulateAlpha {
        1.0
    } else {
        layer.alpha.clamp(0.0, 1.0)
    };

    Some(LayerIsolation {
        effect: layer.render_effect.clone(),
        blend_mode: layer.blend_mode,
        composite_alpha,
    })
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
        alpha: layer.alpha,
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
        assert!((isolation.composite_alpha - 0.5).abs() < 1e-6);

        let content = layer_for_content(&layer, Some(&isolation));
        assert!((content.alpha - 1.0).abs() < 1e-6);
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
        assert!((local.alpha - 0.25).abs() < 1e-6);
        assert_eq!(local.color_filter, layer.color_filter);
        assert_eq!(local.shadow_elevation, 0.0);
        assert_eq!(local.translation_x, 0.0);
        assert!(!local.clip);
    }
}
