use crate::{Brush, Color, ColorFilter, CornerRadii, LayerShape, Point, Rect};
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
}
