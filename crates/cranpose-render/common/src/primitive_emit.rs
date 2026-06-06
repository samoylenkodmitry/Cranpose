use cranpose_ui_graphics::{
    BlendMode, Brush, ColorFilter, DrawPrimitive, GraphicsLayer, ImageBitmap, ImageSampling, Rect,
    RoundedCornerShape, ShadowPrimitive,
};

use crate::graph::quad_bounds;
use crate::layer_transform::{
    apply_layer_affine_to_rect, apply_layer_to_quad, apply_layer_to_rect, layer_uniform_scale,
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
    pub clip: Option<Rect>,
    pub blend_mode: BlendMode,
    pub motion_context_animated: bool,
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
                motion_context_animated,
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
}
