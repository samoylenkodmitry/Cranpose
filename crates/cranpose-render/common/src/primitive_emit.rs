use std::rc::Rc;

use cranpose_ui::text::{
    text_style_for_draw_style, AnnotatedString, TextLayoutOptions, TextOverflow, TextStyle,
};
use cranpose_ui_graphics::{
    arc_band, inflate_rect, ArcGeometry, BlendMode, Brush, Color, ColorFilter, CornerRadii,
    DrawPrimitive, GraphicsLayer, ImageBitmap, ImageSampling, Point, Rect, RoundedCornerShape,
    ShadowPrimitive, Stroke, TextPrimitive,
};

use crate::{
    graph::quad_bounds,
    layer_transform::{
        apply_layer_affine_to_point, apply_layer_affine_to_rect, apply_layer_to_quad,
        apply_layer_to_rect, layer_uniform_scale,
    },
    style_shared::{
        apply_layer_to_color, compose_color_filters, resolve_layer_brush, scale_corner_radii,
        ResolvedBrush,
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

/// Resolves the rect a (possibly stroked) rect/round-rect primitive should
/// actually cover, plus the layer-scaled stroke.
///
/// A centered stroke bleeds `width / 2` outside the geometry, so the quad has
/// to grow by that much or the outer half of the outline would be clipped away
/// by its own vertices. The shader shrinks the SDF box back by the same amount.
///
/// Returns `None` for a stroke that cannot draw anything.
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
    // Inflate in pre-scale local space; `apply_layer_affine_to_rect` then
    // scales the padding by scale_x/scale_y while the stroke width itself uses
    // the uniform (minimum) scale. The padding is therefore never smaller than
    // the stroke needs.
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

    fn push_shadow(
        &mut self,
        shadow_primitive: ShadowPrimitive,
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

pub fn draw_shape_params_for_primitive(
    primitive: DrawPrimitive,
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
            _shadow_primitive: ShadowPrimitive,
            _layer_bounds: Rect,
            _layer: &GraphicsLayer,
            _clip: Option<Rect>,
        ) {
        }
    }

    let mut sink = SingleShapeSink { shape: None };
    emit_draw_primitive(
        &primitive,
        layer_bounds,
        layer,
        clip,
        &mut sink,
        Some(blend_mode),
        false,
    );
    sink.shape
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

/// The [`DrawPrimitive::Rect`] arm of [`emit_draw_primitive`] as a pure
/// builder over borrowed fields. The parallel shape-run collect calls these
/// directly from worker threads — a `&DrawPrimitive` cannot cross (the text
/// variant carries `Rc`), but the shape variants' fields can.
#[allow(clippy::too_many_arguments)]
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
#[allow(clippy::too_many_arguments)]
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
#[allow(clippy::too_many_arguments)]
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
    // `rect` already is the tight, cap-inclusive bounding box, so the
    // quad needs no extra inflation here.
    let draw_rect = local_rect.translate(layer_bounds.x, layer_bounds.y);
    let out_rect = apply_layer_affine_to_rect(draw_rect, layer_bounds, layer);
    let quad = apply_layer_to_quad(draw_rect, layer_bounds, layer);
    // Radii scale by the *uniform* (minimum) layer scale, matching how
    // corner radii are handled. Under a non-uniform scale the true
    // shape would be an ellipse; taking the minimum keeps the band
    // strictly inside the (independently scaled) bounding box, so the
    // quad never clips it.
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
    // Borrowing the primitive keeps the per-frame collect walk from cloning
    // the whole enum for every draw; only the payloads a sink actually keeps
    // (brush, image handle, text block, shadow) are cloned below.
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
            sink.push_shadow(shadow_primitive.clone(), layer_bounds, layer, clip);
        }
    }
}

/// Lowers a text primitive into [`TextDrawParams`].
///
/// The block box is translated into layer space and transformed exactly like a
/// `Text` node's rect, and the style is built by the *same*
/// [`text_style_for_draw_style`] the draw scope measured through — so the
/// glyphs the rasterizer lays out and the extent the caller was told about come
/// from one description.
///
/// Returns `None` for geometry no rasterizer would draw, so degenerate text
/// never reaches a scene.
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
        // A draw scope hands the renderer a box it measured itself: re-wrapping
        // or ellipsizing it against that same box could only shorten text the
        // caller already sized for.
        layout_options: TextLayoutOptions {
            soft_wrap: false,
            overflow: TextOverflow::Visible,
            ..TextLayoutOptions::default()
        },
        clip,
    })
}

