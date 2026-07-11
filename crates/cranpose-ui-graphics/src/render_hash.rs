use crate::{
    Brush, Color, ColorFilter, CornerRadii, DrawPrimitive, ImageBitmap, LayerShape, Point, Rect,
    RenderEffect, RuntimeShader, ShadowPrimitive,
};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub trait RenderHash {
    fn render_hash(&self) -> u64;
}

impl RenderHash for Color {
    fn render_hash(&self) -> u64 {
        finish_hash(|state| hash_color(*self, state))
    }
}

impl RenderHash for Point {
    fn render_hash(&self) -> u64 {
        finish_hash(|state| hash_point(*self, state))
    }
}

impl RenderHash for Rect {
    fn render_hash(&self) -> u64 {
        finish_hash(|state| hash_rect(*self, state))
    }
}

impl RenderHash for CornerRadii {
    fn render_hash(&self) -> u64 {
        finish_hash(|state| hash_corner_radii(*self, state))
    }
}

impl RenderHash for LayerShape {
    fn render_hash(&self) -> u64 {
        finish_hash(|state| hash_layer_shape(*self, state))
    }
}

impl RenderHash for Brush {
    fn render_hash(&self) -> u64 {
        finish_hash(|state| hash_brush(self, state))
    }
}

impl RenderHash for ColorFilter {
    fn render_hash(&self) -> u64 {
        finish_hash(|state| hash_color_filter(*self, state))
    }
}

impl RenderHash for ImageBitmap {
    fn render_hash(&self) -> u64 {
        finish_hash(|state| self.id().hash(state))
    }
}

impl RenderHash for RuntimeShader {
    fn render_hash(&self) -> u64 {
        finish_hash(|state| hash_runtime_shader(self, state))
    }
}

impl RenderHash for RenderEffect {
    fn render_hash(&self) -> u64 {
        finish_hash(|state| hash_render_effect(self, state))
    }
}

impl RenderHash for DrawPrimitive {
    fn render_hash(&self) -> u64 {
        finish_hash(|state| hash_draw_primitive(self, state))
    }
}

impl RenderHash for ShadowPrimitive {
    fn render_hash(&self) -> u64 {
        finish_hash(|state| hash_shadow_primitive(self, state))
    }
}

fn finish_hash(write: impl FnOnce(&mut DefaultHasher)) -> u64 {
    let mut hasher = DefaultHasher::new();
    write(&mut hasher);
    hasher.finish()
}

fn hash_f32_bits<H: Hasher>(value: f32, state: &mut H) {
    value.to_bits().hash(state);
}

fn hash_color<H: Hasher>(color: Color, state: &mut H) {
    hash_f32_bits(color.0, state);
    hash_f32_bits(color.1, state);
    hash_f32_bits(color.2, state);
    hash_f32_bits(color.3, state);
}

fn hash_point<H: Hasher>(point: Point, state: &mut H) {
    hash_f32_bits(point.x, state);
    hash_f32_bits(point.y, state);
}

fn hash_rect<H: Hasher>(rect: Rect, state: &mut H) {
    hash_f32_bits(rect.x, state);
    hash_f32_bits(rect.y, state);
    hash_f32_bits(rect.width, state);
    hash_f32_bits(rect.height, state);
}

fn hash_corner_radii<H: Hasher>(radii: CornerRadii, state: &mut H) {
    hash_f32_bits(radii.top_left, state);
    hash_f32_bits(radii.top_right, state);
    hash_f32_bits(radii.bottom_right, state);
    hash_f32_bits(radii.bottom_left, state);
}

fn hash_layer_shape<H: Hasher>(shape: LayerShape, state: &mut H) {
    match shape {
        LayerShape::Rectangle => 0u8.hash(state),
        LayerShape::Rounded(shape) => {
            1u8.hash(state);
            hash_corner_radii(shape.radii(), state);
        }
    }
}

fn hash_brush<H: Hasher>(brush: &Brush, state: &mut H) {
    match brush {
        Brush::Solid(color) => {
            0u8.hash(state);
            hash_color(*color, state);
        }
        Brush::LinearGradient {
            colors,
            stops,
            start,
            end,
            tile_mode,
        } => {
            1u8.hash(state);
            hash_color_slice(colors, state);
            hash_optional_stop_list(stops.as_deref(), state);
            hash_point(*start, state);
            hash_point(*end, state);
            tile_mode.hash(state);
        }
        Brush::RadialGradient {
            colors,
            stops,
            center,
            radius,
            tile_mode,
        } => {
            2u8.hash(state);
            hash_color_slice(colors, state);
            hash_optional_stop_list(stops.as_deref(), state);
            hash_point(*center, state);
            hash_f32_bits(*radius, state);
            tile_mode.hash(state);
        }
        Brush::SweepGradient {
            colors,
            stops,
            center,
        } => {
            3u8.hash(state);
            hash_color_slice(colors, state);
            hash_optional_stop_list(stops.as_deref(), state);
            hash_point(*center, state);
        }
    }
}

