use std::rc::Rc;

use cranpose_ui::text::{
    AnnotatedString, TextLayoutOptions, TextOverflow, TextStyle, text_style_for_draw_style,
};
use cranpose_ui_graphics::{
    ArcGeometry, BlendMode, Brush, Color, ColorFilter, CornerRadii, DrawPrimitive, GraphicsLayer,
    ImageBitmap, ImageSampling, LineGeometry, Point, Rect, RoundedCornerShape, ShadowPrimitive,
    Stroke, TextPrimitive, Trapezoid, arc_band, inflate_rect,
};

use crate::{
    graph::quad_bounds,
    layer_transform::{
        apply_layer_affine_to_point, apply_layer_affine_to_rect, apply_layer_to_quad,
        apply_layer_to_rect, layer_uniform_scale,
    },
    style_shared::{
        ResolvedBrush, apply_layer_to_brush, apply_layer_to_color, compose_color_filters,
        resolve_layer_brush, scale_corner_radii,
    },
};

/// The space in which the clip rectangle of a primitive is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimitiveClipSpace {
    /// The clip is in the layer's own space and the layer transform does not apply to it.
    Local,
    /// The clip is moved with the layer transform, like the primitive itself.
    LayerTransformed,
}

/// A shape ready for a renderer to draw, with the layer transform, paint and clip resolved.
pub struct ShapeDrawParams {
    /// Axis-aligned bounds of `quad`.
    pub rect: Rect,
    /// The shape's rectangle after the layer's scale and translation, before any rotation.
    pub local_rect: Rect,
    /// The shape's rectangle after the full layer transform, as four corners.
    pub quad: [[f32; 2]; 4],
    /// Layer-resolved at emit time. Solid — effectively every shape of a
    /// heavy animated frame — is an inline color; only gradients carry a
    /// cloned [`Brush`].
    pub brush: ResolvedBrush,
    /// Rounded corners of the shape, or `None` for sharp corners.
    pub shape: Option<RoundedCornerShape>,
    /// `Some` means "stroke the outline of `local_rect`/`shape`" instead of
    /// filling it. The width is already in `local_rect` units (the layer scale
    /// has been applied), and `local_rect`/`quad` have already been inflated by
    /// half that width so the outer half of the stroke has geometry to land on.
    pub stroke: Option<Stroke>,
    /// `Some` replaces the rect geometry entirely with a circular band. The
    /// center and radii are in `local_rect` units.
    pub arc: Option<ArcGeometry>,
    /// `Some` replaces the rect geometry entirely with a stroked segment, its
    /// ends and width in `local_rect` units.
    pub line: Option<LineGeometry>,
    /// `Some` replaces the rect geometry entirely with a slice of a path
    /// fill, in `local_rect` units; `local_rect` is the rect its brush
    /// resolves against.
    pub trapezoid: Option<Trapezoid>,
    /// Clip rectangle that cuts the shape, or `None` when nothing clips it.
    pub clip: Option<Rect>,
    /// How the shape blends onto what lies beneath it.
    pub blend_mode: BlendMode,
    /// Whether the shape sits in an animated motion context, where a renderer must not snap it
    /// to the pixel grid.
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

/// An image ready for a renderer to draw, with the layer transform, opacity and clip resolved.
pub struct ImageDrawParams {
    /// Axis-aligned bounds of `quad`.
    pub rect: Rect,
    /// The image's rectangle after the layer's scale and translation, before any rotation.
    pub local_rect: Rect,
    /// The image's rectangle after the full layer transform, as four corners.
    pub quad: [[f32; 2]; 4],
    /// The bitmap to draw.
    pub image: ImageBitmap,
    /// Opacity of the image from 0.0 to 1.0.
    pub alpha: f32,
    /// Color filter applied to the image, if any.
    pub color_filter: Option<ColorFilter>,
    /// How the bitmap is sampled when it is scaled.
    pub sampling: ImageSampling,
    /// Clip rectangle that cuts the image, or `None` when nothing clips it.
    pub clip: Option<Rect>,
    /// Region of the bitmap to draw, in bitmap pixels, or `None` for the whole bitmap.
    pub src_rect: Option<Rect>,
    /// How the image blends onto what lies beneath it.
    pub blend_mode: BlendMode,
    /// Whether the image sits in an animated motion context, where a renderer must not snap it
    /// to the pixel grid.
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
    /// The text to draw.
    pub text: Rc<AnnotatedString>,
    /// `text` as a renderer's draws carry it across threads.
    pub render_text: std::sync::Arc<cranpose_ui::text::RenderString>,
    /// Color that fills the glyphs.
    pub color: Color,
    /// Style the text is drawn with.
    pub text_style: TextStyle,
    /// Font size in logical pixels, before `scale` is applied.
    pub font_size: f32,
    /// Uniform layer scale — the factor glyphs are rasterized at.
    pub scale: f32,
    /// Layout options the text is laid out with.
    pub layout_options: TextLayoutOptions,
    /// Clip rectangle that cuts the text, or `None` when nothing clips it.
    pub clip: Option<Rect>,
}

/// Receives the draw calls that `emit_draw_primitive` produces from a draw primitive.
pub trait DrawPrimitiveSink {
    /// Draws a shape.
    fn push_shape(&mut self, params: ShapeDrawParams);

