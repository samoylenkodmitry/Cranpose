//! The Liquid Glass material: a backdrop lens (blur → refraction/vibrancy
//! shader) applied to a composable's own bounds through
//! [`LiquidModifierExt::glass_effect`] — the analogue of SwiftUI's
//! `.glassEffect(_:in:)`.

use crate::theme::LiquidColors;
use cranpose_ui::current_density;
use cranpose_ui::Modifier;
use cranpose_ui_graphics::{
    apply_glass_surface_profile, glass_surface_max_slope, Color, GlassSurfaceProfile,
    GraphicsLayer, LayerShape, RenderEffect, RoundedCornerShape, RuntimeShader, LIQUID_GLASS_WGSL,
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

/// Shadow owned by a glass surface. Morphing glass evaluates the same live
/// SDF for this shadow; clipped static glass forwards the values to the layer
/// shadow primitive.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlassShadow {
    pub color: Color,
    pub radius: f32,
    pub offset_y: f32,
    pub spread: f32,
}

impl GlassShadow {
    pub fn new(color: Color, radius: f32, offset_y: f32, spread: f32) -> Self {
        Self {
            color,
            radius: radius.max(0.0),
            offset_y,
            spread,
        }
    }
}

/// Per-frame motion inputs for an interactive glass element, read lazily at
/// scene-build time (no recomposition per frame).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct GlassDynamics {
    /// Motion tilt (finger/pointer/device motion), roughly −1..1 per axis.
    pub tilt: (f32, f32),
    /// Extra specular intensity (0 = spec default; e.g. press boost).
    pub highlight_boost: f32,
    /// Optional per-frame multiplier for the material tint alpha. This lets a
    /// resting selection wash clear continuously as its lens enters flight.
    pub tint_alpha_multiplier: Option<f32>,
    /// Extra physical Z depth over the material profile's base depth. Motion
    /// can deepen the same surface without switching to a separate zoom law.
    pub surface_depth_boost: f32,
    /// Motion-state multiplier for displacement and spectral separation.
    /// Zero keeps the material's full resolved strength; positive values
    /// explicitly scale it, allowing one persistent body to become quieter
    /// at rest without cross-fading to another component.
    pub optical_strength: f32,
    /// Shape morph: when set, the glass geometry is these node-local rects
    /// instead of the node cover — the shapeshift channel.
    pub morph: Option<GlassMorph>,
}

pub(crate) fn neutral_surface_tint(foreground: Color, light_alpha: f32, dark_alpha: f32) -> Color {
    let foreground_luma =
        0.2126 * foreground.r() + 0.7152 * foreground.g() + 0.0722 * foreground.b();
    if foreground_luma < 0.5 {
        Color::BLACK.with_alpha(light_alpha.clamp(0.0, 1.0))
    } else {
        Color::WHITE.with_alpha(dark_alpha.clamp(0.0, 1.0))
    }
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
    /// Blends the primary rounded-rectangle field toward an ellipse. This is
    /// used by expanding droplets whose broad phase has continuous curvature
    /// rather than the straight sides of a capsule.
    pub ellipse_blend: f32,
    /// Area-preserving affine strain applied to the primary shape. Extra
    /// scene shapes remain fixed so nearby glass can join the travelling
    /// droplet without being dragged through its local deformation.
    pub deformation: Option<GlassDeformation>,
}

/// A normalized motion axis and reciprocal scales for incompressible glass.
/// Construction derives the cross-axis scale, so callers cannot describe a
/// deformation that changes the bubble's area.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlassDeformation {
    axis: (f32, f32),
    along: f32,
}

impl GlassDeformation {
    pub fn incompressible(axis: (f32, f32), along: f32) -> Self {
        let length = (axis.0 * axis.0 + axis.1 * axis.1).sqrt();
        let axis = if length > f32::EPSILON {
            (axis.0 / length, axis.1 / length)
        } else {
            (1.0, 0.0)
        };
        Self {
            axis,
            along: along.max(f32::EPSILON),
        }
    }

    pub fn axis(self) -> (f32, f32) {
        self.axis
    }

    pub fn along(self) -> f32 {
        self.along
    }

