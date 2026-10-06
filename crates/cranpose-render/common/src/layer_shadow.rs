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
/// The most an ambient shadow reaches past its caster, in pixels: Skia's
/// `kMaxAmbientRadius`.
const MAX_AMBIENT_REACH_PIXELS: f32 = 150.0;
/// Skia's falloff table: 128 texels of `exp(-4 (1 - x)^2) - 0.018`, read
/// with linear filtering at the ramp's coordinate.
const FALLOFF_TEXELS: f32 = 128.0;
const FALLOFF_FLOOR: f32 = 0.018;

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

/// One pass of a shadow as Skia's `ShadowCircularRRectOp` draws it: a round
/// rect whose penumbra rises from its outer edge, `bounds`, toward the umbra
/// `umbra_inset` in. Corners are fans of triangles whose offsets the GPU
/// interpolates, so a corner is a quarter circle where the umbra meets its
/// radius and a skewed one where the blur is wider. Every length is in the
/// space the shadow is collected in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowRRect {
    /// The penumbra's outer edge.
    pub bounds: Rect,
    /// The outer corner radii: top-left, top-right, bottom-right,
    /// bottom-left.
    pub radii: [f32; 4],
    /// How far in from `bounds` the ramp runs, at most half the smaller side.
    pub umbra_inset: f32,
    /// The ramp's coordinate at the umbra: the umbra inset over the blur.
    pub distance_correction: f32,
    /// The part an opaque caster covers, which the shadow leaves undrawn.
    pub hole: Option<ShadowHole>,
}

/// The round rect inside a shadow that it leaves undrawn: `bounds` of the
/// shadow inset by `inset`, its corners rounded at `radius`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowHole {
    pub inset: f32,
    pub radius: f32,
}

impl ShadowRRect {
    /// The shadow `outer` casts with a ramp `blur` wide: Skia's fill of a
    /// circle or round rect, or for `inset_width` within half its smaller
    /// side, its stroke, which leaves the middle to the caster.
    fn new(outer: Rect, radii: [f32; 4], blur: f32, inset_width: f32) -> Option<Self> {
        let half_side = 0.5 * outer.width.min(outer.height);
        if half_side <= 0.0 || blur <= 0.0 {
            return None;
        }
        let largest = radii.into_iter().fold(0.0, f32::max);
        let circle = (outer.width - outer.height).abs() <= f32::EPSILON * outer.width
            && largest >= half_side;
        let (umbra_inset, hole) = if circle {
            let inner = half_side - inset_width;
            let hole = (inner > 0.0).then_some(ShadowHole {
                inset: inset_width,
                radius: inner,
            });
            (half_side, hole)
        } else {
            let umbra = largest.max(blur);
            let hole = (inset_width <= half_side).then(|| ShadowHole {
                inset: inset_width.max(umbra.min(half_side)),
                radius: 0.0,
            });
            (umbra.min(half_side), hole)
        };
        Some(Self {
            bounds: outer,
            radii: if circle { [half_side; 4] } else { radii },
            umbra_inset,
            distance_correction: umbra_inset / blur,
            hole: hole.filter(|hole| hole.inset < half_side),
        })
    }

    /// The same shadow in a space whose units are `scale` of this one's.
    pub fn scaled(self, scale: f32) -> Self {
        Self {
            bounds: Rect {
                x: self.bounds.x * scale,
                y: self.bounds.y * scale,
                width: self.bounds.width * scale,
                height: self.bounds.height * scale,
            },
            radii: self.radii.map(|radius| radius * scale),
            umbra_inset: self.umbra_inset * scale,
            distance_correction: self.distance_correction,
            hole: self.hole.map(|hole| ShadowHole {
                inset: hole.inset * scale,
                radius: hole.radius * scale,
            }),
        }
    }

    /// The same shadow moved by `dx`, `dy`.
    pub fn translated(self, dx: f32, dy: f32) -> Self {
        Self {
            bounds: self.bounds.translate(dx, dy),
            ..self
        }
    }

    /// The rect the hole spans, when there is one.
    pub fn hole_rect(&self) -> Option<Rect> {
        self.hole.map(|hole| Rect {
            x: self.bounds.x + hole.inset,
            y: self.bounds.y + hole.inset,
            width: self.bounds.width - 2.0 * hole.inset,
            height: self.bounds.height - 2.0 * hole.inset,
        })
    }

