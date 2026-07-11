//! The Liquid Glass material: a backdrop lens (blur → refraction/vibrancy
//! shader) applied to a composable's own bounds through
//! [`LiquidModifierExt::glass_effect`] — the analogue of SwiftUI's
//! `.glassEffect(_:in:)`.

use crate::theme::LiquidColors;
use cranpose_ui::current_density;
use cranpose_ui::Modifier;
use cranpose_ui_graphics::{
    Color, GraphicsLayer, LayerShape, RenderEffect, RoundedCornerShape, RuntimeShader,
    LIQUID_GLASS_WGSL,
};
use std::rc::Rc;

/// Corner radius large enough that [`cranpose_ui_graphics::CornerRadii::resolve`]
/// clamps it to half the shape's size — i.e. a capsule.
const CAPSULE_CLIP_RADIUS: f32 = 1.0e6;

/// Shader sentinel requesting the capsule radius (resolved against the node's
/// size at render time; see `liquid_glass.wgsl` cover mode).
const CAPSULE_SHADER_RADIUS: f32 = -1.0;

/// The shape of a glass element (also its clip and shadow shape).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum LiquidShape {
    /// A pill: corner radius follows the smaller half-extent.
    #[default]
    Capsule,
    /// Rounded rectangle with the radius in dp.
    RoundedRect(f32),
    /// A circle (capsule of a square node).
    Circle,
}

impl LiquidShape {
    /// The clip shape handed to the graphics layer.
    pub fn clip_shape(&self) -> RoundedCornerShape {
        match self {
            LiquidShape::Capsule | LiquidShape::Circle => {
                RoundedCornerShape::uniform(CAPSULE_CLIP_RADIUS)
            }
            LiquidShape::RoundedRect(radius) => RoundedCornerShape::uniform(*radius),
        }
    }

    /// The layer shape (clip + shadow geometry).
    pub fn layer_shape(&self) -> LayerShape {
        LayerShape::Rounded(self.clip_shape())
    }

    /// The radius uniform for the lens shader, in px (negative = capsule).
    fn shader_radius_px(&self, density: f32) -> f32 {
        match self {
            LiquidShape::Capsule | LiquidShape::Circle => CAPSULE_SHADER_RADIUS,
            LiquidShape::RoundedRect(radius) => radius * density,
        }
    }
}

/// Material variant, mirroring SwiftUI's `.regular` / `.clear` glass, plus
/// the interactive lens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GlassVariant {
    /// Frosted: strong backdrop blur, vibrancy boost, scheme-adaptive lift.
    #[default]
    Regular,
    /// Transparent: minimal blur, mild vibrancy — for media-rich backdrops.
    Clear,
    /// The interactive magnifying bubble (drag lenses, flying selection):
    /// no frost, no tone shift, a full-element dome and pronounced rainbow
    /// dispersion at the rim.
    Lens,
}

/// Per-frame motion inputs for an interactive glass element, read lazily at
/// scene-build time (no recomposition per frame).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct GlassDynamics {
    /// Motion tilt (finger/pointer/device motion), roughly −1..1 per axis.
    pub tilt: (f32, f32),
    /// Extra specular intensity (0 = spec default; e.g. press boost).
    pub highlight_boost: f32,
    /// Extra magnification over the variant's base (the moving lens
    /// magnifies harder than a resting one).
    pub magnify_boost: f32,
    /// Shape morph: when set, the glass geometry is these node-local rects
    /// instead of the node cover — the shapeshift channel.
    pub morph: Option<GlassMorph>,
}

