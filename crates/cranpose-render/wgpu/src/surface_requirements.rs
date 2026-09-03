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

const LAYER_SCALE_QUANTUM: f32 = 4.0;

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

    /// Whether the only isolating requirement is the backdrop itself, so the
    /// layer's own content could render into its parent once the backdrop
    /// effect has been composited there: no group opacity, blend mode,
    /// render effect, explicit offscreen, text mask or resampling transform.
    /// A shape clip is allowed because the caller proves the content stays
    /// inside the shape.
    pub(crate) fn isolates_only_for_backdrop(self) -> bool {
        self.contains(SurfaceRequirement::Backdrop)
            && (self.bits
                & !(Self::BACKDROP
                    | Self::SHAPE_CLIP
                    | Self::MIXED_DIRECT_CONTENT
                    | Self::IMMEDIATE_SHADOW
                    | Self::PIXEL_STABLE_COMPOSITE))
                == 0
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

    #[test]
    fn neighbouring_animation_frames_share_one_target_scale() {
        let requirements =
            SurfaceRequirementSet::default().with(SurfaceRequirement::NonTranslationTransform);

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