fn hash_color_slice<H: Hasher>(colors: &[Color], state: &mut H) {
    colors.len().hash(state);
    for color in colors {
        hash_color(*color, state);
    }
}

fn hash_optional_stop_list<H: Hasher>(stops: Option<&[f32]>, state: &mut H) {
    match stops {
        Some(stops) => {
            1u8.hash(state);
            stops.len().hash(state);
            for stop in stops {
                hash_f32_bits(*stop, state);
            }
        }
        None => 0u8.hash(state),
    }
}

fn hash_color_filter<H: Hasher>(filter: ColorFilter, state: &mut H) {
    match filter {
        ColorFilter::Tint(color) => {
            0u8.hash(state);
            hash_color(color, state);
        }
        ColorFilter::Modulate(color) => {
            1u8.hash(state);
            hash_color(color, state);
        }
        ColorFilter::Matrix(matrix) => {
            2u8.hash(state);
            for value in matrix {
                hash_f32_bits(value, state);
            }
        }
    }
}

fn hash_runtime_shader<H: Hasher>(shader: &RuntimeShader, state: &mut H) {
    // Only hash the source, not the uniforms. Uniforms change every frame for
    // animated shaders (time, position, etc.) and including them would produce
    // a new effect_hash every frame, filling the layer surface cache with
    // stale entries. The pipeline cache already deduplicates by source hash,
    // and stable_id in the cache key distinguishes different nodes.
    shader.source_hash().hash(state);
    hash_f32_bits(shader.input_padding(), state);
}

fn hash_render_effect<H: Hasher>(effect: &RenderEffect, state: &mut H) {
    match effect {
        RenderEffect::Blur {
            radius_x,
            radius_y,
            edge_treatment,
        } => {
            0u8.hash(state);
            hash_f32_bits(*radius_x, state);
            hash_f32_bits(*radius_y, state);
            edge_treatment.hash(state);
        }
        RenderEffect::Offset { offset_x, offset_y } => {
            1u8.hash(state);
            hash_f32_bits(*offset_x, state);
            hash_f32_bits(*offset_y, state);
        }
        RenderEffect::Shader { shader } => {
            2u8.hash(state);
            hash_runtime_shader(shader, state);
        }
        RenderEffect::Chain { first, second } => {
            3u8.hash(state);
            hash_render_effect(first, state);
            hash_render_effect(second, state);
        }
    }
}

fn hash_draw_primitive<H: Hasher>(primitive: &DrawPrimitive, state: &mut H) {
    match primitive {
        DrawPrimitive::Content => {
            0u8.hash(state);
        }
        DrawPrimitive::Blend {
            primitive,
            blend_mode,
        } => {
            1u8.hash(state);
            blend_mode.hash(state);
            hash_draw_primitive(primitive, state);
        }
        DrawPrimitive::Rect { rect, brush } => {
            2u8.hash(state);
            hash_rect(*rect, state);
            hash_brush(brush, state);
        }
        DrawPrimitive::RoundRect { rect, brush, radii } => {
            3u8.hash(state);
            hash_rect(*rect, state);
            hash_brush(brush, state);
            hash_corner_radii(*radii, state);
        }
        DrawPrimitive::Image {
            rect,
            image,
            alpha,
            color_filter,
            sampling,
            src_rect,
        } => {
            4u8.hash(state);
            hash_rect(*rect, state);
            image.id().hash(state);
            hash_f32_bits(*alpha, state);
            sampling.hash(state);
            match color_filter {
                Some(filter) => {
                    1u8.hash(state);
                    hash_color_filter(*filter, state);
                }
                None => 0u8.hash(state),
            }
            match src_rect {
                Some(rect) => {
                    1u8.hash(state);
                    hash_rect(*rect, state);
                }
                None => 0u8.hash(state),
            }
        }
        DrawPrimitive::Shadow(shadow) => {
            5u8.hash(state);
            hash_shadow_primitive(shadow, state);
        }
    }
}