    /// Draws an image.
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

/// Returns the clip for a primitive: `local_clip` moved to `layer_bounds`, moved with the layer
/// transform for `PrimitiveClipSpace::LayerTransformed`, and intersected with `parent_clip`.
/// Returns `parent_clip` when the primitive has no clip of its own.
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

// Where a shape lands: the layer that places it, the clip that cuts it and
// the blend it draws with.
struct ShapePlacement<'a> {
    layer_bounds: Rect,
    layer: &'a GraphicsLayer,
    clip: Option<Rect>,
    blend_mode: BlendMode,
    motion_context_animated: bool,
}

impl ShapePlacement<'_> {
    // A shape over `draw_rect` with no geometry of its own; each builder sets its geometry.
    fn fill(&self, draw_rect: Rect, brush: &Brush) -> ShapeDrawParams {
        let quad = apply_layer_to_quad(draw_rect, self.layer_bounds, self.layer);
        ShapeDrawParams {
            rect: quad_bounds(quad),
            local_rect: apply_layer_affine_to_rect(draw_rect, self.layer_bounds, self.layer),
            quad,
            brush: resolve_layer_brush(brush, self.layer),
            shape: None,
            stroke: None,
            arc: None,
            line: None,
            trapezoid: None,
            clip: self.clip,
            blend_mode: self.blend_mode,
            motion_context_animated: self.motion_context_animated,
        }
    }

    fn translate(&self, rect: Rect) -> Rect {
        rect.translate(self.layer_bounds.x, self.layer_bounds.y)
    }

    fn place(&self, point: Point) -> Point {
        apply_layer_affine_to_point(
            Point::new(point.x + self.layer_bounds.x, point.y + self.layer_bounds.y),
            self.layer_bounds,
            self.layer,
        )
    }
}

fn rect_shape_params(
    local_rect: Rect,
    brush: &Brush,
    stroke: Option<Stroke>,
    placement: &ShapePlacement,
) -> Option<ShapeDrawParams> {
    let (draw_rect, stroke) =
        stroked_draw_rect(local_rect, stroke, placement.layer_bounds, placement.layer)?;
    Some(ShapeDrawParams {
        stroke,
        ..placement.fill(draw_rect, brush)
    })
}

fn round_rect_shape_params(
    local_rect: Rect,
    brush: &Brush,
    radii: CornerRadii,
    stroke: Option<Stroke>,
    placement: &ShapePlacement,
) -> Option<ShapeDrawParams> {
    let (draw_rect, stroke) =
        stroked_draw_rect(local_rect, stroke, placement.layer_bounds, placement.layer)?;
    let radii = scale_corner_radii(radii, layer_uniform_scale(placement.layer));
    Some(ShapeDrawParams {
        shape: Some(RoundedCornerShape::with_radii(radii)),
        stroke,
        ..placement.fill(draw_rect, brush)
    })
}

// `arc` is in scope coordinates and `local_rect` is its tight bounds.
fn arc_shape_params(
    local_rect: Rect,
    brush: &Brush,
    arc: ArcGeometry,
    placement: &ShapePlacement,
) -> Option<ShapeDrawParams> {
    if arc.is_degenerate() {
        return None;
    }
    let scale = layer_uniform_scale(placement.layer);
    Some(ShapeDrawParams {
        arc: Some(arc.scaled_about(placement.place(arc.center), scale)),
        ..placement.fill(placement.translate(local_rect), brush)
    })
}

// The segment's ends are placed by the layer's transform and its width is
// scaled by the layer's uniform scale.
fn line_shape_params(
    brush: &Brush,
    start: Point,
    end: Point,
    stroke: Stroke,
    placement: &ShapePlacement,
) -> Option<ShapeDrawParams> {
    let line = LineGeometry::new(start, end, stroke);
    if line.is_degenerate() {
        return None;
    }
    let scale = layer_uniform_scale(placement.layer);
    Some(ShapeDrawParams {
        line: Some(line.placed(placement.place(start), placement.place(end), scale)),
        ..placement.fill(placement.translate(line.bounds()), brush)
    })
}

