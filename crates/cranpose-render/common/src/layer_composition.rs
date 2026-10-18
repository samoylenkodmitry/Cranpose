use std::borrow::Cow;

use cranpose_ui_graphics::{BlendMode, CompositingStrategy, GraphicsLayer, RenderEffect};

/// What a layer needs to be composited from its own offscreen buffer.
#[derive(Clone)]
pub struct LayerIsolation {
    /// The render effect applied to the buffer, if any.
    pub effect: Option<RenderEffect>,
    /// How the buffer blends onto its parent.
    pub blend_mode: BlendMode,
    /// Alpha applied when the buffer is composited, from 0.0 to 1.0. It is 1.0 for a layer that
    /// modulates alpha in its content instead.
    pub composite_alpha: f32,
}

/// Whether `layer` must be drawn into an offscreen buffer first, given its compositing strategy,
/// effect, blend mode and alpha.
#[inline]
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

/// The alpha `layer`'s content draws with when the layer is drawn in place:
/// its own, or the 8-bit alpha of the isolated composite it needs.
pub fn folded_layer_alpha(layer: &GraphicsLayer) -> f32 {
    if layer.compositing_strategy != CompositingStrategy::ModulateAlpha
        && layer_requires_isolation(layer)
    {
        GraphicsLayer::composite_alpha_8bit(layer.alpha)
    } else {
        layer.alpha
    }
}

/// Returns what `layer` needs to be composited from its own buffer, or `None` when it can be
/// drawn directly.
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
#[inline]
pub fn layer_composite_params(layer: &GraphicsLayer) -> Option<(f32, BlendMode)> {
    layer_requires_isolation(layer).then(|| (isolation_composite_alpha(layer), layer.blend_mode))
}

/// Returns the layer to draw the content with inside the offscreen buffer: `layer` with its alpha
/// reset to one when `isolation` moves the alpha to the composite step.
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

/// The alpha and colour filter of `layer_for_content(layer, isolation)`
/// folded in place, without building it. The content layer differs from
/// `layer` only in the alpha the isolation moves to the composite step, and
/// only that alpha and the colour filter pass to the content, so the two
/// clones the composed form performs — one `GraphicsLayer`, one
/// `RenderEffect` — are pure waste. A layer that passes its content no
/// alpha and no filter, as most do, lends the default layer.
#[inline]
pub fn local_content_layer_for(layer: &GraphicsLayer) -> Cow<'static, GraphicsLayer> {
    let alpha = if layer.compositing_strategy != CompositingStrategy::ModulateAlpha
        && layer_requires_isolation(layer)
    {
        1.0
    } else {
        layer.alpha
    };
    if alpha == 1.0 && layer.color_filter.is_none() {
        return Cow::Borrowed(&GraphicsLayer::DEFAULT);
    }
    Cow::Owned(GraphicsLayer {
        alpha,
        color_filter: layer.color_filter.clone(),
        ..GraphicsLayer::default()
    })
}

#[cfg(test)]
#[path = "tests/layer_composition_tests.rs"]
mod tests;