#[cfg(test)]
mod tests {
    use cranpose_ui_graphics::{Brush, Color, CornerRadii};

    use super::*;

    #[test]
    fn draw_shape_params_for_primitive_returns_transformed_rect_shape() {
        let shape = draw_shape_params_for_primitive(
            DrawPrimitive::Rect {
                rect: Rect {
                    x: 2.0,
                    y: 3.0,
                    width: 8.0,
                    height: 5.0,
                },
                brush: Brush::solid(Color::WHITE),
                stroke: None,
            },
            Rect {
                x: 10.0,
                y: 20.0,
                width: 40.0,
                height: 30.0,
            },
            &GraphicsLayer::default(),
            None,
            BlendMode::SrcOver,
        )
        .expect("rect shape");

        assert_eq!(
            shape.rect,
            Rect {
                x: 12.0,
                y: 23.0,
                width: 8.0,
                height: 5.0,
            }
        );
        assert!(shape.shape.is_none());
    }

    #[test]
    fn draw_shape_params_for_primitive_resolves_blended_round_rect() {
        let shape = draw_shape_params_for_primitive(
            DrawPrimitive::Blend {
                primitive: Box::new(DrawPrimitive::RoundRect {
                    rect: Rect {
                        x: 1.0,
                        y: 1.0,
                        width: 10.0,
                        height: 6.0,
                    },
                    brush: Brush::solid(Color::BLACK),
                    radii: CornerRadii::uniform(4.0),
                    stroke: None,
                }),
                blend_mode: BlendMode::DstOut,
            },
            Rect::from_size(cranpose_ui_graphics::Size {
                width: 20.0,
                height: 20.0,
            }),
            &GraphicsLayer::default(),
            None,
            BlendMode::SrcOver,
        )
        .expect("round rect shape");

        assert_eq!(shape.blend_mode, BlendMode::SrcOver);
        assert!(shape.shape.is_some());
    }

    #[test]
    fn draw_shape_params_for_primitive_rejects_non_shape_primitives() {
        assert!(draw_shape_params_for_primitive(
            DrawPrimitive::Image {
                rect: Rect::from_size(cranpose_ui_graphics::Size {
                    width: 4.0,
                    height: 4.0,
                }),
                image: cranpose_ui_graphics::ImageBitmap::from_rgba8(
                    1,
                    1,
                    vec![255, 255, 255, 255],
                )
                .expect("image"),
                alpha: 1.0,
                color_filter: None,
                sampling: ImageSampling::Nearest,
                src_rect: None,
            },
            Rect::from_size(cranpose_ui_graphics::Size {
                width: 10.0,
                height: 10.0,
            }),
            &GraphicsLayer::default(),
            None,
            BlendMode::SrcOver,
        )
        .is_none());
    }

    // ── Stroke / arc lowering ───────────────────────────────────────────────

    use std::f32::consts::FRAC_PI_2;

