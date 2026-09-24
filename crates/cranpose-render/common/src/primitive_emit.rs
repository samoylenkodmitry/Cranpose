use std::rc::Rc;

use cranpose_ui::text::{
    AnnotatedString, TextLayoutOptions, TextOverflow, TextStyle, text_style_for_draw_style,
};
use cranpose_ui_graphics::{
    ArcGeometry, BlendMode, Brush, Color, ColorFilter, CornerRadii, DrawPrimitive, GraphicsLayer,
    ImageBitmap, ImageSampling, Point, Rect, RoundedCornerShape, ShadowPrimitive, Stroke,
    TextPrimitive, arc_band, inflate_rect,
};

use crate::{
    graph::quad_bounds,
    layer_transform::{
        apply_layer_affine_to_point, apply_layer_affine_to_rect, apply_layer_to_quad,
        apply_layer_to_rect, layer_uniform_scale,
    },
    style_shared::{
        ResolvedBrush, apply_layer_to_color, compose_color_filters, resolve_layer_brush,
        scale_corner_radii,
    },
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
    /// Layer-resolved at emit time. Solid — effectively every shape of a
    /// heavy animated frame — is an inline color; only gradients carry a
    /// cloned [`Brush`].
    pub brush: ResolvedBrush,
    pub shape: Option<RoundedCornerShape>,
    /// `Some` means "stroke the outline of `local_rect`/`shape`" instead of
    /// filling it. The width is already in `local_rect` units (the layer scale
    /// has been applied), and `local_rect`/`quad` have already been inflated by
    /// half that width so the outer half of the stroke has geometry to land on.
    pub stroke: Option<Stroke>,
    /// `Some` replaces the rect geometry entirely with a circular band. The
    /// center and radii are in `local_rect` units.
    pub arc: Option<ArcGeometry>,
    pub clip: Option<Rect>,
    pub blend_mode: BlendMode,
    pub motion_context_animated: bool,
}

fn stroked_draw_rect(
    local_rect: Rect,
    stroke: Option<Stroke>,
    layer_bounds: Rect,
    layer: &GraphicsLayer,
) -> Option<(Rect, Option<Stroke>)> {
    let draw_rect = local_rect.translate(layer_bounds.x, layer_bounds.y);
    let Some(stroke) = stroke else {
        return Some((draw_rect, None));
    };
    if !stroke.is_visible() {
        return None;
    }
    let outset = stroke.half_width();
    Some((
        inflate_rect(draw_rect, outset),
        Some(stroke.scaled(layer_uniform_scale(layer))),
    ))
}

pub struct ImageDrawParams {
    pub rect: Rect,
    pub local_rect: Rect,
    pub quad: [[f32; 2]; 4],
    pub image: ImageBitmap,
    pub alpha: f32,
    pub color_filter: Option<ColorFilter>,
    pub sampling: ImageSampling,
    pub clip: Option<Rect>,
    pub src_rect: Option<Rect>,
    pub blend_mode: BlendMode,
    pub motion_context_animated: bool,
}

/// A `DrawScope` text run, lowered into the vocabulary the text pipeline
/// already speaks.
///
/// The fields line up one-for-one with `CompositorScene::push_text` in both
/// backends, so a text primitive joins the same glyph atlas, run cache and
/// shader the `Text` composable uses instead of getting a pipeline of its own.
pub struct TextDrawParams {
    /// Layer-transformed block box. Glyphs are laid out from its top-left; the
    /// draw scope already resolved alignment into it.
    pub rect: Rect,
    pub text: Rc<AnnotatedString>,
    pub color: Color,
    pub text_style: TextStyle,
    pub font_size: f32,
    /// Uniform layer scale — the factor glyphs are rasterized at.
    pub scale: f32,
    pub layout_options: TextLayoutOptions,
    pub clip: Option<Rect>,
}

pub trait DrawPrimitiveSink {
    fn push_shape(&mut self, params: ShapeDrawParams);

    fn push_image(&mut self, params: ImageDrawParams);

    /// Emits a shadow without transferring ownership of its caster and cutout.
    fn push_shadow(
        &mut self,
        shadow_primitive: &ShadowPrimitive,
        layer_bounds: Rect,
        layer: &GraphicsLayer,
        clip: Option<Rect>,
    );

    /// Draws a text run. The default drops it, for sinks that only collect
    /// geometry (shadow casters, hit testing) and backends with no text
    /// pipeline.
    fn push_text(&mut self, params: TextDrawParams) {
        let _ = params;
    }
}

