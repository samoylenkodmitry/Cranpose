use cranpose_ui_graphics::{
    arc_band, inflate_rect, ArcGeometry, BlendMode, Brush, ColorFilter, DrawPrimitive,
    GraphicsLayer, ImageBitmap, ImageSampling, Point, Rect, RoundedCornerShape, ShadowPrimitive,
    Stroke,
};

use crate::graph::quad_bounds;
use crate::layer_transform::{
    apply_layer_affine_to_point, apply_layer_affine_to_rect, apply_layer_to_quad,
    apply_layer_to_rect, layer_uniform_scale,
};
use crate::style_shared::{apply_layer_to_brush, compose_color_filters, scale_corner_radii};

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
    motion_context_animated: bool,
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
            motion_context_animated,
        ),
        DrawPrimitive::Rect {
            rect: local_rect,
            brush,
            stroke,
        } => {
            let Some((draw_rect, stroke)) =
                stroked_draw_rect(local_rect, stroke, layer_bounds, layer)
            else {
                return;
            };
            let local_rect = apply_layer_affine_to_rect(draw_rect, layer_bounds, layer);
            let quad = apply_layer_to_quad(draw_rect, layer_bounds, layer);
            sink.push_shape(ShapeDrawParams {
                rect: quad_bounds(quad),
                local_rect,
                quad,
                brush: apply_layer_to_brush(brush, layer),
                shape: None,
                stroke,
                arc: None,
                clip,
                blend_mode: blend_mode.unwrap_or(BlendMode::SrcOver),
                motion_context_animated,
            });
        }
        DrawPrimitive::RoundRect {
            rect: local_rect,
            brush,
            radii,
            stroke,
        } => {
            let Some((draw_rect, stroke)) =
                stroked_draw_rect(local_rect, stroke, layer_bounds, layer)
            else {
                return;
            };
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
                stroke,
                arc: None,
                clip,
                blend_mode: blend_mode.unwrap_or(BlendMode::SrcOver),
                motion_context_animated,
            });
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
                return;
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
            sink.push_shape(ShapeDrawParams {
                rect: quad_bounds(quad),
                local_rect: out_rect,
                quad,
                brush: apply_layer_to_brush(brush, layer),
                shape: None,
                stroke: None,
                arc: Some(arc.scaled_about(arc_center, scale)),
                clip,
                blend_mode: blend_mode.unwrap_or(BlendMode::SrcOver),
                motion_context_animated,
            });
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
                image,
                alpha: (alpha * layer.alpha).clamp(0.0, 1.0),
                color_filter: compose_color_filters(color_filter, layer.color_filter),
                sampling,
                clip,
                src_rect,
                blend_mode: blend_mode.unwrap_or(BlendMode::SrcOver),
                motion_context_animated,
            });
        }
        DrawPrimitive::Shadow(shadow_primitive) => {
            sink.push_shadow(shadow_primitive, layer_bounds, layer, clip);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranpose_ui_graphics::{Brush, Color, CornerRadii};

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

    use cranpose_ui_graphics::{Stroke, StrokeCap, StrokeJoin};
    use std::f32::consts::FRAC_PI_2;

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
}
