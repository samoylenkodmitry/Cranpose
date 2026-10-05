//! Elevation shadows as Jetpack Compose draws them: Android's renderer and
//! Compose Multiplatform's both light a layer from one point above the window
//! and hand the caster to Skia's `SkShadowUtils`, which draws an ambient and a
//! spot shadow. The constants below are theirs.

use cranpose_ui_graphics::{Color, CornerRadii, GraphicsLayer, Rect, RoundedCornerShape};

use crate::layer_transform::layer_uniform_scale;

const MIN_LAYER_SHADOW_SCALE: f32 = 0.1;
/// The share of a shadow color's alpha each pass keeps: Android's
/// `ambientShadowAlpha` and `spotShadowAlpha` theme values, which Compose
/// Multiplatform copies.
const AMBIENT_SHADOW_ALPHA: f32 = 0.039;
const SPOT_SHADOW_ALPHA: f32 = 0.19;
/// Where the light sits, in dp: `light_y`, `light_z` and `light_radius`.
const LIGHT_Y: f32 = 0.0;
const LIGHT_Z: f32 = 500.0;
const LIGHT_RADIUS: f32 = 800.0;
/// The window size, in dp, at which the light stands at `LIGHT_Z`; larger
/// windows raise it, smaller ones lower it, against shadow distortion.
const LIGHT_Z_REFERENCE: f32 = 450.0;
/// Skia's penumbra rises from its outer edge as `exp(-4 (1 - d)^2)` over its
/// width: half strength at 0.594 of the way in, 10 to 90% over 0.585 of it.
/// A Gaussian blur centred at that half-strength line with a radius (twice
/// the sigma) of 0.456 widths draws the same ramp.
const PENUMBRA_MIDPOINT: f32 = 0.594;
const PENUMBRA_BLUR: f32 = 0.456;

/// The light elevation shadows are cast by, in the units and coordinates the
/// shadow is collected in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowLight {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub radius: f32,
    /// Device pixels per unit: Skia scales the ambient penumbra by the
    /// caster's height in pixels.
    pub pixels_per_unit: f32,
}

impl ShadowLight {
    /// The light over a window `width × height` units in size whose origin
    /// is the collection origin, where one dp is `units_per_dp` units. Matches
    /// Kotlin: `ThreadedRenderer.setLightCenter` on Android and its copy in
    /// Compose Multiplatform's `GraphicsLayerOwnerLayer.skiko.kt`.
    pub fn for_window(width: f32, height: f32, units_per_dp: f32, pixels_per_unit: f32) -> Self {
        let units_per_dp = units_per_dp.max(f32::EPSILON);
        let window_dp = width.min(height).max(0.0) / units_per_dp;
        let adjustment = (window_dp / LIGHT_Z_REFERENCE + 2.0) / 3.0;
        Self {
            x: width / 2.0,
            y: LIGHT_Y * units_per_dp,
            z: LIGHT_Z * units_per_dp * adjustment,
            radius: LIGHT_RADIUS * units_per_dp,
            pixels_per_unit,
        }
    }

    /// The same light seen from a space whose origin sits at `origin` in
    /// this one and whose units are `scale` of this one's.
    pub fn in_space(self, origin_x: f32, origin_y: f32, scale: f32) -> Self {
        let scale = scale.max(MIN_LAYER_SHADOW_SCALE);
        Self {
            x: (self.x - origin_x) / scale,
            y: (self.y - origin_y) / scale,
            z: self.z / scale,
            radius: self.radius / scale,
            pixels_per_unit: self.pixels_per_unit * scale,
        }
    }
}

/// One blurred pass of a shadow: the caster's shape, grown by
/// `corner_outset` after its corners scale by `corner_scale`, drawn over
/// `rect` at `alpha` and blurred with `blur_radius`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerShadowPass {
    pub rect: Rect,
    pub corner_scale: f32,
    pub corner_outset: f32,
    pub blur_radius: f32,
    pub alpha: f32,
}