/// Resolves a borrowed shape with the layer transform, paint, and clipping applied.
pub fn draw_shape_params_for_primitive(
    primitive: &DrawPrimitive,
    layer_bounds: Rect,
    layer: &GraphicsLayer,
    clip: Option<Rect>,
    blend_mode: BlendMode,
) -> Option<ShapeDrawParams> {
    struct SingleShapeSink {
        shape: Option<ShapeDrawParams>,
    }

    impl DrawPrimitiveSink for SingleShapeSink {
        fn push_shape(&mut self, params: ShapeDrawParams) {
            if self.shape.is_none() {
                self.shape = Some(params);
            }
        }

        fn push_image(&mut self, _params: ImageDrawParams) {}

        fn push_shadow(
            &mut self,
            _shadow_primitive: &ShadowPrimitive,
            _layer_bounds: Rect,
            _layer: &GraphicsLayer,
            _clip: Option<Rect>,
        ) {
        }
    }

    let mut sink = SingleShapeSink { shape: None };
    emit_draw_primitive(
        primitive,
        layer_bounds,
        layer,
        clip,
        &mut sink,
        Some(blend_mode),
        false,
    );
    sink.shape
}

/// The clip a node paints within once its own clip meets the clips above
/// it: `None` only when nothing clips at all. Two clips that do not overlap
/// resolve to [`Rect::EMPTY`], never to `None` -- a list that has scrolled a
/// control past its edge has clipped the control away, not set it free.
pub fn resolve_clip(parent_clip: Option<Rect>, requested_clip: Option<Rect>) -> Option<Rect> {
    match (parent_clip, requested_clip) {
        (Some(parent), Some(current)) => Some(parent.intersect(current).unwrap_or(Rect::EMPTY)),
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

/// The [`DrawPrimitive::Rect`] arm of [`emit_draw_primitive`] as a pure
/// builder over borrowed fields. The parallel shape-run collect calls these
/// directly from worker threads — a `&DrawPrimitive` cannot cross (the text
/// variant carries `Rc`), but the shape variants' fields can.
#[expect(clippy::too_many_arguments)]
pub fn rect_shape_params(
    local_rect: Rect,
    brush: &Brush,
    stroke: Option<Stroke>,
    layer_bounds: Rect,
    layer: &GraphicsLayer,
    clip: Option<Rect>,
    blend_mode: BlendMode,
    motion_context_animated: bool,
) -> Option<ShapeDrawParams> {
    let (draw_rect, stroke) = stroked_draw_rect(local_rect, stroke, layer_bounds, layer)?;
    let local_rect = apply_layer_affine_to_rect(draw_rect, layer_bounds, layer);
    let quad = apply_layer_to_quad(draw_rect, layer_bounds, layer);
    Some(ShapeDrawParams {
        rect: quad_bounds(quad),
        local_rect,
        quad,
        brush: resolve_layer_brush(brush, layer),
        shape: None,
        stroke,
        arc: None,
        clip,
        blend_mode,
        motion_context_animated,
    })
}

/// The [`DrawPrimitive::RoundRect`] arm of [`emit_draw_primitive`]; see
/// [`rect_shape_params`].
#[expect(clippy::too_many_arguments)]
pub fn round_rect_shape_params(
    local_rect: Rect,
    brush: &Brush,
    radii: CornerRadii,
    stroke: Option<Stroke>,
    layer_bounds: Rect,
    layer: &GraphicsLayer,
    clip: Option<Rect>,
    blend_mode: BlendMode,
    motion_context_animated: bool,
) -> Option<ShapeDrawParams> {
    let (draw_rect, stroke) = stroked_draw_rect(local_rect, stroke, layer_bounds, layer)?;
    let local_rect = apply_layer_affine_to_rect(draw_rect, layer_bounds, layer);
    let quad = apply_layer_to_quad(draw_rect, layer_bounds, layer);
    let shape =
        RoundedCornerShape::with_radii(scale_corner_radii(radii, layer_uniform_scale(layer)));
    Some(ShapeDrawParams {
        rect: quad_bounds(quad),
        local_rect,
        quad,
        brush: resolve_layer_brush(brush, layer),
        shape: Some(shape),
        stroke,
        arc: None,
        clip,
        blend_mode,
        motion_context_animated,
    })
}

/// The [`DrawPrimitive::Arc`] arm of [`emit_draw_primitive`]; see
/// [`rect_shape_params`].
#[expect(clippy::too_many_arguments)]
pub fn arc_shape_params(
    local_rect: Rect,
    brush: &Brush,
    center: Point,
    radius: f32,
    start_angle: f32,
    sweep_angle: f32,
    stroke: Option<Stroke>,
    inner_radius: f32,
    layer_bounds: Rect,
    layer: &GraphicsLayer,
    clip: Option<Rect>,
    blend_mode: BlendMode,
    motion_context_animated: bool,
) -> Option<ShapeDrawParams> {
    let (band_inner, band_outer, cap) = arc_band(radius, inner_radius, stroke);
    let arc = ArcGeometry::new(
        center,
        band_inner,
        band_outer,
        start_angle,
        sweep_angle,
        cap,
    );
    if arc.is_degenerate() {
        return None;
    }
    let draw_rect = local_rect.translate(layer_bounds.x, layer_bounds.y);
    let out_rect = apply_layer_affine_to_rect(draw_rect, layer_bounds, layer);
    let quad = apply_layer_to_quad(draw_rect, layer_bounds, layer);
    let scale = layer_uniform_scale(layer);
    let arc_center = apply_layer_affine_to_point(
        Point::new(center.x + layer_bounds.x, center.y + layer_bounds.y),
        layer_bounds,
        layer,
    );
    Some(ShapeDrawParams {
        rect: quad_bounds(quad),
        local_rect: out_rect,
        quad,
        brush: resolve_layer_brush(brush, layer),
        shape: None,
        stroke: None,
        arc: Some(arc.scaled_about(arc_center, scale)),
        clip,
        blend_mode,
        motion_context_animated,
    })
}

pub fn emit_draw_primitive<S: DrawPrimitiveSink>(
    primitive: &DrawPrimitive,
    layer_bounds: Rect,
    layer: &GraphicsLayer,
    clip: Option<Rect>,
    sink: &mut S,
    blend_mode: Option<BlendMode>,
    motion_context_animated: bool,
) {
    match primitive {
        DrawPrimitive::Content => {}
        DrawPrimitive::Blend {
            primitive,
            blend_mode: nested,
        } => emit_draw_primitive(
            primitive,
            layer_bounds,
            layer,
            clip,
            sink,
            blend_mode.or(Some(*nested)),
            motion_context_animated,
        ),
        DrawPrimitive::Rect {
            rect: local_rect,
            brush,
            stroke,
        } => {
            if let Some(params) = rect_shape_params(
                *local_rect,
                brush,
                *stroke,
                layer_bounds,
                layer,
                clip,
                blend_mode.unwrap_or(BlendMode::SrcOver),
                motion_context_animated,
            ) {
                sink.push_shape(params);
            }
        }
        DrawPrimitive::RoundRect {
            rect: local_rect,
            brush,
            radii,
            stroke,
        } => {
            if let Some(params) = round_rect_shape_params(
                *local_rect,
                brush,
                *radii,
                *stroke,
                layer_bounds,
                layer,
                clip,
                blend_mode.unwrap_or(BlendMode::SrcOver),
                motion_context_animated,
            ) {
                sink.push_shape(params);
            }
        }
        DrawPrimitive::Arc {
            rect: local_rect,
            brush,
            center,
            radius,
            start_angle,
            sweep_angle,
            stroke,
            inner_radius,
        } => {
            if let Some(params) = arc_shape_params(
                *local_rect,
                brush,
                *center,
                *radius,
                *start_angle,
                *sweep_angle,
                *stroke,
                *inner_radius,
                layer_bounds,
                layer,
                clip,
                blend_mode.unwrap_or(BlendMode::SrcOver),
                motion_context_animated,
            ) {
                sink.push_shape(params);
            }
        }
        DrawPrimitive::Image {
            rect: local_rect,
            image,
            alpha,
            color_filter,
            sampling,
            src_rect,
        } => {
            let draw_rect = local_rect.translate(layer_bounds.x, layer_bounds.y);
            let local_rect = apply_layer_affine_to_rect(draw_rect, layer_bounds, layer);
            let quad = apply_layer_to_quad(draw_rect, layer_bounds, layer);
            sink.push_image(ImageDrawParams {
                rect: quad_bounds(quad),
                local_rect,
                quad,
                image: image.clone(),
                alpha: (alpha * layer.alpha).clamp(0.0, 1.0),
                color_filter: compose_color_filters(*color_filter, layer.color_filter),
                sampling: *sampling,
                clip,
                src_rect: *src_rect,
                blend_mode: blend_mode.unwrap_or(BlendMode::SrcOver),
                motion_context_animated,
            });
        }
        DrawPrimitive::Text(text) => {
            if let Some(params) = text_draw_params((**text).clone(), layer_bounds, layer, clip) {
                sink.push_text(params);
            }
        }
        DrawPrimitive::Shadow(shadow_primitive) => {
            sink.push_shadow(shadow_primitive, layer_bounds, layer, clip);
        }
    }
}

fn text_draw_params(
    text: TextPrimitive,
    layer_bounds: Rect,
    layer: &GraphicsLayer,
    clip: Option<Rect>,
) -> Option<TextDrawParams> {
    if text.text.is_empty() {
        return None;
    }
    let draw_rect = text.rect.translate(layer_bounds.x, layer_bounds.y);
    let rect = apply_layer_to_rect(draw_rect, layer_bounds, layer);
    if !(rect.width > 0.0 && rect.height > 0.0) {
        return None;
    }
    let scale = layer_uniform_scale(layer);
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let color = apply_layer_to_color(text.color, layer);
    if color.3 <= 0.0 {
        return None;
    }

    Some(TextDrawParams {
        rect,
        text: cranpose_ui::text::shared_plain_annotated_string(text.text.as_ref()),
        color,
        text_style: text_style_for_draw_style(&text.style),
        font_size: text.style.resolved_font_size(),
        scale,
        layout_options: TextLayoutOptions {
            soft_wrap: false,
            overflow: TextOverflow::Visible,
            ..TextLayoutOptions::default()
        },
        clip,
    })
}

#[cfg(test)]
#[path = "tests/primitive_emit_tests.rs"]
mod tests;