    /// How much of the shadow's color the point `x`, `y` takes: the falloff
    /// at the offset Skia's mesh interpolates there, zero outside the shadow
    /// and in its hole.
    pub fn coverage(&self, x: f32, y: f32) -> f32 {
        let bounds = self.bounds;
        let right_edge = bounds.x + bounds.width;
        let bottom_edge = bounds.y + bounds.height;
        if x < bounds.x || y < bounds.y || x >= right_edge || y >= bottom_edge {
            return 0.0;
        }
        if let (Some(hole), Some(rect)) = (self.hole, self.hole_rect())
            && rounded_rect_contains(rect, hole.radius, x, y)
        {
            return 0.0;
        }
        let umbra = self.umbra_inset;
        let left = bounds.x + umbra - x;
        let right = x - (right_edge - umbra);
        let top = bounds.y + umbra - y;
        let bottom = y - (bottom_edge - umbra);
        let corner = match (left > right, top > bottom) {
            (true, true) => 0,
            (false, true) => 1,
            (false, false) => 2,
            (true, false) => 3,
        };
        let offset = fan_offset(
            left.max(right).max(0.0),
            top.max(bottom).max(0.0),
            umbra,
            self.radii[corner],
        );
        shadow_falloff(self.distance_correction * (1.0 - offset))
    }
}

fn rounded_rect_contains(rect: Rect, radius: f32, x: f32, y: f32) -> bool {
    let half_width = 0.5 * rect.width;
    let half_height = 0.5 * rect.height;
    let dx = (x - rect.x - half_width).abs() - (half_width - radius);
    let dy = (y - rect.y - half_height).abs() - (half_height - radius);
    let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt();
    outside + dx.max(dy).min(0.0) - radius < 0.0
}

/// The length of the offset Skia's corner fan interpolates at `along` and
/// `across`, a point's distances out from the umbra's corner toward the two
/// edges, for an umbra inset `umbra` and a corner radius `radius`: 0 at the
/// umbra and 1 on the outer edge. The fan's triangles meet the edge at the
/// umbra line, at `umbra - radius` along it and at the corner, whose offsets
/// Skia skews when the umbra is deeper than the radius.
pub fn fan_offset(along: f32, across: f32, umbra: f32, radius: f32) -> f32 {
    let (a, b) = if along >= across {
        (along, across)
    } else {
        (across, along)
    };
    if a <= 0.0 || umbra <= 0.0 {
        return 0.0;
    }
    let length = (2.0 * (radius * radius + umbra * umbra)).sqrt();
    let outer = [(radius - umbra) / length, (-radius - umbra) / length];
    let straight = umbra - radius;
    let [x, y] = if b * umbra <= a * straight {
        let toward_curve = if straight > 0.0 { b / straight } else { 0.0 };
        let toward_edge = a / umbra - toward_curve;
        [
            toward_curve * outer[0],
            -toward_edge + toward_curve * outer[1],
        ]
    } else {
        let diagonal = umbra / (std::f32::consts::SQRT_2 * (radius - umbra) - radius);
        let toward_curve = (a - b) / radius;
        let toward_corner = a / umbra - toward_curve;
        [
            toward_curve * outer[0] + toward_corner * diagonal,
            toward_curve * outer[1] + toward_corner * diagonal,
        ]
    };
    (x * x + y * y).sqrt()
}

