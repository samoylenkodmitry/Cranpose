use crate::effect_renderer::CompositeSampleMode;

pub(crate) const MOTION_STABLE_SURFACE_SCALE_MULTIPLIER: f32 = 9.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SurfaceRequirement {
    ExplicitOffscreen,
    RenderEffect,
    Backdrop,
    GroupOpacity,
    BlendMode,
    ShapeClip,
    ImmediateShadow,
    TextMaterialMask,
    MotionStableCapture,
    NonTranslationTransform,
    MixedDirectContent,
    PixelStableComposite,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct SurfaceRequirementSet {
    bits: u16,
}

/// Quantisation ladder for a magnifying layer's surface density: quarter
/// steps. `ScaleBucket` (~0.004 wide) exists to absorb float noise, not
/// animation, so an animated or pinched scale walked straight through it and
/// allocated a fresh texture every frame.
const LAYER_SCALE_QUANTUM: f32 = 4.0;

/// The share of a layer's uniform scale that may raise its surface density.
/// Garbage values fall back to `1.0`, and minification never lowers it.
///
/// The result is rounded UP to the next [`LAYER_SCALE_QUANTUM`] step, so an
/// animating or pinched scale holds one surface across a range of frames
/// instead of reallocating per frame, and the density is never below the
/// layer's effective scale (rounding down would reintroduce the softness this
/// whole path exists to remove). The overshoot is at most one quarter step,
/// and `clamp_effect_surface_scale` still bounds the result against the
/// texture-dimension limit and the per-surface byte budget.
fn magnifying_layer_scale(layer_scale: f32) -> f32 {
    if !layer_scale.is_finite() || layer_scale <= 1.0 {
        return 1.0;
    }
    (layer_scale * LAYER_SCALE_QUANTUM).ceil() / LAYER_SCALE_QUANTUM
}

impl SurfaceRequirementSet {
    const EXPLICIT_OFFSCREEN: u16 = 1 << 0;
    const RENDER_EFFECT: u16 = 1 << 1;
    const BACKDROP: u16 = 1 << 2;
    const GROUP_OPACITY: u16 = 1 << 3;
    const BLEND_MODE: u16 = 1 << 4;
    const SHAPE_CLIP: u16 = 1 << 5;
    const IMMEDIATE_SHADOW: u16 = 1 << 6;
    const TEXT_MATERIAL_MASK: u16 = 1 << 7;
    const MOTION_STABLE_CAPTURE: u16 = 1 << 8;
    const NON_TRANSLATION_TRANSFORM: u16 = 1 << 9;
    const MIXED_DIRECT_CONTENT: u16 = 1 << 10;
    const PIXEL_STABLE_COMPOSITE: u16 = 1 << 11;

    pub(crate) fn insert(&mut self, requirement: SurfaceRequirement) {
        self.bits |= Self::bit(requirement);
    }

    pub(crate) fn with(mut self, requirement: SurfaceRequirement) -> Self {
        self.insert(requirement);
        self
    }

    pub(crate) fn contains(self, requirement: SurfaceRequirement) -> bool {
        (self.bits & Self::bit(requirement)) != 0
    }

    pub(crate) fn has_isolating_requirement(self) -> bool {
        (self.bits
            & !(Self::MIXED_DIRECT_CONTENT | Self::IMMEDIATE_SHADOW | Self::PIXEL_STABLE_COMPOSITE))
            != 0
    }

    pub(crate) fn has_renderer_forced_surface(self) -> bool {
        self.contains(SurfaceRequirement::TextMaterialMask)
            || self.contains(SurfaceRequirement::NonTranslationTransform)
    }

    pub(crate) fn composite_requires_resampling(self) -> bool {
        self.contains(SurfaceRequirement::NonTranslationTransform)
            || !self.composite_preserves_raster_content()
    }

    pub(crate) fn composite_preserves_raster_content(self) -> bool {
        self.contains(SurfaceRequirement::MotionStableCapture)
            || self.contains(SurfaceRequirement::PixelStableComposite)
            || self.contains(SurfaceRequirement::ExplicitOffscreen)
            || self.contains(SurfaceRequirement::RenderEffect)
            || self.contains(SurfaceRequirement::Backdrop)
            || self.contains(SurfaceRequirement::GroupOpacity)
            || self.contains(SurfaceRequirement::BlendMode)
            || self.contains(SurfaceRequirement::ShapeClip)
    }

    #[cfg(test)]
    pub(crate) fn labels(self) -> impl Iterator<Item = &'static str> {
        const ORDERED: &[(SurfaceRequirement, &str)] = &[
            (SurfaceRequirement::ExplicitOffscreen, "explicit_offscreen"),
            (SurfaceRequirement::RenderEffect, "render_effect"),
            (SurfaceRequirement::Backdrop, "backdrop"),
            (SurfaceRequirement::GroupOpacity, "group_opacity"),
            (SurfaceRequirement::BlendMode, "blend_mode"),
            (SurfaceRequirement::ShapeClip, "shape_clip"),
            (SurfaceRequirement::ImmediateShadow, "immediate_shadow"),
            (SurfaceRequirement::TextMaterialMask, "text_material_mask"),
            (
                SurfaceRequirement::MotionStableCapture,
                "motion_stable_capture",
            ),
            (
                SurfaceRequirement::NonTranslationTransform,
                "non_translation_transform",
            ),
            (
                SurfaceRequirement::MixedDirectContent,
                "mixed_direct_content",
            ),
            (
                SurfaceRequirement::PixelStableComposite,
                "pixel_stable_composite",
            ),
        ];

        ORDERED
            .iter()
            .filter(move |(requirement, _)| self.contains(*requirement))
            .map(|(_, label)| *label)
    }

    #[cfg(test)]
    pub(crate) fn display(self) -> String {
        let mut joined = String::new();
        for (index, label) in self.labels().enumerate() {
            if index > 0 {
                joined.push('+');
            }
            joined.push_str(label);
        }
        if joined.is_empty() {
            joined.push_str("none");
        }
        joined
    }

    pub(crate) fn composite_sample_mode(self) -> CompositeSampleMode {
        if self.composite_requires_resampling() {
            CompositeSampleMode::Linear
        } else {
            CompositeSampleMode::Box4
        }
    }

    /// Device-pixel density the surface texture must be rasterized at.
    ///
    /// `layer_scale` is the uniform scale of the layer transform the composite
    /// will map this surface through (see
    /// `cranpose_render_common::layer_transform::layer_uniform_scale`). A forced
    /// surface renders the layer's LOCAL, untransformed rect, so rasterizing a
    /// magnifying layer at plain `root_scale` and then blowing the texture up
    /// through the composite's sampler is precisely why zoomed content never
    /// resolves more detail. A [`SurfaceRequirement::NonTranslationTransform`]
    /// therefore resolves at the layer's *effective* device scale.
    ///
    /// Minification is deliberately not followed downward: `layer_scale` only
    /// ever raises the density, because shrinking the texture would discard
    /// detail an interactive scale may zoom straight back into.
    ///
    /// This is the *desired* scale. Callers still clamp it against the
    /// texture-dimension limit and the `MAX_EFFECT_LAYER_SURFACE_BYTES` budget
    /// via `clamp_effect_surface_scale`, which only clamps downward.
    pub(crate) fn target_scale(self, root_scale: f32, layer_scale: f32) -> f32 {
        if self.contains(SurfaceRequirement::MotionStableCapture) {
            root_scale * MOTION_STABLE_SURFACE_SCALE_MULTIPLIER
        } else if self.contains(SurfaceRequirement::NonTranslationTransform) {
            root_scale * magnifying_layer_scale(layer_scale)
        } else {
            root_scale
        }
    }

    fn bit(requirement: SurfaceRequirement) -> u16 {
        match requirement {
            SurfaceRequirement::ExplicitOffscreen => Self::EXPLICIT_OFFSCREEN,
            SurfaceRequirement::RenderEffect => Self::RENDER_EFFECT,
            SurfaceRequirement::Backdrop => Self::BACKDROP,
            SurfaceRequirement::GroupOpacity => Self::GROUP_OPACITY,
            SurfaceRequirement::BlendMode => Self::BLEND_MODE,
            SurfaceRequirement::ShapeClip => Self::SHAPE_CLIP,
            SurfaceRequirement::ImmediateShadow => Self::IMMEDIATE_SHADOW,
            SurfaceRequirement::TextMaterialMask => Self::TEXT_MATERIAL_MASK,
            SurfaceRequirement::MotionStableCapture => Self::MOTION_STABLE_CAPTURE,
            SurfaceRequirement::NonTranslationTransform => Self::NON_TRANSLATION_TRANSFORM,
            SurfaceRequirement::MixedDirectContent => Self::MIXED_DIRECT_CONTENT,
            SurfaceRequirement::PixelStableComposite => Self::PIXEL_STABLE_COMPOSITE,
        }
    }
}