    pub fn across(self) -> f32 {
        1.0 / self.along
    }
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
    /// Principal-axis wavelength response for the X-Z and Y-Z profiles.
    pub dispersion_axes: (f32, f32),
    /// Lens strength: refraction displacement in px.
    pub displacement: f32,
    /// Bezel width in dp (the refractive rim zone).
    pub bezel_width: f32,
    /// Specular rim intensity.
    pub highlight: f32,
    /// Physical X-Z/Y-Z cross-sections for the surface.
    pub surface_profile: GlassSurfaceProfile,
    /// Broad Fresnel sheen across the bezel. `None` uses the variant default.
    pub sheen: Option<f32>,
    /// Screen-lift override (brightening toward white; negative darkens).
    /// Defaults per variant.
    pub lift: Option<f32>,
    /// Drop shadow below the glass.
    pub shadow: bool,
    /// Per-surface shadow override.
    pub shadow_style: Option<GlassShadow>,
    /// Clip the layer to `shape`. Morphing glass disables this — coverage
    /// comes entirely from the shader's SDF.
    pub clip: bool,
    /// Foreground color whose contrast the frost must protect. `None` uses
    /// the theme label color.
    pub foreground: Option<Color>,
    /// Strength of backdrop+foreground frost adaptation.
    pub adaptive_frost: f32,
    /// Accent applied only to dark foreground detail sampled inside the glass.
    pub content_recolor: Option<(Color, f32)>,
    /// Top-edge fold strength (0 = off): content just above the glass
    /// renders vertically mirrored inside the top band (the reference bar's
    /// meniscus folds section headers upside-down into it).
    pub edge_fold: f32,
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
            dispersion_axes: (1.0, 1.0),
            displacement: 22.0,
            bezel_width: 12.0,
            highlight: 0.9,
            surface_profile: GlassSurfaceProfile::regular(),
            sheen: None,
            lift: None,
            shadow: true,
            shadow_style: None,
            clip: true,
            foreground: None,
            adaptive_frost: 0.65,
            content_recolor: None,
            edge_fold: 0.0,
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
            tint: Some(Color::rgba(1.0, 1.0, 1.0, 0.07)),
            blur_radius: None,
            saturation: None,
            chromatic_aberration: 3.2,
            dispersion_axes: (1.0, 1.0),
            displacement: 30.0,
            // One compact signed cross-section owns the raised lip and the
            // recessed-face return. A wide band reads as two concentric
            // bevels and consumes most of the smaller toggle lens.
            bezel_width: 10.0,
            highlight: 1.15,
            surface_profile: GlassSurfaceProfile::lens(),
            sheen: None,
            lift: None,
            shadow: true,
            shadow_style: None,
            clip: true,
            foreground: None,
            adaptive_frost: 0.0,
            content_recolor: None,
            edge_fold: 0.0,
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

    pub fn dispersion_axes(mut self, x: f32, y: f32) -> Self {
        self.dispersion_axes = (x.clamp(0.0, 4.0), y.clamp(0.0, 4.0));
        self
    }

    pub fn displacement(mut self, displacement_px: f32) -> Self {
        self.displacement = displacement_px;
        self
    }

    pub fn bezel_width(mut self, width_dp: f32) -> Self {
        self.bezel_width = width_dp.max(0.0);
        self
    }

    pub fn highlight(mut self, highlight: f32) -> Self {
        self.highlight = highlight;
        self
    }

    pub fn surface_profile(mut self, profile: GlassSurfaceProfile) -> Self {
        self.surface_profile = profile;
        self
    }

    pub fn sheen(mut self, sheen: f32) -> Self {
        self.sheen = Some(sheen);
        self
    }

    /// Overrides the screen-lift (how hard the glass brightens what it
    /// shows; the bar lens uses a near-zero lift to stay transmissive).
    pub fn lift(mut self, lift: f32) -> Self {
        self.lift = Some(lift);
        self
    }

    pub fn adaptive_frost(mut self, foreground: Color, strength: f32) -> Self {
        self.foreground = Some(foreground);
        self.adaptive_frost = strength.clamp(0.0, 1.0);
        self
    }

    pub fn content_recolor(mut self, color: Color, strength: f32) -> Self {
        self.content_recolor = Some((color, strength.clamp(0.0, 1.0)));
        self
    }

    pub fn edge_fold(mut self, strength: f32) -> Self {
        self.edge_fold = strength;
        self
    }

