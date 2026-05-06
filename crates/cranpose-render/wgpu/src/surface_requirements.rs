use crate::effect_renderer::CompositeSampleMode;

pub(crate) const MOTION_STABLE_SURFACE_SCALE_MULTIPLIER: f32 = 9.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SurfaceRequirement {
    ExplicitOffscreen,
    RenderEffect,
    Backdrop,
    GroupOpacity,
    BlendMode,
    ImmediateShadow,
    TextMaterialMask,
    MotionStableCapture,
    NonTranslationTransform,
    MixedDirectContent,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct SurfaceRequirementSet {
    bits: u16,
}

impl SurfaceRequirementSet {
    const EXPLICIT_OFFSCREEN: u16 = 1 << 0;
    const RENDER_EFFECT: u16 = 1 << 1;
    const BACKDROP: u16 = 1 << 2;
    const GROUP_OPACITY: u16 = 1 << 3;
    const BLEND_MODE: u16 = 1 << 4;
    const IMMEDIATE_SHADOW: u16 = 1 << 5;
    const TEXT_MATERIAL_MASK: u16 = 1 << 6;
    const MOTION_STABLE_CAPTURE: u16 = 1 << 7;
    const NON_TRANSLATION_TRANSFORM: u16 = 1 << 8;
    const MIXED_DIRECT_CONTENT: u16 = 1 << 9;

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

    #[cfg(test)]
    pub(crate) fn has_any(self) -> bool {
        self.bits != 0
    }

    pub(crate) fn has_isolating_requirement(self) -> bool {
        (self.bits & !(Self::MIXED_DIRECT_CONTENT | Self::IMMEDIATE_SHADOW)) != 0
    }

    pub(crate) fn has_renderer_forced_surface(self) -> bool {
        self.contains(SurfaceRequirement::TextMaterialMask)
            || self.contains(SurfaceRequirement::NonTranslationTransform)
    }

    #[cfg(test)]
    pub(crate) fn labels(self) -> impl Iterator<Item = &'static str> {
        const ORDERED: &[(SurfaceRequirement, &str)] = &[
            (SurfaceRequirement::ExplicitOffscreen, "explicit_offscreen"),
            (SurfaceRequirement::RenderEffect, "render_effect"),
            (SurfaceRequirement::Backdrop, "backdrop"),
            (SurfaceRequirement::GroupOpacity, "group_opacity"),
            (SurfaceRequirement::BlendMode, "blend_mode"),
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
        if self.contains(SurfaceRequirement::MotionStableCapture) {
            CompositeSampleMode::Box4
        } else {
            CompositeSampleMode::Linear
        }
    }

    pub(crate) fn target_scale(self, root_scale: f32) -> f32 {
        if self.contains(SurfaceRequirement::MotionStableCapture) {
            root_scale * MOTION_STABLE_SURFACE_SCALE_MULTIPLIER
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
            SurfaceRequirement::ImmediateShadow => Self::IMMEDIATE_SHADOW,
            SurfaceRequirement::TextMaterialMask => Self::TEXT_MATERIAL_MASK,
            SurfaceRequirement::MotionStableCapture => Self::MOTION_STABLE_CAPTURE,
            SurfaceRequirement::NonTranslationTransform => Self::NON_TRANSLATION_TRANSFORM,
            SurfaceRequirement::MixedDirectContent => Self::MIXED_DIRECT_CONTENT,
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
}
