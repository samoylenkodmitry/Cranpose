//! The Liquid Glass material: Gaussian backdrop frost followed by wcKSRD
//! refraction applied to a composable's own bounds through
//! [`LiquidModifierExt::glass_effect`] — the analogue of SwiftUI's
//! `.glassEffect(_:in:)`.

use std::{cell::Cell, rc::Rc};

use cranpose_ui::{Modifier, current_density};
use cranpose_ui_graphics::{
    Color, GLASS_ACTIVITY_UNIFORM, GLASS_ADAPTIVE_FROST_UNIFORM, GLASS_BLUR_RADIUS_UNIFORM,
    GLASS_DISPERSION_UNIFORM, GLASS_EFFECT_DENSITY_UNIFORM, GLASS_FOLD_DEPTH_UNIFORM,
    GLASS_LIGHT_DIRECTION_UNIFORM, GLASS_MENISCUS_ABSORPTION_UNIFORM,
    GLASS_OPTICAL_ZOOM_ANCHOR_UNIFORM, GLASS_OPTICAL_ZOOM_UNIFORM,
    GLASS_PHYSICAL_REFRACTION_DEPTH_ENABLED_UNIFORM, GLASS_PHYSICAL_REFRACTION_DEPTH_UNIFORM,
    GLASS_REFRACTION_CURVE_UNIFORM, GLASS_RESTING_TINT_UNIFORM,
    GLASS_TRANSMISSION_REFRACTION_UNIFORM, GraphicsLayer, LIQUID_GLASS_WGSL, LayerShape, Rect,
    RenderEffect, RoundedCornerShape, RuntimeShader, TileMode, liquid_glass_runtime_effect,
};

use crate::theme::LiquidColors;

thread_local! {
    static GLASS_LIGHT_RETURN: Cell<(f32, f32)> = const { Cell::new((0.0, 1.0)) };
}

/// Sets the ambient glass light return direction (screen space, need not be
/// normalized — the shader normalizes). Feed device attitude here.
pub fn set_glass_light_direction(direction: (f32, f32)) {
    GLASS_LIGHT_RETURN.with(|cell| cell.set(direction));
}

/// The current ambient glass light return direction.
pub fn glass_light_direction() -> (f32, f32) {
    GLASS_LIGHT_RETURN.with(|cell| cell.get())
}

const CAPSULE_CLIP_RADIUS: f32 = 1.0e6;

const CAPSULE_SHADER_RADIUS: f32 = -1.0;

const WCKSRD_OPTICAL_BLUR_RADIUS_PX: f32 = 2.0;

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
    /// Continuous optical presence. `None` preserves the resolved material;
    /// `Some(0)` is an exact backdrop identity while retaining the same node,
    /// SDF, and pointer ride path.
    pub activity: Option<f32>,
    /// Base tint of the same persistent SDF surface at zero optical activity.
    /// Its alpha cross-fades out as the refractive material rises.
    pub resting_tint: Option<Color>,
    /// Extra specular intensity (0 = spec default; e.g. press boost).
    pub highlight_boost: f32,
    /// Per-frame saturation added to the resolved material. Interactive
    /// surfaces use this to raise chroma without changing their base tint.
    pub saturation_boost: f32,
    /// Optional per-frame multiplier for the material tint alpha. Values
    /// below one clear a resting wash; values above one densify a raised tint.
    pub tint_alpha_multiplier: Option<f32>,
    /// Shape morph: when set, the glass geometry is these node-local rects
    /// instead of the node cover — the shapeshift channel.
    pub morph: Option<GlassMorph>,
    /// Touch glow: `(x_dp, y_dp, intensity)` in node-local dp. A pressed
    /// surface concentrates saturation and a soft light in a radial
    /// gradient under the finger (never a flat recolor).
    pub touch: Option<(f32, f32, f32)>,
    /// Dome press depth: how squashed the interactive dome is. At 1 the
    /// pressed dome refracts deep and vivid (wide rim band, strong
    /// chromatic split — the reference toggle's gray-hold rainbow); toward
    /// 0 the released bead relaxes shallow and its split fades with it
    /// (the reference's thin settled ring). `None` = 1.
    pub press_depth: Option<f32>,
}

impl GlassDynamics {
    /// The universal touched-up state (user-directed law): a pressed
    /// liquid surface comes closer to the user while its OWN colors go
    /// HDR-bright and saturated, with a radial glow following the finger.
    /// Never a repaint with alternate colors — the boosts amplify whatever
    /// the surface already is. `press` is 0..1; `finger` is node-local dp
    /// (falls back to `center`, the surface's own heart).
    pub fn touched_up(
        mut self,
        press: f32,
        finger: Option<(f32, f32)>,
        center: (f32, f32),
    ) -> Self {
        let press = press.clamp(0.0, 1.0);
        if press <= 0.0 {
            return self;
        }
        self.highlight_boost += 0.85 * press;
        self.saturation_boost += 0.45 * press;
        let (fx, fy) = finger.unwrap_or(center);
        let carried = self.touch.map(|(_, _, i)| i).unwrap_or(0.0);
        self.touch = Some((fx, fy, (carried + press).clamp(0.0, 1.0)));
        self
    }
}

fn foreground_is_dark(foreground: Color) -> bool {
    let foreground_luma =
        0.2126 * foreground.r() + 0.7152 * foreground.g() + 0.0722 * foreground.b();
    foreground_luma < 0.5
}

fn boost_tint_saturation(tint: Color, boost: f32) -> Color {
    if boost.abs() <= f32::EPSILON {
        return tint;
    }
    let luma = 0.2126 * tint.r() + 0.7152 * tint.g() + 0.0722 * tint.b();
    let saturation = (1.0 + boost).max(0.0);
    Color::rgba(
        (luma + (tint.r() - luma) * saturation).clamp(0.0, 1.0),
        (luma + (tint.g() - luma) * saturation).clamp(0.0, 1.0),
        (luma + (tint.b() - luma) * saturation).clamp(0.0, 1.0),
        tint.a(),
    )
}