    pub fn shadow(mut self, shadow: bool) -> Self {
        self.shadow = shadow;
        self
    }

    pub fn shadow_style(mut self, shadow: GlassShadow) -> Self {
        self.shadow_style = Some(shadow);
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
        let foreground = self.foreground.unwrap_or(colors.label);
        let shadow = self.shadow_style.unwrap_or_else(|| {
            GlassShadow::new(
                Color::BLACK.with_alpha(match (self.variant, colors.is_dark) {
                    (GlassVariant::Lens, false) => 0.14,
                    (GlassVariant::Lens, true) => 0.28,
                    (_, false) => 0.16,
                    (_, true) => 0.5,
                }),
                if self.variant == GlassVariant::Lens {
                    10.0
                } else {
                    22.0
                },
                if self.variant == GlassVariant::Lens {
                    3.0
                } else {
                    8.0
                },
                if self.variant == GlassVariant::Lens {
                    -6.0
                } else {
                    -2.0
                },
            )
        });
        ResolvedGlass {
            shape: self.shape,
            tint: self.tint.unwrap_or(colors.glass_tint),
            blur_radius_dp: self
                .blur_radius
                .unwrap_or_else(|| self.default_blur_radius()),
            saturation: self.saturation.unwrap_or_else(|| self.default_saturation()),
            chromatic_aberration: self.chromatic_aberration,
            dispersion_axes: self.dispersion_axes,
            displacement: self.displacement,
            bezel_width_dp: self.bezel_width,
            highlight: self.highlight,
            surface_profile: self.surface_profile,
            lift,
            contrast: if self.variant == GlassVariant::Lens {
                1.0
            } else {
                1.03
            },
            shadow: self.shadow,
            clip: self.clip,
            foreground_luma: 0.2126 * foreground.r()
                + 0.7152 * foreground.g()
                + 0.0722 * foreground.b(),
            adaptive_frost: self.adaptive_frost,
            content_recolor: self.content_recolor.unwrap_or((Color::BLACK, 0.0)),
            edge_fold: self.edge_fold,
            sheen: self.sheen.unwrap_or(if self.variant == GlassVariant::Lens {
                0.05
            } else {
                1.0
            }),
            rim_style: if self.variant == GlassVariant::Lens {
                1.0
            } else {
                0.0
            },
            shadow_color: shadow.color,
            shadow_radius: shadow.radius,
            shadow_offset_y: shadow.offset_y,
            shadow_spread: shadow.spread,
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
    pub dispersion_axes: (f32, f32),
    pub displacement: f32,
    pub bezel_width_dp: f32,
    pub highlight: f32,
    pub surface_profile: GlassSurfaceProfile,
    pub lift: f32,
    pub contrast: f32,
    pub shadow: bool,
    pub clip: bool,
    /// Broad bezel-glow strength selected by the resolved material.
    pub sheen: f32,
    /// 0 = surface glass (soft white spec rim); 1 = interactive lens (thin
    /// bright line + stronger dark outline, chroma does the color).
    pub rim_style: f32,
    pub foreground_luma: f32,
    pub adaptive_frost: f32,
    pub content_recolor: (Color, f32),
    /// Top-edge fold strength (see [`Glass::edge_fold`]).
    pub edge_fold: f32,
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
        self.runtime_effect(density, dynamics, false)
    }

    fn content_mask_effect(&self, density: f32, dynamics: GlassDynamics) -> RenderEffect {
        self.runtime_effect(density, dynamics, true)
    }

    fn runtime_effect(
        &self,
        density: f32,
        dynamics: GlassDynamics,
        content_mask: bool,
    ) -> RenderEffect {
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
            shader.set_float(110, morph.ellipse_blend.clamp(0.0, 1.0));
            if let Some(deformation) = morph.deformation {
                let axis = deformation.axis();
                shader.set_float2(106, axis.0, axis.1);
                shader.set_float(108, deformation.along());
                shader.set_float(109, deformation.across());
            } else {
                shader.set_float2(106, 1.0, 0.0);
                shader.set_float2(108, 1.0, 1.0);
            }
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
            self.tint.a()
                * dynamics
                    .tint_alpha_multiplier
                    .unwrap_or(1.0)
                    .clamp(0.0, 1.0),
        );
        shader.set_float(18, self.saturation);
        shader.set_float(19, self.chromatic_aberration);
        shader.set_float2(118, self.dispersion_axes.0, self.dispersion_axes.1);
        shader.set_float(20, self.lift);
        shader.set_float(21, 0.5);
        shader.set_float2(22, 0.0, 1.0);
        shader.set_float(24, self.contrast);
        shader.set_float(29, self.sheen);
        shader.set_float(28, self.rim_style);
        shader.set_float(111, dynamics.optical_strength.clamp(0.0, 1.0));
        shader.set_float(112, if content_mask { 1.0 } else { 0.0 });
        let profile_unit_scale = if dynamics.morph.is_some() {
            1.0
        } else {
            density
        };
        apply_glass_surface_profile(
            &mut shader,
            self.surface_profile,
            profile_unit_scale * (1.0 + dynamics.surface_depth_boost).max(0.0),
        );
        shader.set_float(91, self.adaptive_frost);
        shader.set_float(92, self.edge_fold);
        shader.set_float(97, self.foreground_luma);
        shader.set_float(98, self.content_recolor.1);
        shader.set_float4(
            99,
            self.content_recolor.0.r(),
            self.content_recolor.0.g(),
            self.content_recolor.0.b(),
            0.0,
        );
        let dynamic_shadow = !self.clip && self.shadow;
        shader.set_float(
            102,
            if dynamic_shadow {
                self.shadow_color.a() * 0.55
            } else {
                0.0
            },
        );
        shader.set_float(103, self.shadow_radius);
        shader.set_float(104, self.shadow_offset_y);
        shader.set_float(105, self.shadow_spread);
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
            let shadow_reach = if dynamic_shadow {
                self.shadow_radius + self.shadow_offset_y.abs() + self.shadow_spread.max(0.0)
            } else {
                0.0
            };
            shader.set_output_padding(morph_pad + shadow_reach + 4.0);
        }