impl FromIterator<SurfaceRequirement> for SurfaceRequirementSet {
    fn from_iter<T: IntoIterator<Item = SurfaceRequirement>>(iter: T) -> Self {
        let mut requirements = Self::default();
        for requirement in iter {
            requirements.insert(requirement);
        }
        requirements
    }
}

#[cfg(test)]
mod tests {
    use super::{SurfaceRequirement, SurfaceRequirementSet};
    use crate::effect_renderer::CompositeSampleMode;

    #[test]
    fn labels_and_display_follow_requirement_order() {
        let requirements = SurfaceRequirementSet::default()
            .with(SurfaceRequirement::ImmediateShadow)
            .with(SurfaceRequirement::MixedDirectContent);

        assert_eq!(
            requirements.labels().collect::<Vec<_>>(),
            vec!["immediate_shadow", "mixed_direct_content"]
        );
        assert_eq!(
            requirements.display(),
            "immediate_shadow+mixed_direct_content"
        );
    }

    #[test]
    fn display_reports_none_for_empty_set() {
        assert_eq!(SurfaceRequirementSet::default().display(), "none");
    }

    #[test]
    fn immediate_shadow_is_ordered_draw_work_not_layer_isolation() {
        let requirements =
            SurfaceRequirementSet::default().with(SurfaceRequirement::ImmediateShadow);

        assert!(!requirements.has_isolating_requirement());
        assert!(!requirements.has_renderer_forced_surface());
    }