/// A liquid shapeshift frame: the primary shape plus any number of nearby
/// glass shapes, ALL smooth-unioned into one field — liquid glass glues to
/// whatever glass it passes near (a growing menu necks with a neighboring
/// button, the drag lens merges with the search circle). Up to
/// [`GlassMorph::MAX_SHAPES`] extra shapes; an angular wobble makes the
/// mid-flight field bubble like a droplet. All geometry is node-local dp:
/// `(center_x, center_y, width, height, corner_radius)`; radius sentinel
/// `-1` means capsule; `-2` means SUBTRACT capsule — the shape carves a
/// smooth hole in the field (the growing menu leaves its anchor button
/// crisp on top until it swallows it).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct GlassMorph {
    /// The glass node's own size in dp. The shader receives all morph
    /// geometry in dp and derives px-per-dp from the renderer-injected node
    /// pixel rect divided by this — geometry then lands correctly at ANY
    /// render scale (live window density, robot captures at 1.0, fractional
    /// desktop scales). The authoring widget always knows its node size.
    pub node_size: (f32, f32),
    pub primary: (f32, f32, f32, f32, f32),
    /// Nearby glass shapes participating in the field.
    pub shapes: Vec<(f32, f32, f32, f32, f32)>,
    /// Smooth-union glue radius (dp): shapes within it neck together.
    pub glue: f32,
    /// Wobble amplitude (dp) and phase (radians).
    pub wobble_amplitude: f32,
    pub wobble_phase: f32,
    /// Viscous leading-edge bulge: while the shape travels or inflates, its
    /// side facing `bulge_direction` (radians, math convention) swells like a
    /// pulled droplet. Amplitude in dp, usually driven by morph velocity.
    pub bulge_amplitude: f32,
    pub bulge_direction: f32,
}

impl GlassMorph {
    /// Shader budget for extra scene shapes.
    pub const MAX_SHAPES: usize = 8;
}

/// Builder describing a glass material. Resolved against the theme at the
/// composition site, then evaluated per frame for density and dynamics.
#[derive(Clone, Debug, PartialEq)]
pub struct Glass {
    pub variant: GlassVariant,
    pub shape: LiquidShape,
    /// Tint over the refracted backdrop; defaults to the theme's glass tint.
    pub tint: Option<Color>,
    /// Backdrop blur radius in dp (defaults per variant).
    pub blur_radius: Option<f32>,
    /// Saturation boost (defaults per variant).
    pub saturation: Option<f32>,
    /// Chromatic aberration spread at the bezel.
    pub chromatic_aberration: f32,
    /// Lens strength: refraction displacement in px.
    pub displacement: f32,
    /// Bezel width in dp (the refractive rim zone).
    pub bezel_width: f32,
    /// Specular rim intensity.
    pub highlight: f32,
    /// Screen-lift override (brightening toward white; negative darkens).
    /// Defaults per variant.
    pub lift: Option<f32>,
    /// Drop shadow below the glass.
    pub shadow: bool,
    /// Clip the layer to `shape`. Morphing glass disables this — coverage
    /// comes entirely from the shader's SDF.
    pub clip: bool,
}

impl Glass {
    pub fn regular() -> Self {
        Self {
            variant: GlassVariant::Regular,
            shape: LiquidShape::Capsule,
            tint: None,
            blur_radius: None,
            saturation: None,
            chromatic_aberration: 0.5,
            displacement: 22.0,
            bezel_width: 12.0,
            highlight: 0.9,
            lift: None,
            shadow: true,
            clip: true,
        }
    }

    pub fn clear() -> Self {
        Self {
            variant: GlassVariant::Clear,
            ..Self::regular()
        }
    }

    /// The interactive lens bubble (reference: the iOS toggle/tab-bar drag
    /// lens): fully transparent, magnifying dome across the whole element,
    /// strong rainbow rim.
    pub fn lens() -> Self {
        Self {
            variant: GlassVariant::Lens,
            shape: LiquidShape::Capsule,
            tint: Some(Color::rgba(1.0, 1.0, 1.0, 0.0)),
            blur_radius: None,
            saturation: None,
            chromatic_aberration: 3.2,
            displacement: 30.0,
            // Rim-hugging: refraction, dispersion and sheen live in a thin
            // band at the edge; the interior is pure crisp magnification
            // (a whole-element bezel made the edge term fight the magnify
            // pull mid-face — double images of whatever sat under the lens).
            bezel_width: 16.0,
            highlight: 1.15,
            lift: None,
            shadow: true,
            clip: true,
        }
    }