    use cranpose_ui_graphics::{Stroke, StrokeCap, StrokeJoin};

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    fn layer_bounds() -> Rect {
        Rect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 100.0,
        }
    }

    #[test]
    fn stroked_rect_inflates_the_quad_by_half_the_width() {
        // Without the inflation the outer half of the outline would be clipped
        // away by the quad it is drawn on.
        let params = draw_shape_params_for_primitive(
            DrawPrimitive::Rect {
                rect: Rect {
                    x: 5.0,
                    y: 5.0,
                    width: 40.0,
                    height: 30.0,
                },
                brush: Brush::solid(Color::WHITE),
                stroke: Some(Stroke::new(6.0).with_join(StrokeJoin::Bevel)),
            },
            layer_bounds(),
            &GraphicsLayer::default(),
            None,
            BlendMode::SrcOver,
        )
        .expect("stroked rect");

        let stroke = params.stroke.expect("stroke must survive lowering");
        assert_eq!(stroke.width, 6.0);
        assert_eq!(stroke.join, StrokeJoin::Bevel);
        // Geometry (15, 25) 40x30, grown by 3 on every side.
        assert_eq!(
            params.local_rect,
            Rect {
                x: 12.0,
                y: 22.0,
                width: 46.0,
                height: 36.0,
            }
        );
        assert_eq!(params.rect, params.local_rect);
        assert!(params.arc.is_none());
    }

    #[test]
    fn stroke_width_and_inflation_follow_the_layer_scale() {
        let layer = GraphicsLayer {
            scale: 2.0,
            transform_origin: cranpose_ui_graphics::TransformOrigin::new(0.0, 0.0),
            ..Default::default()
        };
        let params = draw_shape_params_for_primitive(
            DrawPrimitive::Rect {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 20.0,
                    height: 20.0,
                },
                brush: Brush::solid(Color::WHITE),
                stroke: Some(Stroke::new(4.0)),
            },
            Rect {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            },
            &layer,
            None,
            BlendMode::SrcOver,
        )
        .expect("stroked rect");

        assert_eq!(params.stroke.expect("stroke").width, 8.0);
        // Geometry (-2,-2)..(22,22) scaled 2x about the origin.
        assert_eq!(
            params.local_rect,
            Rect {
                x: -4.0,
                y: -4.0,
                width: 48.0,
                height: 48.0,
            }
        );
    }

    #[test]
    fn zero_width_stroke_emits_nothing() {
        for width in [0.0, -2.0, f32::NAN] {
            assert!(
                draw_shape_params_for_primitive(
                    DrawPrimitive::Rect {
                        rect: Rect::from_size(cranpose_ui_graphics::Size {
                            width: 10.0,
                            height: 10.0,
                        }),
                        brush: Brush::solid(Color::WHITE),
                        stroke: Some(Stroke::new(width)),
                    },
                    layer_bounds(),
                    &GraphicsLayer::default(),
                    None,
                    BlendMode::SrcOver,
                )
                .is_none(),
                "stroke width {width} must not reach the renderer"
            );
        }
    }

    #[test]
    fn arc_lowers_to_a_band_translated_into_layer_space() {
        let arc_rect = Rect {
            x: 50.0,
            y: 50.0,
            width: 12.0,
            height: 12.0,
        };
        let params = draw_shape_params_for_primitive(
            DrawPrimitive::Arc {
                rect: arc_rect,
                brush: Brush::solid(Color::WHITE),
                center: Point::new(50.0, 50.0),
                radius: 12.0,
                start_angle: 0.0,
                sweep_angle: FRAC_PI_2,
                stroke: None,
                inner_radius: 6.0,
            },
            layer_bounds(),
            &GraphicsLayer::default(),
            None,
            BlendMode::SrcOver,
        )
        .expect("arc");

        let arc = params.arc.expect("arc geometry must survive lowering");
        // Center translated by the layer origin, exactly like the bbox.
        assert_eq!(arc.center, Point::new(60.0, 70.0));
        assert_eq!(arc.inner_radius, 6.0);
        assert_eq!(arc.outer_radius, 12.0);
        assert_eq!(arc.cap, StrokeCap::Butt, "a filled sector has flat ends");
        assert!(approx(arc.sweep_angle, FRAC_PI_2));
        assert!(params.stroke.is_none());
        assert!(params.shape.is_none());
        assert_eq!(params.local_rect, arc_rect.translate(10.0, 20.0));
    }

    #[test]
    fn stroked_arc_lowers_to_the_band_around_the_radius() {
        let params = draw_shape_params_for_primitive(
            DrawPrimitive::Arc {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 60.0,
                    height: 60.0,
                },
                brush: Brush::solid(Color::WHITE),
                center: Point::new(30.0, 30.0),
                radius: 20.0,
                start_angle: 0.0,
                sweep_angle: 1.0,
                stroke: Some(Stroke::new(8.0).with_cap(StrokeCap::Round)),
                inner_radius: 0.0,
            },
            Rect::from_size(cranpose_ui_graphics::Size {
                width: 60.0,
                height: 60.0,
            }),
            &GraphicsLayer::default(),
            None,
            BlendMode::SrcOver,
        )
        .expect("stroked arc");

        let arc = params.arc.expect("arc geometry");
        assert_eq!(arc.inner_radius, 16.0);
        assert_eq!(arc.outer_radius, 24.0);
        assert_eq!(arc.cap, StrokeCap::Round);
        assert!(
            params.stroke.is_none(),
            "an arc carries its width in the band radii, not in `stroke`"
        );
    }

    #[test]
    fn arc_radii_and_center_follow_the_layer_transform() {
        let layer = GraphicsLayer {
            scale: 3.0,
            transform_origin: cranpose_ui_graphics::TransformOrigin::new(0.0, 0.0),
            ..Default::default()
        };
        let params = draw_shape_params_for_primitive(
            DrawPrimitive::Arc {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 20.0,
                    height: 20.0,
                },
                brush: Brush::solid(Color::WHITE),
                center: Point::new(10.0, 10.0),
                radius: 10.0,
                start_angle: 0.0,
                sweep_angle: 1.0,
                stroke: None,
                inner_radius: 4.0,
            },
            Rect {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            },
            &layer,
            None,
            BlendMode::SrcOver,
        )
        .expect("arc");

        let arc = params.arc.expect("arc geometry");
        assert_eq!(arc.center, Point::new(30.0, 30.0));
        assert_eq!(arc.inner_radius, 12.0);
        assert_eq!(arc.outer_radius, 30.0);
        // The band must stay inside the quad the renderer emits.
        assert_eq!(
            params.local_rect,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 60.0,
                height: 60.0,
            }
        );
    }

    #[test]
    fn degenerate_arcs_emit_nothing() {
        let base_rect = Rect::from_size(cranpose_ui_graphics::Size {
            width: 20.0,
            height: 20.0,
        });
        let cases: [(f32, f32, f32, Option<Stroke>); 4] = [
            // inner >= outer
            (10.0, 10.0, 1.0, None),
            // zero sweep
            (10.0, 0.0, 0.0, None),
            // zero radius
            (0.0, 0.0, 1.0, None),
            // zero-width stroke
            (10.0, 0.0, 1.0, Some(Stroke::new(0.0))),
        ];
        for (radius, inner_radius, sweep_angle, stroke) in cases {
            assert!(
                draw_shape_params_for_primitive(
                    DrawPrimitive::Arc {
                        rect: base_rect,
                        brush: Brush::solid(Color::WHITE),
                        center: Point::new(10.0, 10.0),
                        radius,
                        start_angle: 0.0,
                        sweep_angle,
                        stroke,
                        inner_radius,
                    },
                    layer_bounds(),
                    &GraphicsLayer::default(),
                    None,
                    BlendMode::SrcOver,
                )
                .is_none(),
                "degenerate arc (r={radius}, inner={inner_radius}, sweep={sweep_angle}) \
                 must not reach the renderer"
            );
        }
    }

    #[test]
    fn fills_still_lower_without_stroke_or_arc() {
        let params = draw_shape_params_for_primitive(
            DrawPrimitive::RoundRect {
                rect: Rect::from_size(cranpose_ui_graphics::Size {
                    width: 10.0,
                    height: 10.0,
                }),
                brush: Brush::solid(Color::WHITE),
                radii: CornerRadii::uniform(2.0),
                stroke: None,
            },
            layer_bounds(),
            &GraphicsLayer::default(),
            None,
            BlendMode::SrcOver,
        )
        .expect("round rect");
        assert!(params.stroke.is_none());
        assert!(params.arc.is_none());
        assert!(params.shape.is_some());
    }

    // ── Text lowering ───────────────────────────────────────────────────────

    use std::rc::Rc as StdRc;

    use cranpose_ui_graphics::{
        FontWeight as DrawFontWeight, TextPrimitive, TextStyle as DrawTextStyle,
    };

    #[derive(Default)]
    struct CollectingTextSink {
        texts: Vec<TextDrawParams>,
    }

    impl DrawPrimitiveSink for CollectingTextSink {
        fn push_shape(&mut self, _params: ShapeDrawParams) {}
        fn push_image(&mut self, _params: ImageDrawParams) {}
        fn push_shadow(
            &mut self,
            _shadow_primitive: ShadowPrimitive,
            _layer_bounds: Rect,
            _layer: &GraphicsLayer,
            _clip: Option<Rect>,
        ) {
        }
        fn push_text(&mut self, params: TextDrawParams) {
            self.texts.push(params);
        }
    }

    fn text_primitive(rect: Rect, text: &str, style: DrawTextStyle) -> DrawPrimitive {
        DrawPrimitive::Text(Box::new(TextPrimitive {
            rect,
            text: StdRc::from(text),
            style,
            color: Color::WHITE,
        }))
    }

    fn lower_text(primitive: DrawPrimitive, layer: &GraphicsLayer) -> Vec<TextDrawParams> {
        let mut sink = CollectingTextSink::default();
        emit_draw_primitive(
            &primitive,
            layer_bounds(),
            layer,
            None,
            &mut sink,
            None,
            false,
        );
        sink.texts
    }

    #[test]
    fn text_lowers_into_the_layer_translated_block_the_scope_measured() {
        let params = lower_text(
            text_primitive(
                Rect {
                    x: 5.0,
                    y: 6.0,
                    width: 40.0,
                    height: 18.0,
                },
                "SCORE",
                DrawTextStyle::new(12.0),
            ),
            &GraphicsLayer::default(),
        );
        assert_eq!(params.len(), 1);
        // Layer bounds are (10, 20); an untransformed layer only translates.
        assert_eq!(
            params[0].rect,
            Rect {
                x: 15.0,
                y: 26.0,
                width: 40.0,
                height: 18.0,
            }
        );
        assert_eq!(params[0].text.text, "SCORE");
        assert_eq!(params[0].font_size, 12.0);
        assert_eq!(params[0].scale, 1.0);
        assert_eq!(params[0].color, Color::WHITE);
    }

    #[test]
    fn text_lowering_carries_the_uniform_layer_scale_for_rasterization() {
        let layer = GraphicsLayer {
            scale: 2.0,
            transform_origin: cranpose_ui_graphics::TransformOrigin::new(0.0, 0.0),
            ..Default::default()
        };
        let params = lower_text(
            text_primitive(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 30.0,
                    height: 14.0,
                },
                "AB",
                DrawTextStyle::new(10.0),
            ),
            &layer,
        );
        assert_eq!(params[0].scale, 2.0, "glyphs rasterize at the layer scale");
        assert!(approx(params[0].rect.width, 60.0), "{:?}", params[0].rect);
    }

    #[test]
    fn text_lowering_folds_the_layer_alpha_into_the_glyph_color() {
        let layer = GraphicsLayer {
            alpha: 0.5,
            ..Default::default()
        };
        let params = lower_text(
            text_primitive(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 30.0,
                    height: 14.0,
                },
                "AB",
                DrawTextStyle::new(10.0),
            ),
            &layer,
        );
        assert!(approx(params[0].color.3, 0.5));
    }

    #[test]
    fn text_lowering_uses_the_same_style_translation_the_draw_scope_measured_with() {
        let style = DrawTextStyle::new(18.0)
            .with_font_family("Fira Sans")
            .with_weight(DrawFontWeight::BOLD);
        let params = lower_text(
            text_primitive(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 30.0,
                    height: 20.0,
                },
                "AB",
                style.clone(),
            ),
            &GraphicsLayer::default(),
        );
        assert_eq!(params[0].text_style, text_style_for_draw_style(&style));
    }

    #[test]
    fn lowered_text_is_never_re_wrapped_against_the_box_it_was_measured_into() {
        // The draw scope already sized the box from the string; wrapping or
        // ellipsizing here could only truncate text the caller asked for.
        let params = lower_text(
            text_primitive(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 4.0,
                    height: 14.0,
                },
                "a very long line",
                DrawTextStyle::new(10.0),
            ),
            &GraphicsLayer::default(),
        );
        assert!(!params[0].layout_options.soft_wrap);
        assert_eq!(params[0].layout_options.overflow, TextOverflow::Visible);
    }

    #[test]
    fn degenerate_text_never_reaches_a_sink() {
        let invisible_layer = GraphicsLayer {
            alpha: 0.0,
            ..Default::default()
        };
        let cases: [(DrawPrimitive, GraphicsLayer); 3] = [
            (
                text_primitive(
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 20.0,
                        height: 10.0,
                    },
                    "",
                    DrawTextStyle::new(10.0),
                ),
                GraphicsLayer::default(),
            ),
            (
                text_primitive(
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 0.0,
                        height: 10.0,
                    },
                    "AB",
                    DrawTextStyle::new(10.0),
                ),
                GraphicsLayer::default(),
            ),
            (
                text_primitive(
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 20.0,
                        height: 10.0,
                    },
                    "AB",
                    DrawTextStyle::new(10.0),
                ),
                invisible_layer,
            ),
        ];
        for (primitive, layer) in cases {
            assert!(
                lower_text(primitive, &layer).is_empty(),
                "degenerate text must not reach the renderer"
            );
        }
    }

    #[test]
    fn blended_text_still_lowers_because_glyphs_composite_src_over() {
        // `DrawScope` offers no blended text, but a nested `Blend` wrapper must
        // not swallow the run if one is built by hand.
        let params = lower_text(
            DrawPrimitive::Blend {
                primitive: Box::new(text_primitive(
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 20.0,
                        height: 10.0,
                    },
                    "AB",
                    DrawTextStyle::new(10.0),
                )),
                blend_mode: BlendMode::DstOut,
            },
            &GraphicsLayer::default(),
        );
        assert_eq!(params.len(), 1);
    }
}