        let lens = RenderEffect::runtime_shader(shader);
        if content_mask {
            return lens;
        }
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
        let slope = glass_surface_max_slope(self.surface_profile, self.bezel_width_dp.max(1.0));
        let reach = slope + std::f32::consts::SQRT_2 * 0.18;
        let axis_scale = self.dispersion_axes.0.max(self.dispersion_axes.1);
        let spread = 1.0 + axis_scale * self.chromatic_aberration.max(0.0) * 0.5;
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
        if resolved.shadow && resolved.clip {
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
        modifier.graphics_layer(move || {
            let density = current_density();
            let frame = dynamics();
            let render_effect = (!clip && frame.morph.is_some())
                .then(|| layer_resolved.content_mask_effect(density, frame.clone()));
            GraphicsLayer {
                backdrop_effect: Some(layer_resolved.backdrop_effect(density, frame)),
                render_effect,
                shape: shape.layer_shape(),
                clip,
                ..Default::default()
            }
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
        let colors = light_colors();
        let resolved = Glass::regular().resolve(&colors);
        assert!(resolved.blur_radius_dp > 5.0, "regular material is frosted");
        assert!(resolved.saturation > 1.3, "regular material is vibrant");
        assert!(resolved.lift > 0.0, "light scheme lifts toward white");
        assert_eq!(resolved.adaptive_frost, 0.65);
        let label_luma =
            0.2126 * colors.label.r() + 0.7152 * colors.label.g() + 0.0722 * colors.label.b();
        assert!((resolved.foreground_luma - label_luma).abs() < 1e-6);
    }

    #[test]
    fn neutral_surface_tint_follows_foreground_polarity() {
        let light_surface = neutral_surface_tint(Color::BLACK, 0.08, 0.10);
        assert_eq!(light_surface, Color::BLACK.with_alpha(0.08));
        let dark_surface = neutral_surface_tint(Color::WHITE, 0.08, 0.10);
        assert_eq!(dark_surface, Color::WHITE.with_alpha(0.10));
    }

    #[test]
    fn dynamics_can_clear_a_material_tint_without_switching_bodies() {
        let tint = Color::BLACK.with_alpha(0.8);
        let resolved = Glass::lens().tint(tint).resolve(&light_colors());
        let effect = resolved.backdrop_effect(
            1.0,
            GlassDynamics {
                tint_alpha_multiplier: Some(0.25),
                ..Default::default()
            },
        );
        let RenderEffect::Shader { shader } = effect else {
            panic!("lens glass must be a bare shader");
        };
        assert!((shader.uniforms()[17] - 0.2).abs() < 1.0e-6);
    }

    #[test]
    fn clear_glass_is_barely_frosted() {
        let resolved = Glass::clear().resolve(&light_colors());
        assert!(resolved.blur_radius_dp < 5.0);
        assert!(resolved.saturation < 1.3);
    }

    #[test]
    fn interactive_lens_uses_a_compact_signed_meniscus() {
        let glass = Glass::lens();
        assert!(
            (9.0..=11.0).contains(&glass.bezel_width),
            "the target lens has one thin raised-lip/recess section, got {}dp",
            glass.bezel_width
        );
        let resolved = glass.resolve(&light_colors());
        let RenderEffect::Shader { shader } =
            resolved.backdrop_effect(1.0, GlassDynamics::default())
        else {
            panic!("lens glass must be a bare shader");
        };
        assert_eq!(shader.uniforms()[7], glass.bezel_width);

        assert_eq!(Glass::lens().bezel_width(7.5).bezel_width, 7.5);
        assert_eq!(Glass::lens().bezel_width(-1.0).bezel_width, 0.0);
        assert_eq!(
            Glass::lens().dispersion_axes(-1.0, 5.0).dispersion_axes,
            (0.0, 4.0)
        );
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
    fn explicit_sheen_overrides_the_variant_default() {
        let resolved = Glass::lens().sheen(0.85).resolve(&light_colors());
        assert_eq!(resolved.sheen, 0.85);
        let effect = resolved.backdrop_effect(1.0, GlassDynamics::default());
        let RenderEffect::Shader { shader } = effect else {
            panic!("lens glass must be a bare shader");
        };
        assert_eq!(shader.uniforms()[29], 0.85);
    }

    #[test]
    fn physical_surface_profile_is_packed_for_the_shader() {
        let x = cranpose_ui_graphics::GlassProfileCurve::from_points(&[
            (0.0, 0.1),
            (0.5, 0.2),
            (1.0, 0.8),
        ])
        .expect("X-Z profile");
        let y = cranpose_ui_graphics::GlassProfileCurve::from_points(&[
            (0.0, 0.1),
            (0.6, 0.3),
            (1.0, 0.7),
        ])
        .expect("Y-Z profile");
        let profile = GlassSurfaceProfile::new(x, y, 4.0, 3.0)
            .and_then(|profile| profile.with_axis_coupling(0.3))
            .expect("surface profile");
        let resolved = Glass::lens()
            .surface_profile(profile)
            .dispersion_axes(0.25, 2.0)
            .resolve(&light_colors());
        assert_eq!(resolved.surface_profile, profile);
        assert_eq!(resolved.dispersion_axes, (0.25, 2.0));
        let effect = resolved.backdrop_effect(
            1.0,
            GlassDynamics {
                surface_depth_boost: 0.25,
                ..Default::default()
            },
        );
        let RenderEffect::Shader { shader } = effect else {
            panic!("lens glass must be a bare shader");
        };
        let uniforms = shader.uniforms();
        assert_eq!(&uniforms[113..118], &[1.0, 3.0, 3.0, 3.0, 5.0]);
        assert_eq!(&uniforms[118..120], &[0.25, 2.0]);
        assert_eq!(uniforms[120], 0.0);
        assert_eq!(uniforms[121], 0.1);
        assert_eq!(uniforms[126], 1.0);
        assert_eq!(uniforms[127], 0.8);
        assert_eq!(uniforms[140], 0.0);
        assert_eq!(uniforms[141], 0.1);
        assert_eq!(uniforms[146], 1.0);
        assert_eq!(uniforms[147], 0.7);
        assert_eq!(uniforms[158], 0.3);
    }

    #[test]
    fn adaptive_frost_and_inside_recolor_are_packed() {
        let foreground = Color::WHITE;
        let accent = Color::from_rgb_u8(0, 122, 255);
        let resolved = Glass::lens()
            .adaptive_frost(foreground, 0.75)
            .content_recolor(accent, 0.9)
            .resolve(&light_colors());
        let effect = resolved.backdrop_effect(1.0, GlassDynamics::default());
        let RenderEffect::Shader { shader } = effect else {
            panic!("lens glass must be a bare shader");
        };
        let uniforms = shader.uniforms();
        assert_eq!(uniforms[91], 0.75);
        assert!((uniforms[97] - 1.0).abs() < 1e-6);
        assert_eq!(uniforms[98], 0.9);
        assert_eq!(&uniforms[99..102], &[accent.r(), accent.g(), accent.b()]);

        let clamped = Glass::lens()
            .adaptive_frost(foreground, 2.0)
            .content_recolor(accent, -1.0)
            .resolve(&light_colors());
        assert_eq!(clamped.adaptive_frost, 1.0);
        assert_eq!(clamped.content_recolor.1, 0.0);
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
        let resolved = Glass::lens().no_clip().resolve(&light_colors());
        let dynamics = GlassDynamics {
            optical_strength: 0.3,
            morph: Some(GlassMorph {
                node_size: (78.0, 59.0),
                primary: (39.0, 29.5, 58.0, 39.0, -1.0),
                shapes: vec![(100.0, 29.5, 44.0, 44.0, -1.0)],
                glue: 12.0,
                ellipse_blend: 0.75,
                deformation: Some(GlassDeformation::incompressible((3.0, 4.0), 1.25)),
                ..Default::default()
            }),
            ..Default::default()
        };
        // Density must NOT leak into the packed geometry.
        let effect = resolved.backdrop_effect(2.0, dynamics.clone());
        let RenderEffect::Shader { shader } = effect else {
            panic!("lens glass must be a bare shader (no frost blur)");
        };
        let uniforms = shader.uniforms();
        assert_eq!(&uniforms[0..2], &[78.0, 59.0], "container = node size dp");
        assert_eq!(&uniforms[2..6], &[39.0, 29.5, 58.0, 39.0], "primary dp");
        assert_eq!(uniforms[6], -1.0, "capsule sentinel unscaled");
        assert_eq!(uniforms[31], 12.0, "glue dp");
        assert_eq!(&uniforms[36..40], &[100.0, 29.5, 44.0, 44.0], "shape dp");
        assert_eq!(&uniforms[106..108], &[0.6, 0.8], "normalized strain axis");
        assert_eq!(&uniforms[108..110], &[1.25, 0.8], "reciprocal scales");
        assert_eq!(uniforms[110], 0.75, "primary ellipse blend");
        assert_eq!(uniforms[111], 0.3, "dynamic optical strength");
        assert_eq!(uniforms[112], 0.0, "backdrop mode must preserve the lens");
        assert!(uniforms[102] > 0.0, "morph shadow is generated by the SDF");
        assert_eq!(uniforms[103], resolved.shadow_radius);
        assert_eq!(uniforms[104], resolved.shadow_offset_y);
        assert_eq!(uniforms[105], resolved.shadow_spread);
        assert!(
            shader.output_padding() > resolved.shadow_radius,
            "morph output padding must include the dynamic shadow"
        );

        let mask = resolved.content_mask_effect(2.0, dynamics);
        let RenderEffect::Shader {
            shader: mask_shader,
        } = mask
        else {
            panic!("the exact morph content mask must be one shader stage");
        };
        let mask_uniforms = mask_shader.uniforms();
        assert_eq!(&mask_uniforms[0..6], &uniforms[0..6]);
        assert_eq!(&mask_uniforms[30..40], &uniforms[30..40]);
        assert_eq!(&mask_uniforms[106..111], &uniforms[106..111]);
        assert_eq!(mask_uniforms[112], 1.0, "content mask mode is explicit");
    }

    #[test]
    fn explicit_shadow_style_controls_static_and_morph_shadow_geometry() {
        let style = GlassShadow::new(Color::BLACK.with_alpha(0.07), 32.0, 10.0, 1.5);
        let resolved = Glass::regular()
            .shadow_style(style)
            .no_clip()
            .resolve(&light_colors());
        assert_eq!(resolved.shadow_color, style.color);
        assert_eq!(resolved.shadow_radius, 32.0);
        assert_eq!(resolved.shadow_offset_y, 10.0);
        assert_eq!(resolved.shadow_spread, 1.5);
        assert_eq!(GlassShadow::new(Color::BLACK, -2.0, 0.0, 0.0).radius, 0.0);
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