pub(crate) fn neutral_surface_tint(foreground: Color, light_alpha: f32, dark_alpha: f32) -> Color {
    if foreground_is_dark(foreground) {
        Color::BLACK.with_alpha(light_alpha.clamp(0.0, 1.0))
    } else {
        Color::WHITE.with_alpha(dark_alpha.clamp(0.0, 1.0))
    }
}

pub(crate) fn neutral_surface_lift(foreground: Color, light_lift: f32, dark_lift: f32) -> f32 {
    if foreground_is_dark(foreground) {
        light_lift
    } else {
        dark_lift
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
    /// Offset (dp) of the optical-zoom axis from the primary SDF center. A
    /// leaning droplet's silhouette shifts toward its travel side while its
    /// curvature apex stays over the content it rides; anchoring the
    /// magnification there keeps the face filled by that content instead of
    /// pulling in whatever lies beyond it.
    pub zoom_anchor: (f32, f32),
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
    /// wcKSRD refraction depth as a fraction of the shape inradius. Regular
    /// frosted surfaces default to zero so their backdrop stays motion-stable;
    /// interactive lenses opt into optical displacement explicitly.
    pub refraction_depth: f32,
    /// Optional physical wcKSRD refraction depth in dp. When present, this
    /// keeps the optical band equally deep across surfaces with different
    /// inradii instead of scaling it from each surface's height.
    pub refraction_depth_dp: Option<f32>,
    /// Normalized wcKSRD ray-return exponent.
    pub refraction_curve: f32,
    /// Normalized wcKSRD spectral ray separation.
    pub dispersion: f32,
    /// Fraction of the wcKSRD displacement applied to the transmitted
    /// backdrop. Regular frosted surfaces default to zero. The mirrored
    /// meniscus follows its own optical path.
    pub transmission_refraction: f32,
    /// Energy removed from the transmitted ray at the meniscus. Reflection
    /// and spectral return follow independent paths.
    pub meniscus_absorption: f32,
    /// Depth of the rim fold band in dp: the raised lens rim replays its
    /// interior mirrored toward the edge (a pure displacement). Zero
    /// disables the fold.
    pub fold_depth: f32,
    /// Uniform face magnification of a riding lens (1.0 = no zoom): the
    /// backdrop projects enlarged across the whole face while the rim band
    /// keeps the wcKSRD edge mapping.
    pub optical_zoom: f32,
    /// Meniscus rim reflectivity multiplier (1.0 = the reference toggle's
    /// visible rim line; near 0 = the segmented lens's invisible body).
    pub rim_reflection: f32,
    /// Optical ink recolor: the lens recolors dark transmitted ink toward
    /// this color at the given strength (the reference tab bubble's
    /// color-mask act). None = off.
    pub ink_recolor: Option<(Color, f32)>,
    /// Specular rim intensity.
    pub highlight: f32,
    /// Screen-lift override (brightening toward white; negative darkens).
    /// Defaults per variant.
    pub lift: Option<f32>,
    /// Tone-compression override around the shader's mid pivot (<1 pulls
    /// every backdrop toward mid-luminance — the reference dark menu reads
    /// bright magenta over deep purple AND dim gray over white through one
    /// law). Defaults per variant.
    pub contrast: Option<f32>,
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
}

impl Glass {
    /// A motion-stable frosted surface without optical displacement.
    pub fn regular() -> Self {
        Self {
            variant: GlassVariant::Regular,
            shape: LiquidShape::Capsule,
            tint: None,
            blur_radius: None,
            saturation: None,
            refraction_depth: 0.0,
            refraction_depth_dp: None,
            refraction_curve: 0.25,
            dispersion: 0.0,
            transmission_refraction: 0.0,
            meniscus_absorption: 1.0,
            fold_depth: 0.0,
            optical_zoom: 1.0,
            rim_reflection: 1.0,
            ink_recolor: None,
            highlight: 0.9,
            lift: None,
            contrast: None,
            shadow: true,
            shadow_style: None,
            clip: true,
            foreground: None,
            adaptive_frost: 0.65,
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
            refraction_depth: 0.34,
            refraction_depth_dp: None,
            refraction_curve: 1.0,
            dispersion: 0.30,
            transmission_refraction: 1.0,
            meniscus_absorption: 1.0,
            fold_depth: 0.0,
            optical_zoom: 1.0,
            rim_reflection: 1.0,
            ink_recolor: None,
            highlight: 1.15,
            lift: None,
            contrast: None,
            shadow: true,
            shadow_style: None,
            clip: true,
            foreground: None,
            adaptive_frost: 0.0,
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

    pub fn refraction_depth(mut self, fraction: f32) -> Self {
        self.refraction_depth = fraction.clamp(0.0, 2.0);
        self.refraction_depth_dp = None;
        self
    }

    /// Uses a density-stable physical refraction depth instead of a fraction
    /// of the live shape inradius.
    pub fn refraction_depth_dp(mut self, depth_dp: f32) -> Self {
        self.refraction_depth_dp = Some(depth_dp.max(0.0));
        self
    }

    pub fn refraction_curve(mut self, curve: f32) -> Self {
        self.refraction_curve = curve.clamp(0.05, 1.0);
        self
    }

    pub fn dispersion(mut self, strength: f32) -> Self {
        self.dispersion = strength.clamp(0.0, 1.0);
        self
    }

    pub fn transmission_refraction(mut self, strength: f32) -> Self {
        self.transmission_refraction = strength.clamp(0.0, 1.0);
        self
    }

    pub fn meniscus_absorption(mut self, strength: f32) -> Self {
        self.meniscus_absorption = strength.clamp(0.0, 1.0);
        self
    }

    /// Sets the rim fold band depth in dp (zero disables the fold).
    pub fn fold_depth(mut self, depth_dp: f32) -> Self {
        self.fold_depth = depth_dp.max(0.0);
        self
    }

    /// Sets the uniform face magnification of a riding lens (1.0 = none).
    pub fn optical_zoom(mut self, zoom: f32) -> Self {
        self.optical_zoom = zoom.max(1.0);
        self
    }

    /// Sets the meniscus rim reflectivity (1.0 = full reference line).
    pub fn rim_reflection(mut self, reflectivity: f32) -> Self {
        self.rim_reflection = reflectivity.clamp(0.0, 2.0);
        self
    }

    /// The lens recolors dark transmitted ink toward `color` at `strength`
    /// (0..1) — the reference bubble's color-mask act as a material optic.
    pub fn ink_recolor(mut self, color: Color, strength: f32) -> Self {
        self.ink_recolor = Some((color, strength.clamp(0.0, 1.0)));
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

    /// Overrides the tone compression around the shader's mid pivot
    /// (values below one pull every backdrop toward mid-luminance).
    pub fn contrast(mut self, contrast: f32) -> Self {
        self.contrast = Some(contrast.max(0.05));
        self
    }

    pub fn adaptive_frost(mut self, foreground: Color, strength: f32) -> Self {
        self.foreground = Some(foreground);
        self.adaptive_frost = strength.clamp(0.0, 1.0);
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
            GlassVariant::Regular => 8.0,
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

    /// The backdrop effect this material produces for a node at `density`
    /// under `dynamics`, resolved against `colors`: the render effect the
    /// glass modifiers install, for a layer built without them.
    pub fn backdrop_effect(
        &self,
        colors: &LiquidColors,
        density: f32,
        dynamics: GlassDynamics,
    ) -> RenderEffect {
        self.resolve(colors).backdrop_effect(density, dynamics)
    }

    pub(crate) fn resolve(&self, colors: &LiquidColors) -> ResolvedGlass {
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
            refraction_depth: self.refraction_depth,
            refraction_depth_dp: self.refraction_depth_dp,
            refraction_curve: self.refraction_curve,
            dispersion: self.dispersion,
            transmission_refraction: self.transmission_refraction,
            meniscus_absorption: self.meniscus_absorption,
            fold_depth: self.fold_depth,
            optical_zoom: self.optical_zoom,
            rim_reflection: self.rim_reflection,
            ink_recolor: self.ink_recolor,
            highlight: self.highlight,
            lift,
            contrast: self.contrast.unwrap_or(match self.variant {
                GlassVariant::Lens => 1.0,
                _ => 1.03,
            }),
            shadow: self.shadow,
            clip: self.clip,
            foreground_luma: 0.2126 * foreground.r()
                + 0.7152 * foreground.g()
                + 0.0722 * foreground.b(),
            adaptive_frost: self.adaptive_frost,
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

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ResolvedGlass {
    pub shape: LiquidShape,
    pub tint: Color,
    pub blur_radius_dp: f32,
    pub saturation: f32,
    pub refraction_depth: f32,
    pub refraction_depth_dp: Option<f32>,
    pub refraction_curve: f32,
    pub dispersion: f32,
    pub transmission_refraction: f32,
    pub meniscus_absorption: f32,
    pub fold_depth: f32,
    pub optical_zoom: f32,
    pub rim_reflection: f32,
    pub ink_recolor: Option<(Color, f32)>,
    pub highlight: f32,
    pub lift: f32,
    pub contrast: f32,
    pub shadow: bool,
    pub clip: bool,
    pub rim_style: f32,
    pub foreground_luma: f32,
    pub adaptive_frost: f32,
    pub shadow_color: Color,
    pub shadow_radius: f32,
    pub shadow_offset_y: f32,
    pub shadow_spread: f32,
}

impl ResolvedGlass {
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
        let activity = dynamics
            .activity
            .filter(|value| value.is_finite())
            .unwrap_or(1.0)
            .clamp(0.0, 1.0);
        let mut shader = RuntimeShader::new(LIQUID_GLASS_WGSL);
        if let Some(morph) = dynamics.morph.as_ref() {
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
            shader.set_float(32, morph.wobble_amplitude * activity);
            shader.set_float(33, morph.wobble_phase);
            shader.set_float(26, morph.bulge_amplitude * activity);
            shader.set_float(27, morph.bulge_direction);
            shader.set_float(110, morph.ellipse_blend.clamp(0.0, 1.0) * activity);
            if let Some(deformation) = morph.deformation {
                let axis = deformation.axis();
                let along = 1.0 + (deformation.along() - 1.0) * activity;
                shader.set_float2(106, axis.0, axis.1);
                shader.set_float(108, along);
                shader.set_float(109, 1.0 / along);
            } else {
                shader.set_float2(106, 1.0, 0.0);
                shader.set_float2(108, 1.0, 1.0);
            }
        } else {
            shader.set_float2(0, 0.0, 0.0);
            shader.set_float(6, self.shape.shader_radius_px(density));
        }
        let press_depth = dynamics.press_depth.unwrap_or(1.0).clamp(0.0, 1.0);
        shader.set_float(9, self.refraction_depth * activity * press_depth);
        shader.set_float(
            GLASS_PHYSICAL_REFRACTION_DEPTH_UNIFORM,
            self.refraction_depth_dp.unwrap_or(0.0) * activity * press_depth,
        );
        shader.set_float(
            GLASS_PHYSICAL_REFRACTION_DEPTH_ENABLED_UNIFORM,
            if self.refraction_depth_dp.is_some() {
                1.0
            } else {
                0.0
            },
        );
        shader.set_float(
            GLASS_REFRACTION_CURVE_UNIFORM,
            self.refraction_curve * activity,
        );
        shader.set_float(
            GLASS_DISPERSION_UNIFORM,
            self.dispersion * activity * press_depth,
        );
        shader.set_float(
            GLASS_TRANSMISSION_REFRACTION_UNIFORM,
            self.transmission_refraction * activity,
        );
        shader.set_float(
            GLASS_MENISCUS_ABSORPTION_UNIFORM,
            self.meniscus_absorption * press_depth,
        );
        shader.set_float(
            GLASS_FOLD_DEPTH_UNIFORM,
            self.fold_depth.max(0.0) * press_depth,
        );
        shader.set_float(
            GLASS_OPTICAL_ZOOM_UNIFORM,
            1.0 + (self.optical_zoom - 1.0).max(0.0) * activity,
        );
        let zoom_anchor = dynamics
            .morph
            .as_ref()
            .map(|morph| morph.zoom_anchor)
            .unwrap_or((0.0, 0.0));
        shader.set_float2(
            GLASS_OPTICAL_ZOOM_ANCHOR_UNIFORM,
            zoom_anchor.0,
            zoom_anchor.1,
        );
        shader.set_float(121, self.rim_reflection.max(0.001));
        let (ink_color, ink_strength) = self
            .ink_recolor
            .map(|(color, strength)| (color, strength * activity))
            .unwrap_or((Color::TRANSPARENT, 0.0));
        shader.set_float(124, ink_color.r());
        shader.set_float(125, ink_color.g());
        shader.set_float(126, ink_color.b());
        shader.set_float(127, ink_strength);
        let (light_x, light_y) = glass_light_direction();
        shader.set_float(GLASS_LIGHT_DIRECTION_UNIFORM, light_x);
        shader.set_float(GLASS_LIGHT_DIRECTION_UNIFORM + 1, light_y);
        let (touch_x, touch_y, touch_intensity) = dynamics.touch.unwrap_or((0.0, 0.0, 0.0));
        shader.set_float(118, touch_x);
        shader.set_float(119, touch_y);
        shader.set_float(120, touch_intensity.clamp(0.0, 1.0));
        shader.set_float(GLASS_EFFECT_DENSITY_UNIFORM, density);
        shader.set_float(
            11,
            (self.highlight + dynamics.highlight_boost).clamp(0.0, 2.0) * activity,
        );
        let dynamic_tint = boost_tint_saturation(self.tint, dynamics.saturation_boost);
        let dynamic_tint_alpha = (dynamic_tint.a()
            * dynamics
                .tint_alpha_multiplier
                .unwrap_or(1.0)
                .clamp(0.0, 2.0)
            * activity)
            .clamp(0.0, 1.0);
        shader.set_float4(
            14,
            dynamic_tint.r(),
            dynamic_tint.g(),
            dynamic_tint.b(),
            dynamic_tint_alpha,
        );
        let saturation = (self.saturation + dynamics.saturation_boost).max(0.0);
        shader.set_float(18, 1.0 + (saturation - 1.0) * activity);
        shader.set_float(20, self.lift * activity);
        shader.set_float(21, 0.5 * activity);
        shader.set_float2(22, 0.0, 1.0);
        shader.set_float(24, 1.0 + (self.contrast - 1.0) * activity);
        shader.set_float(28, self.rim_style * activity);
        let requested_blur_radius_px = if content_mask {
            0.0
        } else {
            self.blur_radius_dp * density * activity
        };
        let (wcksrd_blur_radius, gaussian_blur_radius) =
            if requested_blur_radius_px > WCKSRD_OPTICAL_BLUR_RADIUS_PX {
                (0.0, requested_blur_radius_px)
            } else {
                (requested_blur_radius_px, 0.0)
            };
        shader.set_float(GLASS_BLUR_RADIUS_UNIFORM, wcksrd_blur_radius);
        shader.set_float(GLASS_ACTIVITY_UNIFORM, activity);
        let resting_tint = dynamics.resting_tint.unwrap_or(Color::TRANSPARENT);
        shader.set_float4(
            GLASS_RESTING_TINT_UNIFORM,
            resting_tint.r(),
            resting_tint.g(),
            resting_tint.b(),
            resting_tint.a(),
        );
        shader.set_float(112, if content_mask { 1.0 } else { 0.0 });
        shader.set_float(GLASS_ADAPTIVE_FROST_UNIFORM, self.adaptive_frost * activity);
        shader.set_float(97, self.foreground_luma);
        let dynamic_shadow = !self.clip && self.shadow;
        shader.set_float(
            102,
            if dynamic_shadow {
                self.shadow_color.a() * 0.55 * activity
            } else {
                0.0
            },
        );
        shader.set_float(103, self.shadow_radius);
        shader.set_float(104, self.shadow_offset_y);
        shader.set_float(105, self.shadow_spread);
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
        shader.set_input_padding(self.input_padding() + morph_pad + wcksrd_blur_radius / density);
        if let Some(morph) = dynamics.morph.as_ref() {
            let shadow_reach = if dynamic_shadow {
                self.shadow_radius + self.shadow_offset_y.abs() + self.shadow_spread.max(0.0)
            } else {
                0.0
            };
            let support = morph_output_support(
                morph,
                activity,
                morph_pad,
                shadow_reach,
                if dynamic_shadow {
                    self.shadow_offset_y.abs()
                } else {
                    0.0
                },
            );
            let (node_w, node_h) = morph.node_size;
            let overhang = (-support.x)
                .max(-support.y)
                .max(support.x + support.width - node_w)
                .max(support.y + support.height - node_h)
                .max(0.0);
            shader.set_output_padding((morph_pad + shadow_reach + 4.0).max(overhang));
            shader.set_output_support(Some(support));
        }

        let optical_effect = liquid_glass_runtime_effect(shader);
        if gaussian_blur_radius > f32::EPSILON {
            RenderEffect::blur_with_edge_treatment(gaussian_blur_radius, TileMode::Mirror)
                .then(optical_effect)
        } else {
            optical_effect
        }
    }

    fn input_padding(&self) -> f32 {
        2.0
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

fn morph_output_support(
    morph: &GlassMorph,
    activity: f32,
    morph_pad: f32,
    shadow_reach: f32,
    shadow_offset: f32,
) -> Rect {
    let (px, py, pw, ph, _) = morph.primary;
    let (along, (ax, ay)) = morph.deformation.map_or((1.0, (1.0, 0.0)), |deformation| {
        (
            1.0 + (deformation.along() - 1.0) * activity,
            deformation.axis(),
        )
    });
    let across = 1.0 / along;
    let smallest_strain = along.min(across);
    let reach = (morph_pad + shadow_reach + 4.0) / smallest_strain;
    let rows = [
        (
            (along * ax * ax + across * ay * ay).abs(),
            ((along - across) * ax * ay).abs(),
        ),
        (
            ((along - across) * ax * ay).abs(),
            (along * ay * ay + across * ax * ax).abs(),
        ),
    ];
    let half = (pw * 0.5 + reach, ph * 0.5 + reach);
    let extent_x = rows[0].0 * half.0 + rows[0].1 * half.1;
    let extent_y = rows[1].0 * half.0 + rows[1].1 * half.1;
    let (mut left, mut top) = (px - extent_x, py - extent_y);
    let (mut right, mut bottom) = (px + extent_x, py + extent_y);
    for (sx, sy, sw, sh, _) in &morph.shapes {
        left = left.min(sx - sw * 0.5 - reach);
        top = top.min(sy - sh * 0.5 - reach);
        right = right.max(sx + sw * 0.5 + reach);
        bottom = bottom.max(sy + sh * 0.5 + reach);
    }
    Rect {
        x: left,
        y: top - shadow_offset,
        width: right - left,
        height: bottom - top + 2.0 * shadow_offset,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light_colors() -> LiquidColors {
        LiquidColors::light(Color::from_rgb_u8(0, 122, 255))
    }

    fn terminal_shader(effect: RenderEffect) -> RuntimeShader {
        match effect {
            RenderEffect::Shader { shader } => shader,
            RenderEffect::Chain { second, .. } => terminal_shader(*second),
            effect => panic!("expected runtime shader, got {effect:?}"),
        }
    }

    #[test]
    fn glass_light_direction_defaults_overhead_and_reaches_the_shader() {
        assert_eq!(glass_light_direction(), (0.0, 1.0));
        let resolved = Glass::regular().resolve(&light_colors());
        let effect = resolved.backdrop_effect(2.0, GlassDynamics::default());
        let shader = terminal_shader(effect);
        let u = shader.uniforms();
        assert_eq!(u[GLASS_LIGHT_DIRECTION_UNIFORM], 0.0);
        assert_eq!(u[GLASS_LIGHT_DIRECTION_UNIFORM + 1], 1.0);

        set_glass_light_direction((1.0, 0.0));
        let effect = resolved.backdrop_effect(2.0, GlassDynamics::default());
        let u_rotated = terminal_shader(effect);
        let u_rotated = u_rotated.uniforms();
        assert_eq!(u_rotated[GLASS_LIGHT_DIRECTION_UNIFORM], 1.0);
        assert_eq!(u_rotated[GLASS_LIGHT_DIRECTION_UNIFORM + 1], 0.0);
        set_glass_light_direction((0.0, 1.0));
    }

    #[test]
    fn liquid_shape_builds_matching_clip_and_layer_shapes() {
        for shape in [
            LiquidShape::Capsule,
            LiquidShape::Circle,
            LiquidShape::RoundedRect(12.0),
        ] {
            assert_eq!(shape.layer_shape(), LayerShape::Rounded(shape.clip_shape()));
        }
    }

    #[test]
    fn glass_shadow_clamps_negative_radius() {
        let shadow = GlassShadow::new(Color::BLACK, -2.0, 3.0, -1.0);
        assert_eq!(shadow.radius, 0.0);
        assert_eq!(shadow.offset_y, 3.0);
        assert_eq!(shadow.spread, -1.0);
    }

    #[test]
    fn incompressible_deformation_normalizes_axis_and_conserves_area() {
        let deformation = GlassDeformation::incompressible((3.0, 4.0), 1.25);
        assert_eq!(deformation.axis(), (0.6, 0.8));
        assert_eq!(deformation.along(), 1.25);
        assert!((deformation.along() * deformation.across() - 1.0).abs() < 1.0e-6);
        assert_eq!(
            GlassDeformation::incompressible((0.0, 0.0), 0.0).axis(),
            (1.0, 0.0)
        );
    }

    #[test]
    fn glass_builders_clamp_physical_inputs() {
        let glass = Glass::lens()
            .shape(LiquidShape::Circle)
            .tint(Color::BLACK)
            .blur_radius(-2.0)
            .saturation(1.2)
            .refraction_depth(3.0)
            .refraction_depth_dp(-3.0)
            .refraction_curve(2.0)
            .dispersion(2.0)
            .transmission_refraction(2.0)
            .meniscus_absorption(2.0)
            .highlight(0.4)
            .lift(-0.2)
            .adaptive_frost(Color::WHITE, 2.0)
            .shadow(false)
            .no_clip();
        assert_eq!(glass.shape, LiquidShape::Circle);
        assert_eq!(glass.tint, Some(Color::BLACK));
        assert_eq!(glass.blur_radius, Some(-2.0));
        assert_eq!(glass.saturation, Some(1.2));
        assert_eq!(glass.refraction_depth, 2.0);
        assert_eq!(glass.refraction_depth_dp, Some(0.0));
        assert_eq!(glass.refraction_curve, 1.0);
        assert_eq!(glass.dispersion, 1.0);
        assert_eq!(glass.transmission_refraction, 1.0);
        assert_eq!(glass.meniscus_absorption, 1.0);
        assert_eq!(glass.highlight, 0.4);
        assert_eq!(glass.lift, Some(-0.2));
        assert_eq!(glass.adaptive_frost, 1.0);
        assert!(!glass.shadow);
        assert!(!glass.clip);
    }

    #[test]
    fn material_variants_resolve_distinct_frost_levels() {
        let regular = Glass::regular().resolve(&light_colors());
        let clear = Glass::clear().resolve(&light_colors());
        let lens = Glass::lens().resolve(&light_colors());
        assert!(regular.blur_radius_dp > clear.blur_radius_dp);
        assert!(clear.blur_radius_dp > lens.blur_radius_dp);
        assert!(regular.saturation > clear.saturation);
        assert_eq!(lens.rim_style, 1.0);
        assert_eq!(regular.refraction_depth, 0.0);
        assert_eq!(regular.transmission_refraction, 0.0);
        assert_eq!(regular.refraction_curve, 0.25);
        assert_eq!(lens.refraction_curve, 1.0);
        assert_eq!(regular.dispersion, 0.0);
        assert_eq!(lens.dispersion, 0.30);
    }

    #[test]
    fn neutral_surface_helpers_follow_foreground_polarity() {
        assert_eq!(
            neutral_surface_tint(Color::BLACK, 0.08, 0.10),
            Color::BLACK.with_alpha(0.08)
        );
        assert_eq!(
            neutral_surface_tint(Color::WHITE, 0.08, 0.10),
            Color::WHITE.with_alpha(0.10)
        );
        assert_eq!(neutral_surface_lift(Color::BLACK, 0.7, -0.3), 0.7);
        assert_eq!(neutral_surface_lift(Color::WHITE, 0.7, -0.3), -0.3);
    }

    #[test]
    fn dynamic_saturation_reaches_the_material_tint() {
        let resting = Color::from_rgb_u8(0, 199, 208);
        let raised = boost_tint_saturation(resting, 0.55);
        assert_eq!(raised.r(), 0.0);
        assert!(raised.g() > resting.g());
        assert!(raised.b() > resting.b());
        assert_eq!(raised.a(), resting.a());
    }

    #[test]
    fn resolved_material_packs_wcksrd_and_dynamic_tint() {
        let resolved = Glass::lens()
            .refraction_depth(0.72)
            .refraction_depth_dp(21.0)
            .refraction_curve(0.8)
            .dispersion(0.42)
            .transmission_refraction(0.35)
            .meniscus_absorption(0.3)
            .blur_radius(3.0)
            .tint(Color::BLACK.with_alpha(0.8))
            .resolve(&light_colors());
        let effect = resolved.backdrop_effect(
            2.0,
            GlassDynamics {
                highlight_boost: 0.2,
                saturation_boost: 0.35,
                tint_alpha_multiplier: Some(0.25),
                ..Default::default()
            },
        );
        let RenderEffect::Chain { first, .. } = &effect else {
            panic!("macroscopic frost must precede the wcKSRD optical pass");
        };
        assert!(matches!(
            first.as_ref(),
            RenderEffect::Blur {
                radius_x: 6.0,
                radius_y: 6.0,
                ..
            }
        ));
        let shader = terminal_shader(effect);
        assert_eq!(shader.uniforms()[9], 0.72);
        assert_eq!(
            shader.uniforms()[GLASS_PHYSICAL_REFRACTION_DEPTH_UNIFORM],
            21.0
        );
        assert_eq!(
            shader.uniforms()[GLASS_PHYSICAL_REFRACTION_DEPTH_ENABLED_UNIFORM],
            1.0
        );
        assert_eq!(shader.uniforms()[GLASS_REFRACTION_CURVE_UNIFORM], 0.8);
        assert_eq!(shader.uniforms()[GLASS_DISPERSION_UNIFORM], 0.42);
        assert_eq!(
            shader.uniforms()[GLASS_TRANSMISSION_REFRACTION_UNIFORM],
            0.35
        );
        assert_eq!(shader.uniforms()[GLASS_MENISCUS_ABSORPTION_UNIFORM], 0.3);
        assert_eq!(shader.uniforms()[11], resolved.highlight + 0.2);
        assert_eq!(shader.uniforms()[18], resolved.saturation + 0.35);
        assert!((shader.uniforms()[17] - 0.2).abs() < 1.0e-6);
        assert_eq!(shader.uniforms()[GLASS_BLUR_RADIUS_UNIFORM], 0.0);
        assert_eq!(shader.uniforms()[GLASS_EFFECT_DENSITY_UNIFORM], 2.0);
    }

    #[test]
    fn heavy_backdrop_blur_rides_the_gaussian_pass_alone() {
        let effect = Glass::regular()
            .resolve(&light_colors())
            .backdrop_effect(3.0, GlassDynamics::default());
        let RenderEffect::Chain { first, .. } = &effect else {
            panic!("heavy blur must ride the separable Gaussian pre-pass");
        };
        assert!(matches!(
            first.as_ref(),
            RenderEffect::Blur {
                radius_x: 24.0,
                radius_y: 24.0,
                ..
            }
        ));
        assert_eq!(
            terminal_shader(effect).uniforms()[GLASS_BLUR_RADIUS_UNIFORM],
            0.0
        );
    }

    #[test]
    fn featherweight_backdrop_blur_stays_in_the_optical_tap() {
        let RenderEffect::Shader { shader } = Glass::regular()
            .blur_radius(0.5)
            .resolve(&light_colors())
            .backdrop_effect(3.0, GlassDynamics::default())
        else {
            panic!("a sub-cap blur must not build a Gaussian chain");
        };
        assert_eq!(shader.uniforms()[GLASS_BLUR_RADIUS_UNIFORM], 1.5);
    }

    #[test]
    fn raised_tint_density_stays_premultiplied_alpha_safe() {
        let effect = Glass::lens()
            .tint(Color::WHITE.with_alpha(0.8))
            .resolve(&light_colors())
            .backdrop_effect(
                1.0,
                GlassDynamics {
                    tint_alpha_multiplier: Some(2.0),
                    ..Default::default()
                },
            );
        assert_eq!(terminal_shader(effect).uniforms()[17], 1.0);
    }

    #[test]
    fn optical_activity_reaches_identity_without_removing_the_glass_geometry() {
        let resolved = Glass::lens()
            .refraction_depth(0.72)
            .refraction_curve(0.8)
            .dispersion(0.42)
            .blur_radius(3.0)
            .saturation(1.4)
            .highlight(0.6)
            .lift(0.2)
            .tint(Color::BLACK.with_alpha(0.8))
            .no_clip()
            .resolve(&light_colors());
        let morph = GlassMorph {
            node_size: (120.0, 72.0),
            primary: (60.0, 36.0, 96.0, 52.0, -1.0),
            wobble_amplitude: 3.0,
            bulge_amplitude: 4.0,
            deformation: Some(GlassDeformation::incompressible((1.0, 0.0), 1.25)),
            ..Default::default()
        };

        let RenderEffect::Shader { shader } = resolved.backdrop_effect(
            2.0,
            GlassDynamics {
                activity: Some(0.0),
                morph: Some(morph),
                ..Default::default()
            },
        ) else {
            panic!("material must resolve to the shared wcKSRD shader");
        };
        let uniforms = shader.uniforms();
        assert_eq!(uniforms[GLASS_ACTIVITY_UNIFORM], 0.0);
        assert_eq!(uniforms[9], 0.0);
        assert_eq!(uniforms[GLASS_REFRACTION_CURVE_UNIFORM], 0.0);
        assert_eq!(uniforms[GLASS_DISPERSION_UNIFORM], 0.0);
        assert_eq!(uniforms[GLASS_TRANSMISSION_REFRACTION_UNIFORM], 0.0);
        assert_eq!(uniforms[11], 0.0);
        assert_eq!(uniforms[17], 0.0);
        assert_eq!(uniforms[18], 1.0);
        assert_eq!(uniforms[20], 0.0);
        assert_eq!(uniforms[21], 0.0);
        assert_eq!(uniforms[24], 1.0);
        assert_eq!(uniforms[28], 0.0);
        assert_eq!(uniforms[GLASS_BLUR_RADIUS_UNIFORM], 0.0);
        assert_eq!(uniforms[91], 0.0);
        assert_eq!(uniforms[102], 0.0);
        assert_eq!(
            &uniforms[GLASS_RESTING_TINT_UNIFORM..GLASS_RESTING_TINT_UNIFORM + 4],
            &[0.0, 0.0, 0.0, 0.0]
        );
        assert_eq!(uniforms[32], 0.0);
        assert_eq!(uniforms[26], 0.0);
        assert_eq!(&uniforms[108..110], &[1.0, 1.0]);
        assert_eq!(&uniforms[2..6], &[60.0, 36.0, 96.0, 52.0]);
    }

    #[test]
    fn resting_surface_tint_survives_zero_optical_activity() {
        let tint = Color::BLACK.with_alpha(0.11);
        let RenderEffect::Shader { shader } = Glass::lens()
            .no_clip()
            .resolve(&light_colors())
            .backdrop_effect(
                1.0,
                GlassDynamics {
                    activity: Some(0.0),
                    resting_tint: Some(tint),
                    ..Default::default()
                },
            )
        else {
            panic!("resting surface must use the shared wcKSRD shader");
        };
        assert_eq!(
            &shader.uniforms()[GLASS_RESTING_TINT_UNIFORM..GLASS_RESTING_TINT_UNIFORM + 4],
            &[tint.r(), tint.g(), tint.b(), tint.a()]
        );
        assert_eq!(shader.uniforms()[GLASS_ACTIVITY_UNIFORM], 0.0);
    }

    #[test]
    fn full_optical_activity_preserves_the_resolved_material() {
        let resolved = Glass::lens()
            .refraction_depth(0.72)
            .refraction_curve(0.8)
            .dispersion(0.42)
            .blur_radius(3.0)
            .resolve(&light_colors());
        let shader = terminal_shader(resolved.backdrop_effect(
            2.0,
            GlassDynamics {
                activity: Some(1.0),
                ..Default::default()
            },
        ));
        let uniforms = shader.uniforms();
        assert_eq!(uniforms[GLASS_ACTIVITY_UNIFORM], 1.0);
        assert_eq!(uniforms[9], 0.72);
        assert_eq!(
            uniforms[GLASS_PHYSICAL_REFRACTION_DEPTH_ENABLED_UNIFORM],
            0.0
        );
        assert_eq!(uniforms[GLASS_REFRACTION_CURVE_UNIFORM], 0.8);
        assert_eq!(uniforms[GLASS_DISPERSION_UNIFORM], 0.42);
        assert_eq!(uniforms[GLASS_TRANSMISSION_REFRACTION_UNIFORM], 1.0);
        assert_eq!(uniforms[GLASS_BLUR_RADIUS_UNIFORM], 0.0);
    }

    #[test]
    fn morph_geometry_and_incompressible_strain_are_packed() {
        let deformation = GlassDeformation::incompressible((0.0, 2.0), 1.25);
        let morph = GlassMorph {
            node_size: (78.0, 59.0),
            primary: (39.0, 29.5, 58.0, 39.0, -1.0),
            shapes: vec![(70.0, 29.5, 40.0, 40.0, -1.0)],
            glue: 8.0,
            wobble_amplitude: 1.0,
            wobble_phase: 0.5,
            bulge_amplitude: 2.0,
            bulge_direction: 0.25,
            ellipse_blend: 0.3,
            deformation: Some(deformation),
            zoom_anchor: (0.0, 0.0),
        };
        let RenderEffect::Shader { shader } = Glass::lens()
            .no_clip()
            .resolve(&light_colors())
            .backdrop_effect(
                1.0,
                GlassDynamics {
                    morph: Some(morph),
                    ..Default::default()
                },
            )
        else {
            panic!("morph must use the shared wcKSRD shader");
        };
        let uniforms = shader.uniforms();
        assert_eq!(&uniforms[0..6], &[78.0, 59.0, 39.0, 29.5, 58.0, 39.0]);
        assert_eq!(uniforms[30], 1.0);
        assert_eq!(&uniforms[106..110], &[0.0, 1.0, 1.25, 0.8]);
        assert_eq!(uniforms[110], 0.3);
        assert!(shader.output_padding() > 0.0);
    }

    #[test]
    fn optical_builders_hold_their_own_legal_range() {
        let glass = Glass::regular()
            .fold_depth(-4.0)
            .optical_zoom(0.5)
            .rim_reflection(9.0);
        assert_eq!(glass.fold_depth, 0.0, "a fold cannot be negative deep");
        assert_eq!(glass.optical_zoom, 1.0, "a lens never shrinks its face");
        assert_eq!(
            glass.rim_reflection, 2.0,
            "the rim tops out at twice the line"
        );

        let glass = Glass::regular()
            .fold_depth(6.0)
            .optical_zoom(1.4)
            .rim_reflection(0.25);
        assert_eq!(glass.fold_depth, 6.0);
        assert_eq!(glass.optical_zoom, 1.4);
        assert_eq!(glass.rim_reflection, 0.25);
    }

    #[test]
    fn ink_recolor_records_its_color_at_a_clamped_strength() {
        let red = Color::from_rgb_u8(255, 0, 0);
        let glass = Glass::regular().ink_recolor(red, 3.0);
        assert_eq!(glass.ink_recolor, Some((red, 1.0)));

        let glass = Glass::regular().ink_recolor(red, -1.0);
        assert_eq!(glass.ink_recolor, Some((red, 0.0)));
        assert_eq!(Glass::regular().ink_recolor, None, "no recolor by default");
    }

    #[test]
    fn a_touched_up_surface_amplifies_what_it_already_is() {
        let dynamics = GlassDynamics::default().touched_up(1.0, None, (10.0, 4.0));
        assert!(
            dynamics.highlight_boost > 0.0,
            "a pressed surface goes brighter"
        );
        assert!(dynamics.saturation_boost > 0.0, "and more saturated");
        assert_eq!(
            dynamics.touch,
            Some((10.0, 4.0, 1.0)),
            "with no finger the glow sits at the surface's own heart"
        );
        assert_eq!(
            dynamics.resting_tint, None,
            "a touch never repaints the surface with another color"
        );
    }

    #[test]
    fn a_touch_glow_follows_the_finger_and_accumulates_to_full() {
        let dynamics = GlassDynamics::default()
            .touched_up(0.5, Some((3.0, 7.0)), (10.0, 4.0))
            .touched_up(0.8, Some((3.0, 7.0)), (10.0, 4.0));
        let (x, y, intensity) = dynamics.touch.expect("a pressed surface glows");
        assert_eq!(
            (x, y),
            (3.0, 7.0),
            "the glow tracks the finger, not the center"
        );
        assert_eq!(intensity, 1.0, "carried press saturates at full");
    }

    #[test]
    fn an_unpressed_surface_is_left_exactly_as_it_was() {
        let resting = GlassDynamics::default();
        let untouched = resting
            .clone()
            .touched_up(0.0, Some((3.0, 7.0)), (10.0, 4.0));
        assert_eq!(untouched.highlight_boost, resting.highlight_boost);
        assert_eq!(untouched.saturation_boost, resting.saturation_boost);
        assert_eq!(untouched.touch, None, "no press, no glow");
    }

    #[test]
    fn an_off_axis_strain_widens_the_support_by_the_affine_rows_times_the_half_extents() {
        let resolved = Glass::regular()
            .shape(LiquidShape::RoundedRect(0.0))
            .blur_radius(0.0)
            .shadow(false)
            .no_clip()
            .resolve(&light_colors());
        let angle = 22.5f32.to_radians();
        let morph = GlassMorph {
            node_size: (600.0, 600.0),
            primary: (300.0, 300.0, 200.0, 200.0, 0.0),
            deformation: Some(GlassDeformation::incompressible(
                (angle.cos(), angle.sin()),
                2.0,
            )),
            ..Default::default()
        };
        let RenderEffect::Shader { shader } = resolved.backdrop_effect(
            1.0,
            GlassDynamics {
                activity: Some(1.0),
                morph: Some(morph),
                ..Default::default()
            },
        ) else {
            panic!("an unblurred glass is one shader");
        };
        let support = shader
            .output_support()
            .expect("a morphing glass declares its support");
        let half_x = 100.0 * (2.0 * angle.cos().powi(2) + 0.5 * angle.sin().powi(2))
            + 100.0 * (1.5 * angle.sin() * angle.cos());
        assert!(
            (half_x - 231.066).abs() < 0.01,
            "the example's half extent is {half_x}"
        );
        assert!(
            support.x <= 300.0 - half_x && support.x + support.width >= 300.0 + half_x,
            "the support {support:?} must hold the strained square's x extent {half_x}"
        );
        let half_y = 100.0 * (1.5 * angle.sin() * angle.cos())
            + 100.0 * (2.0 * angle.sin().powi(2) + 0.5 * angle.cos().powi(2));
        assert!(
            (half_y - 125.0).abs() < 0.01,
            "the example's y half extent is {half_y}"
        );
        assert!(
            support.y <= 300.0 - half_y && support.y + support.height >= 300.0 + half_y,
            "the support {support:?} must hold the strained square's y extent {half_y}"
        );
    }

    #[test]
    fn a_morphing_glass_declares_its_output_support_around_its_shapes_and_their_reach() {
        let resolved = Glass::regular()
            .shape(LiquidShape::RoundedRect(12.0))
            .blur_radius(0.0)
            .shadow(true)
            .no_clip()
            .resolve(&light_colors());
        let morph = GlassMorph {
            node_size: (280.0, 160.0),
            primary: (140.0, 80.0, 120.0, 40.0, -1.0),
            shapes: vec![(230.0, 80.0, 40.0, 40.0, -1.0)],
            glue: 20.0,
            wobble_amplitude: 3.0,
            bulge_amplitude: 8.0,
            deformation: Some(GlassDeformation::incompressible((1.0, 0.0), 1.25)),
            ..Default::default()
        };
        let dynamics = GlassDynamics {
            activity: Some(1.0),
            morph: Some(morph.clone()),
            ..Default::default()
        };
        let RenderEffect::Shader { shader } = resolved.backdrop_effect(2.0, dynamics) else {
            panic!("an unblurred glass is one shader");
        };
        let support = shader
            .output_support()
            .expect("a morphing glass declares its support");
        let shape_reach = (230.0 + 20.0) - (140.0 + 60.0);
        let shadow_reach = resolved.shadow_radius
            + resolved.shadow_offset_y.abs()
            + resolved.shadow_spread.max(0.0);
        let field_reach = 3.0 * 2.0 + 8.0 + shape_reach + 40.0 + shadow_reach + 4.0;
        let smallest_strain = 0.8;
        let reach = field_reach / smallest_strain;
        let contains = |x: f32, y: f32| {
            support.x <= x
                && support.y <= y
                && support.x + support.width >= x
                && support.y + support.height >= y
        };
        for (cx, cy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            let corner = ((60.0 + reach) * cx, (20.0 + reach) * cy);
            let stretched = (140.0 + corner.0 * 1.25, 80.0 + corner.1 * 0.8);
            assert!(
                contains(stretched.0, stretched.1 - resolved.shadow_offset_y.abs())
                    && contains(stretched.0, stretched.1 + resolved.shadow_offset_y.abs()),
                "{support:?} must hold the strained, dilated corner {stretched:?}"
            );
        }
        assert!(
            contains(230.0 + 20.0 + reach, 80.0),
            "the glued shape's reach is inside"
        );
        assert!(shader.output_padding() >= field_reach);

        let RenderEffect::Shader { shader } =
            resolved.backdrop_effect(2.0, GlassDynamics::default())
        else {
            panic!("an unblurred glass is one shader");
        };
        assert_eq!(
            shader.output_support(),
            None,
            "cover-mode glass fills its rect"
        );
    }
}