    pub fn shape(mut self, shape: LiquidShape) -> Self {
        self.shape = shape;
        self
    }

    pub fn tint(mut self, tint: Color) -> Self {
        self.tint = Some(tint);
        self
    }

    pub fn blur_radius(mut self, radius_dp: f32) -> Self {
        self.blur_radius = Some(radius_dp);
        self
    }

    pub fn saturation(mut self, saturation: f32) -> Self {
        self.saturation = Some(saturation);
        self
    }

    pub fn chromatic_aberration(mut self, spread: f32) -> Self {
        self.chromatic_aberration = spread;
        self
    }

    pub fn displacement(mut self, displacement_px: f32) -> Self {
        self.displacement = displacement_px;
        self
    }

    pub fn highlight(mut self, highlight: f32) -> Self {
        self.highlight = highlight;
        self
    }

    /// Overrides the screen-lift (how hard the glass brightens what it
    /// shows; the bar lens uses a near-zero lift to stay transmissive).
    pub fn lift(mut self, lift: f32) -> Self {
        self.lift = Some(lift);
        self
    }

    pub fn shadow(mut self, shadow: bool) -> Self {
        self.shadow = shadow;
        self
    }

    /// Disables the layer clip: the shader's SDF coverage is the only shape
    /// (required while morphing across geometry the clip can't follow).
    pub fn no_clip(mut self) -> Self {
        self.clip = false;
        self
    }

    fn default_blur_radius(&self) -> f32 {
        match self.variant {
            GlassVariant::Regular => 18.0,
            GlassVariant::Clear => 3.0,
            GlassVariant::Lens => 0.0,
        }
    }

    fn default_saturation(&self) -> f32 {
        match self.variant {
            GlassVariant::Regular => 1.5,
            GlassVariant::Clear => 1.25,
            GlassVariant::Lens => 1.0,
        }
    }

    /// Theme-resolved material constants (captured at composition time).
    pub(crate) fn resolve(&self, colors: &LiquidColors) -> ResolvedGlass {
        // Screen-lift keeps the blurred backdrop's colors alive while reading
        // bright (the reference material is far whiter than an alpha tint
        // could get without going milky).
        // The reference menu/bar glass shows blurred content smudges through
        // its body — lift bright but never opaque; the lens lightens what it
        // magnifies noticeably (the pressed toggle's green reads lifted).
        let lift = self.lift.unwrap_or(match (self.variant, colors.is_dark) {
            (GlassVariant::Regular, false) => 0.42,
            (GlassVariant::Regular, true) => -0.38,
            (GlassVariant::Clear, false) => 0.12,
            (GlassVariant::Clear, true) => -0.12,
            (GlassVariant::Lens, false) => 0.10,
            (GlassVariant::Lens, true) => -0.08,
        });
        ResolvedGlass {
            shape: self.shape,
            tint: self.tint.unwrap_or(colors.glass_tint),
            blur_radius_dp: self
                .blur_radius
                .unwrap_or_else(|| self.default_blur_radius()),
            saturation: self.saturation.unwrap_or_else(|| self.default_saturation()),
            chromatic_aberration: self.chromatic_aberration,
            displacement: self.displacement,
            bezel_width_dp: self.bezel_width,
            highlight: self.highlight,
            lift,
            contrast: if self.variant == GlassVariant::Lens {
                1.0
            } else {
                1.03
            },
            edge_band: if self.variant == GlassVariant::Lens {
                0.35
            } else {
                0.4
            },
            shadow: self.shadow,
            clip: self.clip,
            dome_direction: if self.variant == GlassVariant::Lens {
                -1.0
            } else {
                1.0
            },
            magnify: if self.variant == GlassVariant::Lens {
                1.35
            } else {
                1.0
            },
            sheen: if self.variant == GlassVariant::Lens {
                0.05
            } else {
                1.0
            },
            rim_style: if self.variant == GlassVariant::Lens {
                1.0
            } else {
                0.0
            },
            shadow_color: Color::BLACK.with_alpha(match (self.variant, colors.is_dark) {
                (GlassVariant::Lens, false) => 0.14,
                (GlassVariant::Lens, true) => 0.28,
                (_, false) => 0.16,
                (_, true) => 0.5,
            }),
            // The lens is a small floating bubble — its shadow is a tight,
            // barely-there contact hint; large surfaces get the soft wide
            // ambient. The reference thumb shadows are almost invisible.
            shadow_radius: if self.variant == GlassVariant::Lens {
                10.0
            } else {
                22.0
            },
            shadow_offset_y: if self.variant == GlassVariant::Lens {
                3.0
            } else {
                8.0
            },
            // The lens node is padded well past the visible bubble; a strong
            // negative spread pulls the shadow back to hug the glass (the
            // node-sized shadow read as an oversized halo).
            shadow_spread: if self.variant == GlassVariant::Lens {
                -6.0
            } else {
                -2.0
            },
        }
    }
}

