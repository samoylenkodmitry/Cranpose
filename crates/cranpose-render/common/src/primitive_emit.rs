use cranpose_ui_graphics::{
    BlendMode, Brush, ColorFilter, DrawPrimitive, GraphicsLayer, ImageBitmap, Rect,
    RoundedCornerShape, ShadowPrimitive,
};

use crate::style_shared::{
    apply_layer_affine_to_rect, apply_layer_to_brush, apply_layer_to_quad, apply_layer_to_rect,
    compose_color_filters, layer_uniform_scale, quad_bounds, scale_corner_radii,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimitiveClipSpace {
    Local,
    LayerTransformed,
}

pub struct ShapeDrawParams {
    pub rect: Rect,
    pub local_rect: Rect,
    pub quad: [[f32; 2]; 4],
    pub brush: Brush,
    pub shape: Option<RoundedCornerShape>,
    pub clip: Option<Rect>,
    pub blend_mode: BlendMode,
}

pub struct ImageDrawParams {
    pub rect: Rect,
    pub local_rect: Rect,
    pub quad: [[f32; 2]; 4],
    pub image: ImageBitmap,
    pub alpha: f32,
    pub color_filter: Option<ColorFilter>,
    pub clip: Option<Rect>,
    pub src_rect: Option<Rect>,
    pub blend_mode: BlendMode,
}

pub trait DrawPrimitiveSink {
    fn push_shape(&mut self, params: ShapeDrawParams);

    fn push_image(&mut self, params: ImageDrawParams);

    fn push_shadow(
        &mut self,
        shadow_primitive: ShadowPrimitive,
        layer_bounds: Rect,
        layer: &GraphicsLayer,
        clip: Option<Rect>,
    );
}

pub fn resolve_clip(parent_clip: Option<Rect>, requested_clip: Option<Rect>) -> Option<Rect> {
    match (parent_clip, requested_clip) {
        (Some(parent), Some(current)) => parent.intersect(current),
        (Some(parent), None) => Some(parent),
        (None, Some(current)) => Some(current),
        (None, None) => None,
    }
}

pub fn resolve_primitive_clip(
    local_clip: Option<Rect>,
    layer_bounds: Rect,
    layer: &GraphicsLayer,
    parent_clip: Option<Rect>,
    clip_space: PrimitiveClipSpace,
) -> Option<Rect> {
    let Some(local_clip) = local_clip else {
        return parent_clip;
    };
    let clip_rect = Rect {
        x: layer_bounds.x + local_clip.x,
        y: layer_bounds.y + local_clip.y,
        width: local_clip.width,
        height: local_clip.height,
    };
    let requested_clip = match clip_space {
        PrimitiveClipSpace::Local => clip_rect,
        PrimitiveClipSpace::LayerTransformed => apply_layer_to_rect(clip_rect, layer_bounds, layer),
    };
    resolve_clip(parent_clip, Some(requested_clip))
}

pub fn emit_draw_primitive<S: DrawPrimitiveSink>(
    primitive: DrawPrimitive,
    layer_bounds: Rect,
    layer: &GraphicsLayer,
    clip: Option<Rect>,
    sink: &mut S,
    blend_mode: Option<BlendMode>,
) {
    match primitive {
        DrawPrimitive::Content => {}
        DrawPrimitive::Blend {
            primitive,
            blend_mode: nested,
        } => emit_draw_primitive(
            *primitive,
            layer_bounds,
            layer,
            clip,
            sink,
            blend_mode.or(Some(nested)),
        ),
        DrawPrimitive::Rect {
            rect: local_rect,
            brush,
        } => {
            let draw_rect = local_rect.translate(layer_bounds.x, layer_bounds.y);
            let local_rect = apply_layer_affine_to_rect(draw_rect, layer_bounds, layer);
            let quad = apply_layer_to_quad(draw_rect, layer_bounds, layer);
            sink.push_shape(ShapeDrawParams {
                rect: quad_bounds(quad),
                local_rect,
                quad,
                brush: apply_layer_to_brush(brush, layer),
                shape: None,
                clip,
                blend_mode: blend_mode.unwrap_or(BlendMode::SrcOver),
            });
        }
        DrawPrimitive::RoundRect {
            rect: local_rect,
            brush,
            radii,
        } => {
            let draw_rect = local_rect.translate(layer_bounds.x, layer_bounds.y);
            let local_rect = apply_layer_affine_to_rect(draw_rect, layer_bounds, layer);
            let quad = apply_layer_to_quad(draw_rect, layer_bounds, layer);
            let shape = RoundedCornerShape::with_radii(scale_corner_radii(
                radii,
                layer_uniform_scale(layer),
            ));
            sink.push_shape(ShapeDrawParams {
                rect: quad_bounds(quad),
                local_rect,
                quad,
                brush: apply_layer_to_brush(brush, layer),
                shape: Some(shape),
                clip,
                blend_mode: blend_mode.unwrap_or(BlendMode::SrcOver),
            });
        }
        DrawPrimitive::Image {
            rect: local_rect,
            image,
            alpha,
            color_filter,
            src_rect,
        } => {
            let draw_rect = local_rect.translate(layer_bounds.x, layer_bounds.y);
            let local_rect = apply_layer_affine_to_rect(draw_rect, layer_bounds, layer);
            let quad = apply_layer_to_quad(draw_rect, layer_bounds, layer);
            sink.push_image(ImageDrawParams {
                rect: quad_bounds(quad),
                local_rect,
                quad,
                image,
                alpha: (alpha * layer.alpha).clamp(0.0, 1.0),
                color_filter: compose_color_filters(color_filter, layer.color_filter),
                clip,
                src_rect,
                blend_mode: blend_mode.unwrap_or(BlendMode::SrcOver),
            });
        }
        DrawPrimitive::Shadow(shadow_primitive) => {
            sink.push_shadow(shadow_primitive, layer_bounds, layer, clip);
        }
    }
}