    #[test]
    fn pixel_stable_composite_uses_box4_without_forcing_surface() {
        let requirements =
            SurfaceRequirementSet::default().with(SurfaceRequirement::PixelStableComposite);

        assert!(!requirements.has_isolating_requirement());
        assert!(!requirements.has_renderer_forced_surface());
        assert_eq!(
            requirements.composite_sample_mode(),
            CompositeSampleMode::Box4
        );
        assert_eq!(requirements.target_scale(3.0, 1.0), 3.0);
    }

    /// Bug: a magnifying layer's forced surface was rasterized at plain
    /// `root_scale` and then blown up by the composite's linear sampler, so zoom
    /// never resolved more detail at any scale.
    #[test]
    fn non_translation_transform_resolves_at_the_layer_scale() {
        let requirements = SurfaceRequirementSet::default()
            .with(SurfaceRequirement::ExplicitOffscreen)
            .with(SurfaceRequirement::NonTranslationTransform);

        assert_eq!(
            requirements.target_scale(3.0, 4.0),
            12.0,
            "a 4x layer on a 3x screen needs 12x device density, not 3x"
        );
    }

    #[test]
    fn minified_layer_keeps_the_root_scale_density() {
        let requirements = SurfaceRequirementSet::default()
            .with(SurfaceRequirement::ExplicitOffscreen)
            .with(SurfaceRequirement::NonTranslationTransform);

        assert_eq!(
            requirements.target_scale(3.0, 0.25),
            3.0,
            "shrinking the texture would throw away detail an interactive scale zooms back into"
        );
    }

    #[test]
    fn non_finite_layer_scale_falls_back_to_the_root_scale() {
        let requirements =
            SurfaceRequirementSet::default().with(SurfaceRequirement::NonTranslationTransform);

        assert_eq!(requirements.target_scale(2.0, f32::NAN), 2.0);
        assert_eq!(requirements.target_scale(2.0, f32::INFINITY), 2.0);
    }

    #[test]
    fn translation_only_surfaces_ignore_the_layer_scale() {
        let requirements =
            SurfaceRequirementSet::default().with(SurfaceRequirement::ExplicitOffscreen);

        assert_eq!(
            requirements.target_scale(2.0, 4.0),
            2.0,
            "without a scaling transform the composite is 1:1 and the extra density is waste"
        );
    }