impl Default for Glass {
    fn default() -> Self {
        Self::regular()
    }
}

/// A theme-resolved glass material; density and dynamics are applied per
/// frame in the lazy graphics-layer resolver.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ResolvedGlass {
    pub shape: LiquidShape,
    pub tint: Color,
    pub blur_radius_dp: f32,
    pub saturation: f32,
    pub chromatic_aberration: f32,
    pub displacement: f32,
    pub bezel_width_dp: f32,
    pub highlight: f32,
    pub lift: f32,
    pub contrast: f32,
    pub edge_band: f32,
    pub shadow: bool,
    pub clip: bool,
    /// +1 = rim stretch (bars, menus); −1 = magnifying dome (the lens).
    pub dome_direction: f32,
    /// True center magnification (1 = off); the interactive lens uses ~1.35.
    pub magnify: f32,
    /// Broad bezel-glow strength (1 = default; the lens dials it to ~0.12
    /// for the crisp interior of the reference frames).
    pub sheen: f32,
    /// 0 = surface glass (soft white spec rim); 1 = interactive lens (thin
    /// bright line + stronger dark outline, chroma does the color).
    pub rim_style: f32,
    pub shadow_color: Color,
    /// Variant-scaled drop shadow geometry: the lens bubble carries a tight
    /// contact hint, large surfaces a soft wide ambient.
    pub shadow_radius: f32,
    pub shadow_offset_y: f32,
    pub shadow_spread: f32,
}

