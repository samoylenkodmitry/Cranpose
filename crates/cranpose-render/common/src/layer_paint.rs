//! The alpha and color filter that a layer drawn in place lays over the
//! colors of its content.

use std::hash::Hasher;

use cranpose_ui::Brush;
use cranpose_ui_graphics::{Color, ColorFilter, FxHasher, GraphicsLayer, RenderHash};

use crate::style_shared::apply_paint_to_color;

/// The alpha and color filter of a layer drawn in place. Every color of the
/// layer's content takes them, as [`apply_layer_to_color`] applies them. A
/// text resolves the colors of its glyphs from its style and spans first.
/// Each resolved color then takes this paint.
///
/// [`apply_layer_to_color`]: crate::style_shared::apply_layer_to_color
#[derive(Clone, Debug, PartialEq)]
pub struct LayerPaint {
    alpha: f32,
    color_filter: Option<ColorFilter>,
}

impl LayerPaint {
    /// The paint that leaves every color as it is.
    pub const NONE: Self = Self {
        alpha: 1.0,
        color_filter: None,
    };

    /// The alpha and color filter of `layer`.
    pub fn of(layer: &GraphicsLayer) -> Self {
        Self {
            alpha: layer.alpha,
            color_filter: layer.color_filter.clone(),
        }
    }

    // Whether the paint changes a color.
    pub(crate) fn paints(&self) -> bool {
        self.alpha != 1.0 || self.color_filter.is_some()
    }

    // `color` under the paint: snapped to eight bits, then multiplied by the
    // alpha and filtered. A paint that changes nothing leaves `color` as it
    // is.
    pub(crate) fn color(&self, color: Color) -> Color {
        if self.paints() {
            apply_paint_to_color(color, self.alpha, self.color_filter.as_ref())
        } else {
            color
        }
    }

    /// `brush` with each of its colors under the paint.
    pub fn brush(&self, brush: Brush) -> Brush {
        if self.paints() {
            brush.map_colors(|color| self.color(color))
        } else {
            brush
        }
    }
}

impl RenderHash for LayerPaint {
    fn render_hash(&self) -> u64 {
        let mut state = FxHasher::default();
        state.write_u32(self.alpha.to_bits());
        state.write_u64(
            self.color_filter
                .as_ref()
                .map_or(0, RenderHash::render_hash),
        );
        state.finish()
    }
}