// The brush rect is placed by the layer's transform as a rect is, and the
// slice's corners are placed by it as points.
fn trapezoid_shape_params(
    brush_rect: Rect,
    brush: &Brush,
    trapezoid: Trapezoid,
    placement: &ShapePlacement,
) -> Option<ShapeDrawParams> {
    if trapezoid.right <= trapezoid.left {
        return None;
    }
    let place = |x: f32, y: f32| placement.place(Point::new(x, y));
    let [top_left, top_right, bottom_left, bottom_right] = [
        place(trapezoid.left, trapezoid.top[0]),
        place(trapezoid.right, trapezoid.top[1]),
        place(trapezoid.left, trapezoid.bottom[0]),
        place(trapezoid.right, trapezoid.bottom[1]),
    ];
    Some(ShapeDrawParams {
        trapezoid: Some(Trapezoid {
            left: top_left.x,
            right: top_right.x,
            top: [top_left.y, top_right.y],
            bottom: [bottom_left.y, bottom_right.y],
            ..trapezoid
        }),
        ..placement.fill(placement.translate(brush_rect), brush)
    })
}

/// Lowers `primitive` to shape, image, shadow and text draw calls on `sink`, with the transform
/// of `layer` applied at `layer_bounds`. `blend_mode` is the mode inherited from an enclosing
/// blend primitive, and `motion_context_animated` is passed on to every call. Content and group
/// markers draw nothing: the scene builder turns groups into layers.
pub fn emit_draw_primitive<S: DrawPrimitiveSink>(
    primitive: &DrawPrimitive,
    layer_bounds: Rect,
    layer: &GraphicsLayer,
    clip: Option<Rect>,
    sink: &mut S,
    blend_mode: Option<BlendMode>,
    motion_context_animated: bool,
) {
    let placement = ShapePlacement {
        layer_bounds,
        layer,
        clip,
        blend_mode: blend_mode.unwrap_or(BlendMode::SrcOver),
        motion_context_animated,
    };
    match primitive {
        DrawPrimitive::Content | DrawPrimitive::Group(_) => {}
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
            rect,
            brush,
            stroke,
        } => {
            if let Some(params) = rect_shape_params(*rect, brush, *stroke, &placement) {
                sink.push_shape(params);
            }
        }
        DrawPrimitive::RoundRect {
            rect,
            brush,
            radii,
            stroke,
        } => {
            if let Some(params) = round_rect_shape_params(*rect, brush, *radii, *stroke, &placement)
            {
                sink.push_shape(params);
            }
        }
        DrawPrimitive::Arc {
            rect,
            brush,
            center,
            radius,
            start_angle,
            sweep_angle,
            stroke,
            inner_radius,
        } => {
            let (band_inner, band_outer, cap) = arc_band(*radius, *inner_radius, *stroke);
            let arc = ArcGeometry::new(
                *center,
                band_inner,
                band_outer,
                *start_angle,
                *sweep_angle,
                cap,
            );
            if let Some(params) = arc_shape_params(*rect, brush, arc, &placement) {
                sink.push_shape(params);
            }
        }
        DrawPrimitive::Line {
            brush,
            start,
            end,
            stroke,
            ..
        } => {
            if let Some(params) = line_shape_params(brush, *start, *end, *stroke, &placement) {
                sink.push_shape(params);
            }
        }
        DrawPrimitive::Trapezoid {
            rect,
            brush,
            trapezoid,
        } => {
            if let Some(params) = trapezoid_shape_params(*rect, brush, *trapezoid, &placement) {
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
                color_filter: compose_color_filters(
                    color_filter.as_ref(),
                    layer.color_filter.as_ref(),
                ),
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
    let mut text_style = text_style_for_draw_style(&text.style);
    // A gradient fills the glyphs through the style's brush, as a `Text`
    // composable's does, laid over the text's extent.
    let color = match text.brush {
        Brush::Solid(color) => apply_layer_to_color(color, layer),
        gradient => {
            let gradient = apply_layer_to_brush(gradient, layer);
            let first = gradient
                .colors()
                .first()
                .copied()
                .unwrap_or(Color::TRANSPARENT);
            text_style.span_style.brush = Some(gradient);
            first
        }
    };
    if !text_style
        .span_style
        .brush
        .as_ref()
        .map_or(color.3 > 0.0, Brush::paints)
    {
        return None;
    }

    Some(TextDrawParams {
        rect,
        text: cranpose_ui::text::shared_plain_annotated_string(text.text.as_ref()),
        render_text: cranpose_ui::text::shared_plain_render_string(text.text.as_ref()),
        color,
        text_style,
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