/// Skia's penumbra falloff at `x`, the ramp's coordinate: 0 on the outer
/// edge, 1 where the shadow is whole, read as its table is.
pub fn shadow_falloff(x: f32) -> f32 {
    let texel = ((x * FALLOFF_TEXELS - 0.5) / (FALLOFF_TEXELS - 1.0)).clamp(0.0, 1.0);
    let distance = 1.0 - texel;
    ((-4.0 * distance * distance).exp() - FALLOFF_FLOOR).max(0.0)
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LayerShadowGeometry {
    pub ambient: Option<(ShadowRRect, Color)>,
    pub spot: Option<(ShadowRRect, Color)>,
}

impl LayerShadowGeometry {
    /// Each pass with the color it draws, ambient first.
    pub fn passes(&self) -> impl Iterator<Item = (ShadowRRect, Color)> {
        self.ambient.into_iter().chain(self.spot)
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

fn corner_radii(radii: CornerRadii) -> [f32; 4] {
    [
        radii.top_left,
        radii.top_right,
        radii.bottom_right,
        radii.bottom_left,
    ]
}

/// `color` at the alpha Android's renderer gives a pass: the theme's
/// `share` as a byte, times the caster's alpha, times the color's alpha
/// byte, truncated to a byte (`multiplyAlpha` in
/// `ReorderBarrierDrawables.cpp`).
fn pass_color(color: Color, share: f32, caster_alpha: f32) -> Option<Color> {
    let share_byte = (share * 255.0).floor();
    let color_byte = (color.a().clamp(0.0, 1.0) * 255.0).round();
    let byte = (color_byte * share_byte / 255.0 * caster_alpha).floor();
    (byte > 0.0).then(|| Color(color.r(), color.g(), color.b(), byte / 255.0))
}

/// Whether the caster covers the middle of its shadow: an opaque one that
/// keeps its rect upright. Skia fills the shadow of any other.
fn caster_occludes(layer: &GraphicsLayer) -> bool {
    layer.alpha >= 1.0
        && layer.rotation_x.abs() <= f32::EPSILON
        && layer.rotation_y.abs() <= f32::EPSILON
        && layer.rotation_z.abs() <= f32::EPSILON
}

/// How far the spot shadow strays from its caster at a corner: Skia's
/// `maxOffset`, the manhattan distance for a rect and for a round rect the
/// corner centers' distance plus the radii's difference.
fn spot_stray(caster: Rect, spot: Rect, caster_radius: f32, spot_radius: f32) -> f32 {
    if caster_radius <= 0.0 {
        return (spot.x - caster.x)
            .abs()
            .max((spot.y - caster.y).abs())
            .max((spot.x + spot.width - caster.x - caster.width).abs())
            .max((spot.y + spot.height - caster.y - caster.height).abs());
    }
    let grown = spot_radius - caster_radius;
    let upper = [spot.x - caster.x + grown, spot.y - caster.y + grown];
    let lower = [
        spot.x + spot.width - caster.x - caster.width - grown,
        spot.y + spot.height - caster.y - caster.height - grown,
    ];
    let length = |[x, y]: [f32; 2]| x * x + y * y;
    length(upper).max(length(lower)).sqrt() + grown
}

/// The ambient and spot passes of `layer`'s shadow over `transformed_bounds`
/// with the corners `caster` rounds it at, lit by `light`. Matches Skia's
/// `SurfaceDrawContext::drawFastShadow` and `SkDrawShadowMetrics`.
pub fn layer_shadow_geometry(
    layer: &GraphicsLayer,
    transformed_bounds: Rect,
    caster: Option<RoundedCornerShape>,
    light: ShadowLight,
) -> LayerShadowGeometry {
    if layer.shadow_elevation <= 0.0 {
        return LayerShadowGeometry::default();
    }

    let scale = layer_uniform_scale(layer).max(MIN_LAYER_SHADOW_SCALE);
    let height = layer.shadow_elevation * scale;
    let caster_alpha = layer.alpha.clamp(0.0, 1.0);
    let radii = caster.map_or([0.0; 4], |shape| corner_radii(shape.radii()));
    let occludes = caster_occludes(layer);
    let pixels_per_unit = light.pixels_per_unit.max(f32::EPSILON);

    // The ambient shadow reaches half the height past the caster, its
    // penumbra widened by the height in pixels.
    let reach = (height * 0.5).min(MAX_AMBIENT_REACH_PIXELS / pixels_per_unit);
    let ambient_outer = outset(transformed_bounds, reach);
    let ambient = pass_color(
        layer.ambient_shadow_color,
        AMBIENT_SHADOW_ALPHA,
        caster_alpha,
    )
    .and_then(|color| {
        let blur = reach * (1.0 + height * pixels_per_unit / 128.0);
        let inset = if occludes { reach } else { ambient_outer.width };
        ShadowRRect::new(ambient_outer, radii.map(|r| r + reach), blur, inset)
            .map(|shadow| (shadow, color))
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
    let spot_blur = light.radius * z_ratio;
    let projected = Rect {
        x: transformed_bounds.x + z_ratio * (transformed_bounds.x - light.x),
        y: transformed_bounds.y + z_ratio * (transformed_bounds.y - light.y),
        width: transformed_bounds.width * spot_scale,
        height: transformed_bounds.height * spot_scale,
    };
    let spot =
        pass_color(layer.spot_shadow_color, SPOT_SHADOW_ALPHA, caster_alpha).and_then(|color| {
            let caster_radius = radii.into_iter().fold(0.0, f32::max);
            let inset = spot_blur
                + if occludes {
                    spot_blur.max(spot_stray(
                        transformed_bounds,
                        projected,
                        caster_radius,
                        caster_radius * spot_scale,
                    ))
                } else {
                    projected.width
                };
            ShadowRRect::new(
                outset(projected, spot_blur),
                radii.map(|r| r * spot_scale + spot_blur),
                2.0 * spot_blur,
                inset,
            )
            .map(|shadow| (shadow, color))
        });

    LayerShadowGeometry { ambient, spot }
}

#[cfg(test)]
#[path = "tests/layer_shadow_tests.rs"]
mod tests;
