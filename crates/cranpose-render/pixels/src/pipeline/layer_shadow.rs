// A graphics layer's elevation shadow, as blurred samples of its shape.

use cranpose_render_common::{
    Brush,
    layer_shadow::{ShadowLight, layer_shadow_geometry},
    layer_transform::layer_uniform_scale,
};
use cranpose_ui_graphics::{
    BlendMode, Color, GraphicsLayer, LayerShape, Rect, RoundedCornerShape, whole,
};

use super::RasterLayerBounds;
use crate::{scene::RasterScene, style::scale_corner_radii};

pub(super) fn push_layer_shadow(
    scene: &mut RasterScene,
    layer: &GraphicsLayer,
    layer_bounds: RasterLayerBounds,
    transformed_bounds: Rect,
    clip: Option<Rect>,
    light: ShadowLight,
) {
    if layer.shadow_elevation <= 0.0 {
        return;
    }
    let caster = match layer.shape {
        LayerShape::Rectangle => None,
        LayerShape::Rounded(shape) => {
            let scale = layer_uniform_scale(layer).max(0.1);
            let resolved = shape.resolve(
                layer_bounds.local_bounds.width,
                layer_bounds.local_bounds.height,
            );
            Some(RoundedCornerShape::with_radii(scale_corner_radii(
                resolved, scale,
            )))
        }
    };
    for (shadow, color) in layer_shadow_geometry(layer, transformed_bounds, caster, light).passes()
    {
        scene.push_shadow(shadow, color, clip);
    }
}

#[derive(Clone, Copy)]
pub(super) struct ShadowSample {
    pub(super) expansion: f32,
    pub(super) weight: f32,
}

pub(super) fn blur_samples(blur_radius: f32) -> Vec<ShadowSample> {
    if blur_radius <= f32::EPSILON {
        return Vec::new();
    }

    let sample_count = (whole::ceil(blur_radius * 2.4) as usize).clamp(8, 36);
    let sigma = (blur_radius * 0.5).max(1.0);
    let mut samples = Vec::with_capacity(sample_count);
    let mut weight_sum = 0.0f32;

    for index in 0..sample_count {
        let t0 = index as f32 / sample_count as f32;
        let t1 = (index + 1) as f32 / sample_count as f32;
        let center = blur_radius * (t0 + t1) * 0.5;
        let expansion = blur_radius * t1;
        let weight = (-0.5 * (center / sigma).powi(2)).exp().max(0.0001);
        samples.push(ShadowSample { expansion, weight });
        weight_sum += weight;
    }

    if weight_sum <= f32::EPSILON {
        return vec![ShadowSample {
            expansion: blur_radius,
            weight: 1.0,
        }];
    }

    for sample in &mut samples {
        sample.weight /= weight_sum;
    }

    samples
}

pub(super) fn expanded_shape_rect(shape: &crate::scene::DrawShape, expansion: f32) -> Rect {
    Rect {
        x: shape.rect.x - expansion,
        y: shape.rect.y - expansion,
        width: (shape.rect.width + expansion * 2.0).max(0.0),
        height: (shape.rect.height + expansion * 2.0).max(0.0),
    }
}

pub(super) fn push_blurred_shape_samples(
    scene: &mut RasterScene,
    shape: &crate::scene::DrawShape,
    blend_mode: BlendMode,
    clip: Option<Rect>,
    blur_radius: f32,
) {
    let samples = blur_samples(blur_radius.max(1.0));
    if samples.is_empty() {
        scene.push_shape(
            shape.rect,
            shape.brush.clone(),
            shape.outline.rounded(),
            clip,
            blend_mode,
        );
        return;
    }

    for sample in samples.iter().rev() {
        scene.push_shape(
            expanded_shape_rect(shape, sample.expansion),
            scale_brush_alpha(shape.brush.clone(), sample.weight),
            shape.outline.rounded(),
            clip,
            blend_mode,
        );
    }
}

fn scale_color_alpha(color: Color, alpha: f32) -> Color {
    Color(
        color.r(),
        color.g(),
        color.b(),
        (color.a() * alpha).clamp(0.0, 1.0),
    )
}

pub(super) fn scale_brush_alpha(brush: Brush, alpha: f32) -> Brush {
    brush.map_colors(|color| scale_color_alpha(color, alpha))
}