impl ResolvedGlass {
    /// Builds the backdrop effect chain (blur → lens shader) for the current
    /// density and per-frame dynamics. Cover mode: geometry in px, container
    /// uniform zeroed, the renderer injects the node's rect.
    pub(crate) fn backdrop_effect(&self, density: f32, dynamics: GlassDynamics) -> RenderEffect {
        let density = density.max(f32::EPSILON);
        let mut shader = RuntimeShader::new(LIQUID_GLASS_WGSL);
        if let Some(morph) = dynamics.morph.as_ref() {
            // Morph glass: the container carries the node size in dp and ALL
            // geometry is dp — the shader divides the renderer-injected node
            // pixel rect by the container, so the field lands correctly at
            // any render scale (density-scaled packing broke every capture
            // whose render scale differed from the platform density).
            let (node_w, node_h) = morph.node_size;
            let (cx, cy, w, h, radius) = morph.primary;
            shader.set_float2(0, node_w.max(1.0), node_h.max(1.0));
            shader.set_float2(2, cx, cy);
            shader.set_float2(4, w, h);
            shader.set_float(6, radius);
            let count = morph.shapes.len().min(GlassMorph::MAX_SHAPES);
            shader.set_float(30, count as f32);
            for (index, (sx, sy, sw, sh, sr)) in morph.shapes.iter().take(count).enumerate() {
                let base = 36 + index * 5;
                shader.set_float(base, *sx);
                shader.set_float(base + 1, *sy);
                shader.set_float(base + 2, *sw);
                shader.set_float(base + 3, *sh);
                shader.set_float(base + 4, *sr);
            }
            shader.set_float(31, morph.glue);
            shader.set_float(32, morph.wobble_amplitude);
            shader.set_float(33, morph.wobble_phase);
            shader.set_float(26, morph.bulge_amplitude);
            shader.set_float(27, morph.bulge_direction);
            // Bezel in dp: the shader scales by px-per-dp.
            shader.set_float(7, self.bezel_width_dp);
        } else {
            // Cover mode marker: container size stays zero. Geometry is px at
            // the platform density (node size only known at render time).
            shader.set_float2(0, 0.0, 0.0);
            shader.set_float(6, self.shape.shader_radius_px(density));
            shader.set_float(7, self.bezel_width_dp * density);
        }
        shader.set_float(8, self.displacement);
        shader.set_float(9, 1.5);
        shader.set_float(10, 0.6);
        shader.set_float(
            11,
            (self.highlight + dynamics.highlight_boost).clamp(0.0, 2.0),
        );
        shader.set_float2(12, dynamics.tilt.0, dynamics.tilt.1);
        shader.set_float4(
            14,
            self.tint.r(),
            self.tint.g(),
            self.tint.b(),
            self.tint.a(),
        );
        shader.set_float(18, self.saturation);
        shader.set_float(19, self.chromatic_aberration);
        shader.set_float(20, self.lift);
        shader.set_float(21, 0.5);
        shader.set_float2(22, 0.0, 1.0);
        shader.set_float(24, self.contrast);
        shader.set_float(25, self.edge_band);
        shader.set_float(29, self.sheen);
        shader.set_float(28, self.rim_style);
        shader.set_float(34, self.dome_direction);
        shader.set_float(35, self.magnify + dynamics.magnify_boost.max(0.0));
        // Morph padding: wobble reach plus how far any scene shape (plus its
        // glue neck) extends beyond the primary rect — the capture and the
        // composite surface must cover the whole glued field.
        let morph_pad = dynamics
            .morph
            .as_ref()
            .map(|morph| {
                let (px, py, pw, ph, _) = morph.primary;
                let (left, top) = (px - pw * 0.5, py - ph * 0.5);
                let (right, bottom) = (px + pw * 0.5, py + ph * 0.5);
                let mut shape_reach = 0.0f32;
                for (sx, sy, sw, sh, _) in &morph.shapes {
                    let reach_x = ((sx + sw * 0.5) - right)
                        .max(left - (sx - sw * 0.5))
                        .max(0.0);
                    let reach_y = ((sy + sh * 0.5) - bottom)
                        .max(top - (sy - sh * 0.5))
                        .max(0.0);
                    shape_reach = shape_reach.max(reach_x.max(reach_y));
                }
                let glue_pad = if morph.shapes.is_empty() {
                    0.0
                } else {
                    morph.glue * 2.0
                };
                morph.wobble_amplitude * 2.0 + morph.bulge_amplitude + shape_reach + glue_pad
            })
            .unwrap_or(0.0);
        // Paddings are consumed in LOGICAL units by the backdrop capture and
        // output rects — dp, never density-scaled.
        shader.set_input_padding(self.input_padding() + morph_pad);
        // Morphing glass WRITES outside the node rect (wobble, bulge, glued
        // neighbors, plus the ~2px antialiased rim); declare it so the
        // composite scissor doesn't clip the field at the node edge.
        if dynamics.morph.is_some() {
            shader.set_output_padding(morph_pad + 4.0);
        }

        let lens = RenderEffect::runtime_shader(shader);
        let blur_px = self.blur_radius_dp * density;
        if blur_px > 0.5 {
            RenderEffect::blur(blur_px).then(lens)
        } else {
            lens
        }
    }

