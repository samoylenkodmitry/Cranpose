use std::hash::{Hash, Hasher};

use crate::{
    Brush, Color, ColorFilter, CornerRadii, DrawPrimitive, FxHasher, ImageBitmap, LayerShape,
    Point, Rect, RenderEffect, RuntimeShader, ShadowPrimitive, Stroke,
};

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

impl RenderHash for Stroke {
    fn render_hash(&self) -> u64 {
        finish_hash(|state| hash_stroke(*self, state))
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

fn finish_hash(write: impl FnOnce(&mut FxHasher)) -> u64 {
    let mut hasher = FxHasher::default();
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

/// Every stroke field feeds the hash: layer/scene-range surface caches key off
/// content hashes, so a width/cap/join change that did not move the hash would
/// replay a stale cached surface.
fn hash_stroke<H: Hasher>(stroke: Stroke, state: &mut H) {
    hash_f32_bits(stroke.width, state);
    stroke.cap.hash(state);
    stroke.join.hash(state);
}

fn hash_optional_stroke<H: Hasher>(stroke: Option<Stroke>, state: &mut H) {
    match stroke {
        Some(stroke) => {
            1u8.hash(state);
            hash_stroke(stroke, state);
        }
        None => 0u8.hash(state),
    }
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
    shader.source_hash().hash(state);
    shader.overrides_hash().hash(state);
    hash_f32_bits(shader.input_padding(), state);
    hash_f32_bits(shader.output_padding(), state);
    for rect in [shader.output_support(), shader.sample_domain()] {
        match rect {
            Some(rect) => {
                1u8.hash(state);
                hash_f32_bits(rect.x, state);
                hash_f32_bits(rect.y, state);
                hash_f32_bits(rect.width, state);
                hash_f32_bits(rect.height, state);
            }
            None => 0u8.hash(state),
        }
    }
    shader.hash_substrates(state);
    shader.uniforms().len().hash(state);
    for uniform in shader.uniforms() {
        hash_f32_bits(*uniform, state);
    }
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
        DrawPrimitive::Rect {
            rect,
            brush,
            stroke,
        } => {
            2u8.hash(state);
            hash_rect(*rect, state);
            hash_brush(brush, state);
            hash_optional_stroke(*stroke, state);
        }
        DrawPrimitive::RoundRect {
            rect,
            brush,
            radii,
            stroke,
        } => {
            3u8.hash(state);
            hash_rect(*rect, state);
            hash_brush(brush, state);
            hash_corner_radii(*radii, state);
            hash_optional_stroke(*stroke, state);
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
            6u8.hash(state);
            hash_rect(*rect, state);
            hash_brush(brush, state);
            hash_point(*center, state);
            hash_f32_bits(*radius, state);
            hash_f32_bits(*start_angle, state);
            hash_f32_bits(*sweep_angle, state);
            hash_optional_stroke(*stroke, state);
            hash_f32_bits(*inner_radius, state);
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
        DrawPrimitive::Text(text) => {
            7u8.hash(state);
            hash_rect(text.rect, state);
            text.text.hash(state);
            hash_text_style(&text.style, state);
            hash_color(text.color, state);
        }
        DrawPrimitive::Shadow(shadow) => {
            5u8.hash(state);
            hash_shadow_primitive(shadow, state);
        }
    }
}

fn hash_text_style<H: Hasher>(style: &crate::DrawTextStyle, state: &mut H) {
    style.font_family.hash(state);
    hash_f32_bits(style.font_size, state);
    style.font_weight.hash(state);
    style.font_style.hash(state);
    hash_f32_bits(style.letter_spacing, state);
    match style.line_height {
        Some(line_height) => {
            1u8.hash(state);
            hash_f32_bits(line_height, state);
        }
        None => 0u8.hash(state),
    }
    style.align.hash(state);
    style.vertical_align.hash(state);
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
#[path = "tests/render_hash_tests.rs"]
mod tests;