    /// Regression: an ANIMATING scale walked straight through `ScaleBucket`'s
    /// ~0.004 float-noise quantisation and produced a distinct surface size
    /// every frame, so the layer's antialiasing was recomputed at a new
    /// sub-pixel phase per frame. Neighbouring frames must land on one scale.
    #[test]
    fn neighbouring_animation_frames_share_one_target_scale() {
        let requirements =
            SurfaceRequirementSet::default().with(SurfaceRequirement::NonTranslationTransform);

        // The demo's "Scale + Fade" lambda sweeps 0.85 -> 1.15.
        let scales: Vec<f32> = (0..=40)
            .map(|frame| 0.85 + 0.3 * (frame as f32 / 40.0))
            .collect();
        let mut distinct: Vec<f32> = scales
            .iter()
            .map(|scale| requirements.target_scale(1.0, *scale))
            .collect();
        distinct.dedup();

        assert_eq!(
            distinct,
            vec![1.0, 1.25],
            "a 41-frame sweep must resolve to two densities, not 41"
        );
    }

    /// The quantisation may only ever round UP: rounding down would put the
    /// texture below the layer's effective device scale and hand back exactly
    /// the softness this path exists to remove.
    #[test]
    fn quantised_target_scale_is_never_below_the_effective_scale() {
        let requirements =
            SurfaceRequirementSet::default().with(SurfaceRequirement::NonTranslationTransform);

        for step in 0..=64 {
            let layer_scale = 1.0 + step as f32 * 0.0625;
            let target_scale = requirements.target_scale(2.0, layer_scale);
            assert!(
                target_scale >= 2.0 * layer_scale - 1e-4,
                "layer_scale={layer_scale} resolved at {target_scale}, below its own density"
            );
            assert!(
                target_scale <= 2.0 * (layer_scale + 0.25),
                "layer_scale={layer_scale} overshot a quarter step at {target_scale}"
            );
        }
    }

    /// Whole and quarter scales are already on the ladder, so the density they
    /// ask for is the density they get.
    #[test]
    fn scales_on_the_quantisation_ladder_are_exact() {
        let requirements =
            SurfaceRequirementSet::default().with(SurfaceRequirement::NonTranslationTransform);

        assert_eq!(requirements.target_scale(3.0, 4.0), 12.0);
        assert_eq!(requirements.target_scale(3.0, 2.0), 6.0);
        assert_eq!(requirements.target_scale(2.0, 1.25), 2.5);
        assert_eq!(requirements.target_scale(2.0, 1.0), 2.0);
    }

    #[test]
    fn translation_only_layer_surfaces_composite_without_resampling() {
        for requirement in [
            SurfaceRequirement::ExplicitOffscreen,
            SurfaceRequirement::RenderEffect,
            SurfaceRequirement::Backdrop,
            SurfaceRequirement::GroupOpacity,
            SurfaceRequirement::BlendMode,
            SurfaceRequirement::MotionStableCapture,
        ] {
            let requirements = SurfaceRequirementSet::default().with(requirement);

            assert_eq!(
                requirements.composite_sample_mode(),
                CompositeSampleMode::Box4,
                "{requirement:?} should preserve pixels when the layer transform is pure translation"
            );
            assert!(!requirements.composite_requires_resampling());
        }
    }

    #[test]
    fn text_material_mask_intermediate_uses_linear_sampling() {
        let requirements =
            SurfaceRequirementSet::default().with(SurfaceRequirement::TextMaterialMask);

        assert_eq!(
            requirements.composite_sample_mode(),
            CompositeSampleMode::Linear
        );
        assert!(requirements.composite_requires_resampling());
    }

    #[test]
    fn motion_stable_text_material_mask_uses_box4_sampling() {
        let requirements = SurfaceRequirementSet::default()
            .with(SurfaceRequirement::TextMaterialMask)
            .with(SurfaceRequirement::MotionStableCapture);

        assert_eq!(
            requirements.composite_sample_mode(),
            CompositeSampleMode::Box4
        );
        assert!(!requirements.composite_requires_resampling());
    }

    #[test]
    fn non_translation_transform_uses_linear_sampling() {
        let requirements = SurfaceRequirementSet::default()
            .with(SurfaceRequirement::ExplicitOffscreen)
            .with(SurfaceRequirement::NonTranslationTransform);

        assert_eq!(
            requirements.composite_sample_mode(),
            CompositeSampleMode::Linear
        );
        assert!(requirements.composite_requires_resampling());
    }
}