    /// Backdrop capture padding (px) covering the largest refracted sample
    /// (see `liquid_glass_input_padding` for the explicit-rect twin). Padded
    /// for tilt up to ±1 per axis so per-frame tilt never outruns the capture.
    fn input_padding(&self) -> f32 {
        let bend = 1.0 - 1.0 / 1.5_f32;
        let reach = 1.0 + std::f32::consts::SQRT_2;
        // Edge band peaks at 4; the dome adds 0.35 × its own slope.
        let slope = 4.0 + 0.35 * (2.0 + 2.0 * 0.6);
        let spread = 1.0 + self.chromatic_aberration.max(0.0) * 0.5;
        let displacement = reach * bend * self.displacement.max(0.0) * slope * spread;
        displacement.ceil() + 2.0
    }
}

/// Modifier extension installing the Liquid Glass material.
pub trait LiquidModifierExt {
    /// Applies the glass material to this composable's bounds: backdrop blur +
    /// lens shader, clipped to `glass.shape`, with a soft drop shadow.
    ///
    /// Must be called in composable context (the material resolves theme
    /// colors at the call site).
    fn glass_effect(self, glass: Glass) -> Modifier;

    /// [`glass_effect`](Self::glass_effect) with per-frame motion inputs; the
    /// closure is read at scene-build time, so animating tilt or highlight
    /// does not recompose.
    fn glass_effect_with(
        self,
        glass: Glass,
        dynamics: impl Fn() -> GlassDynamics + 'static,
    ) -> Modifier;
}

impl LiquidModifierExt for Modifier {
    fn glass_effect(self, glass: Glass) -> Modifier {
        self.glass_effect_with(glass, GlassDynamics::default)
    }

