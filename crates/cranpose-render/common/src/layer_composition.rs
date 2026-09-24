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
        1.0
    } else {
        GraphicsLayer::composite_alpha_8bit(layer.alpha)
    }
}

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
#[path = "tests/layer_composition_tests.rs"]
mod tests;