fn hash_shadow_primitive<H: Hasher>(shadow: &ShadowPrimitive, state: &mut H) {
    match shadow {
        ShadowPrimitive::Drop {
            shape,
            cutout,
            blur_radius,
            blend_mode,
        } => {
            0u8.hash(state);
            hash_draw_primitive(shape, state);
            match cutout {
                Some(cutout) => {
                    1u8.hash(state);
                    hash_draw_primitive(cutout, state);
                }
                None => 0u8.hash(state),
            }
            hash_f32_bits(*blur_radius, state);
            blend_mode.hash(state);
        }
        ShadowPrimitive::Inner {
            fill,
            cutout,
            blur_radius,
            blend_mode,
            clip_rect,
        } => {
            1u8.hash(state);
            hash_draw_primitive(fill, state);
            hash_draw_primitive(cutout, state);
            hash_f32_bits(*blur_radius, state);
            blend_mode.hash(state);
            hash_rect(*clip_rect, state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render_effect::TileMode;

    #[test]
    fn color_render_hash_changes_with_channels() {
        assert_ne!(
            Color(1.0, 0.0, 0.0, 1.0).render_hash(),
            Color(0.0, 1.0, 0.0, 1.0).render_hash()
        );
    }

    #[test]
    fn point_rect_and_corner_radii_render_hash_changes_with_geometry() {
        assert_ne!(
            Point::new(1.0, 2.0).render_hash(),
            Point::new(2.0, 1.0).render_hash()
        );
        assert_ne!(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 20.0,
            }
            .render_hash(),
            Rect {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 10.0,
            }
            .render_hash()
        );
        assert_ne!(
            CornerRadii::uniform(4.0).render_hash(),
            CornerRadii::uniform(6.0).render_hash()
        );
    }

    #[test]
    fn layer_shape_render_hash_tracks_shape_kind_and_radii() {
        assert_ne!(
            LayerShape::Rectangle.render_hash(),
            LayerShape::Rounded(crate::RoundedCornerShape::uniform(8.0)).render_hash()
        );
        assert_ne!(
            LayerShape::Rounded(crate::RoundedCornerShape::uniform(4.0)).render_hash(),
            LayerShape::Rounded(crate::RoundedCornerShape::uniform(8.0)).render_hash()
        );
    }

    #[test]
    fn brush_render_hash_tracks_gradient_structure() {
        let base = Brush::linear_gradient_with_tile_mode(
            vec![Color::RED, Color::BLUE],
            Point::new(0.0, 0.0),
            Point::new(10.0, 10.0),
            TileMode::Clamp,
        );
        let shifted = Brush::linear_gradient_with_tile_mode(
            vec![Color::RED, Color::BLUE],
            Point::new(1.0, 0.0),
            Point::new(10.0, 10.0),
            TileMode::Clamp,
        );

        assert_ne!(base.render_hash(), shifted.render_hash());
    }

    #[test]
    fn color_filter_render_hash_tracks_variant_and_values() {
        assert_ne!(
            ColorFilter::Tint(Color::RED).render_hash(),
            ColorFilter::Modulate(Color::RED).render_hash()
        );
        assert_ne!(
            ColorFilter::Matrix([1.0; 20]).render_hash(),
            ColorFilter::Matrix([0.0; 20]).render_hash()
        );
    }

    #[test]
    fn render_effect_render_hash_tracks_variant_parameters() {
        assert_ne!(
            RenderEffect::blur(4.0).render_hash(),
            RenderEffect::blur(6.0).render_hash()
        );
        assert_ne!(
            RenderEffect::offset(2.0, 1.0).render_hash(),
            RenderEffect::offset(1.0, 2.0).render_hash()
        );
    }

    #[test]
    fn runtime_shader_render_hash_ignores_uniforms() {
        let mut base = RuntimeShader::new("// hash");
        base.set_float(0, 1.0);
        let mut changed = base.clone();
        changed.set_float(0, 2.0);
        assert_eq!(
            base.render_hash(),
            changed.render_hash(),
            "render_hash must depend only on source, not uniforms — \
             animated uniforms (time, position) would otherwise produce a \
             new effect_hash every frame, filling the layer cache with stale textures"
        );
    }

    #[test]
    fn runtime_shader_render_hash_tracks_source() {
        let a = RuntimeShader::new("// shader A");
        let b = RuntimeShader::new("// shader B");
        assert_ne!(a.render_hash(), b.render_hash());
    }

    #[test]
    fn draw_primitive_render_hash_tracks_nested_structure() {
        let base = DrawPrimitive::Blend {
            primitive: Box::new(DrawPrimitive::Rect {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 12.0,
                    height: 8.0,
                },
                brush: Brush::solid(Color::WHITE),
            }),
            blend_mode: crate::BlendMode::SrcOver,
        };
        let changed = DrawPrimitive::Blend {
            primitive: Box::new(DrawPrimitive::Rect {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 12.0,
                    height: 8.0,
                },
                brush: Brush::solid(Color::BLACK),
            }),
            blend_mode: crate::BlendMode::SrcOver,
        };
        assert_ne!(base.render_hash(), changed.render_hash());
    }
}