impl LayerShadowPass {
    /// The caster's corners, a rectangle's as zero, as this pass draws them.
    pub fn corners(&self, caster: Option<RoundedCornerShape>) -> RoundedCornerShape {
        let radii = caster.map_or(CornerRadii::uniform(0.0), |shape| shape.radii());
        let grow = |radius: f32| (radius * self.corner_scale + self.corner_outset).max(0.0);
        RoundedCornerShape::with_radii(CornerRadii {
            top_left: grow(radii.top_left),
            top_right: grow(radii.top_right),
            bottom_right: grow(radii.bottom_right),
            bottom_left: grow(radii.bottom_left),
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LayerShadowGeometry {
    pub ambient: Option<LayerShadowPass>,
    pub spot: Option<LayerShadowPass>,
}

impl LayerShadowGeometry {
    /// Each pass with the color it draws: `layer`'s ambient or spot shadow
    /// color at the pass's alpha, ambient first.
    pub fn passes(&self, layer: &GraphicsLayer) -> impl Iterator<Item = (LayerShadowPass, Color)> {
        let tinted = |color: Color, pass: LayerShadowPass| {
            (pass, Color(color.r(), color.g(), color.b(), pass.alpha))
        };
        let ambient = self
            .ambient
            .map(|pass| tinted(layer.ambient_shadow_color, pass));
        let spot = self.spot.map(|pass| tinted(layer.spot_shadow_color, pass));
        ambient.into_iter().chain(spot)
    }
}

fn outset(rect: Rect, amount: f32) -> Rect {
    Rect {
        x: rect.x - amount,
        y: rect.y - amount,
        width: rect.width + amount * 2.0,
        height: rect.height + amount * 2.0,
    }
}

/// The ambient and spot passes of `layer`'s shadow over `transformed_bounds`,
/// lit by `light`. Matches Skia: `GrSurfaceDrawContext::drawFastShadow` and
/// `SkDrawShadowMetrics`.
pub fn layer_shadow_geometry(
    layer: &GraphicsLayer,
    transformed_bounds: Rect,
    light: ShadowLight,
) -> LayerShadowGeometry {
    if layer.shadow_elevation <= 0.0 {
        return LayerShadowGeometry::default();
    }

    let scale = layer_uniform_scale(layer).max(MIN_LAYER_SHADOW_SCALE);
    let height = layer.shadow_elevation * scale;
    let caster_alpha = layer.alpha.clamp(0.0, 1.0);

    // The ambient shadow reaches half the height past the caster, its
    // penumbra widened by the height in pixels.
    let ambient_reach = height * 0.5;
    let ambient_width = ambient_reach * (1.0 + height * light.pixels_per_unit / 128.0);
    let ambient_edge = ambient_reach - PENUMBRA_MIDPOINT * ambient_width;
    let ambient_alpha =
        (layer.ambient_shadow_color.a() * AMBIENT_SHADOW_ALPHA * caster_alpha).clamp(0.0, 1.0);
    let ambient = (ambient_alpha > f32::EPSILON).then_some(LayerShadowPass {
        rect: outset(transformed_bounds, ambient_edge),
        corner_scale: 1.0,
        corner_outset: ambient_edge,
        blur_radius: PENUMBRA_BLUR * ambient_width,
        alpha: ambient_alpha,
    });

    // The spot shadow is the caster projected from the light onto the
    // plane below it: every point moves away from the light by the height's
    // share of the light's, and the penumbra is the light's radius times it.
    let depth = light.z - height;
    let z_ratio = if depth > f32::EPSILON {
        (height / depth).clamp(0.0, 0.95)
    } else {
        0.95
    };
    let spot_scale = 1.0 + z_ratio;
    let spot_width = light.radius * z_ratio;
    let projected = Rect {
        x: transformed_bounds.x + z_ratio * (transformed_bounds.x - light.x),
        y: transformed_bounds.y + z_ratio * (transformed_bounds.y - light.y),
        width: transformed_bounds.width * spot_scale,
        height: transformed_bounds.height * spot_scale,
    };
    let spot_edge = (0.5 - PENUMBRA_MIDPOINT) * spot_width;
    let spot_alpha =
        (layer.spot_shadow_color.a() * SPOT_SHADOW_ALPHA * caster_alpha).clamp(0.0, 1.0);
    let spot = (spot_alpha > f32::EPSILON).then_some(LayerShadowPass {
        rect: outset(projected, spot_edge),
        corner_scale: spot_scale,
        corner_outset: spot_edge,
        blur_radius: PENUMBRA_BLUR * spot_width,
        alpha: spot_alpha,
    });

    LayerShadowGeometry { ambient, spot }
}

#[cfg(test)]
#[path = "tests/layer_shadow_tests.rs"]
mod tests;