    fn glass_effect_with(
        self,
        glass: Glass,
        dynamics: impl Fn() -> GlassDynamics + 'static,
    ) -> Modifier {
        let colors = crate::theme::liquid_colors();
        let resolved = Rc::new(glass.resolve(&colors));
        let shape = resolved.shape;

        let mut modifier = self;
        if resolved.shadow {
            let shadow_color = resolved.shadow_color;
            let (radius, offset_y, spread) = (
                resolved.shadow_radius,
                resolved.shadow_offset_y,
                resolved.shadow_spread,
            );
            modifier = modifier.drop_shadow(shape.layer_shape(), move |scope| {
                scope.radius = radius;
                scope.spread = spread;
                scope.offset.y = offset_y;
                scope.color = shadow_color;
                // Glass samples the backdrop behind itself — knock the shape
                // out of its own shadow so the material stays bright.
                scope.cutout = true;
            });
        }

        let layer_resolved = Rc::clone(&resolved);
        let clip = resolved.clip;
        modifier.graphics_layer(move || GraphicsLayer {
            backdrop_effect: Some(layer_resolved.backdrop_effect(current_density(), dynamics())),
            shape: shape.layer_shape(),
            clip,
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light_colors() -> LiquidColors {
        LiquidColors::light(Color::from_rgb_u8(0, 122, 255))
    }

    #[test]
    fn regular_glass_resolves_frosted_defaults() {
        let resolved = Glass::regular().resolve(&light_colors());
        assert!(resolved.blur_radius_dp > 5.0, "regular material is frosted");
        assert!(resolved.saturation > 1.3, "regular material is vibrant");
        assert!(resolved.lift > 0.0, "light scheme lifts toward white");
    }

    #[test]
    fn clear_glass_is_barely_frosted() {
        let resolved = Glass::clear().resolve(&light_colors());
        assert!(resolved.blur_radius_dp < 5.0);
        assert!(resolved.saturation < 1.3);
    }

    #[test]
    fn dark_scheme_sinks_lift_and_tint() {
        let dark = LiquidColors::dark(Color::from_rgb_u8(0, 122, 255));
        let resolved = Glass::regular().resolve(&dark);
        assert!(resolved.lift < 0.0, "dark scheme sinks toward black");
        assert_eq!(resolved.tint, dark.glass_tint);
    }

    #[test]
    fn explicit_tint_overrides_theme() {
        let tint = Color::from_rgba_u8(0, 122, 255, 60);
        let resolved = Glass::regular().tint(tint).resolve(&light_colors());
        assert_eq!(resolved.tint, tint);
    }

    #[test]
    fn cover_mode_effect_chains_blur_then_lens() {
        let resolved = Glass::regular().resolve(&light_colors());
        let effect = resolved.backdrop_effect(2.0, GlassDynamics::default());
        let RenderEffect::Chain { first, second } = effect else {
            panic!("regular glass must chain blur → lens");
        };
        assert!(matches!(*first, RenderEffect::Blur { .. }));
        let RenderEffect::Shader { shader } = *second else {
            panic!("second stage must be the lens shader");
        };
        let uniforms = shader.uniforms();
        assert_eq!(uniforms[0], 0.0, "cover mode zeroes the container size");
        assert_eq!(uniforms[6], CAPSULE_SHADER_RADIUS, "capsule sentinel");
        assert!(shader.input_padding() > 0.0);
    }

    #[test]
    fn morph_glass_packs_dp_geometry_with_node_size_container() {
        // The density-vs-render-scale contract: morph geometry is dp and the
        // container carries the node size in dp, so the shader derives
        // px-per-dp from the renderer-injected node pixel rect. Packing
        // density-scaled px here broke every capture whose render scale
        // differed from the platform density (robot captures at 1.0 on a
        // 1.354-density desktop rendered the lens displaced and unscaled).
        let resolved = Glass::lens().resolve(&light_colors());
        let dynamics = GlassDynamics {
            morph: Some(GlassMorph {
                node_size: (78.0, 59.0),
                primary: (39.0, 29.5, 58.0, 39.0, -1.0),
                shapes: vec![(100.0, 29.5, 44.0, 44.0, -1.0)],
                glue: 12.0,
                ..Default::default()
            }),
            ..Default::default()
        };
        // Density must NOT leak into the packed geometry.
        let effect = resolved.backdrop_effect(2.0, dynamics);
        let RenderEffect::Shader { shader } = effect else {
            panic!("lens glass must be a bare shader (no frost blur)");
        };
        let uniforms = shader.uniforms();
        assert_eq!(&uniforms[0..2], &[78.0, 59.0], "container = node size dp");
        assert_eq!(&uniforms[2..6], &[39.0, 29.5, 58.0, 39.0], "primary dp");
        assert_eq!(uniforms[6], -1.0, "capsule sentinel unscaled");
        assert_eq!(uniforms[31], 12.0, "glue dp");
        assert_eq!(&uniforms[36..40], &[100.0, 29.5, 44.0, 44.0], "shape dp");
        assert!(
            shader.output_padding() > 0.0,
            "morph glass declares output padding"
        );
    }

    #[test]
    fn clear_variant_skips_heavy_blur() {
        let resolved = Glass::clear().blur_radius(0.0).resolve(&light_colors());
        let effect = resolved.backdrop_effect(2.0, GlassDynamics::default());
        assert!(matches!(effect, RenderEffect::Shader { .. }));
    }

    #[test]
    fn rounded_rect_radius_scales_with_density() {
        let resolved = Glass::regular()
            .shape(LiquidShape::RoundedRect(16.0))
            .resolve(&light_colors());
        let effect = resolved.backdrop_effect(2.0, GlassDynamics::default());
        let RenderEffect::Chain { second, .. } = effect else {
            panic!("expected chain");
        };
        let RenderEffect::Shader { shader } = *second else {
            panic!("expected lens");
        };
        assert_eq!(shader.uniforms()[6], 32.0, "16dp at 2x density = 32px");
    }
}
