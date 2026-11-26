//! The variants of the pipelines of an app's shader: a shaded surface's and a volume's.

use cranpose_ui_graphics::space::Multisample;

use super::{Blend, Depth, PipelineSpec, ScenePipeline, Stencil};

/// Which of a shaded surface's shader's pipelines a draw takes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShadedVariant {
    /// The surfaces of a scene's own space without shadows.
    Plain,
    /// The surfaces of a scene's own space, which read its shadow map.
    Shadowed,
    /// The surfaces in a portal's space, inside its window.
    Portal,
    // The glowing surfaces of a scene's own space, added onto the page.
    Glow,
    // The glowing surfaces in a portal's space, added inside its window.
    PortalGlow,
}

impl ShadedVariant {
    // The fixed pipeline whose targets the variant takes. A surface of a
    // variant that hides what lies behind it draws with it until its
    // shader builds; a glowing one draws nothing until then.
    pub(crate) fn lit(self) -> ScenePipeline {
        match self {
            Self::Plain => ScenePipeline::Plain,
            Self::Shadowed => ScenePipeline::Shadowed,
            Self::Portal => ScenePipeline::Portal,
            Self::Glow => ScenePipeline::Glow,
            Self::PortalGlow => ScenePipeline::PortalGlow,
        }
    }

    // Whether the variant adds its colour onto the page, hiding nothing.
    pub(crate) fn adds(self) -> bool {
        matches!(self, Self::Glow | Self::PortalGlow)
    }

    // The fixed pipeline's targets, with the shader's own entry points.
    pub(super) fn spec(self, samples: Multisample) -> PipelineSpec {
        let fragment = match self {
            Self::Plain | Self::Glow => "mesh_shaded_fs",
            Self::Shadowed => "mesh_shaded_shadowed_fs",
            Self::Portal | Self::PortalGlow => "mesh_shaded_portal_fs",
        };
        PipelineSpec {
            label: "Scene Shaded Surface Pipeline",
            entry_points: ("mesh_shaded_vs", fragment),
            ..self.lit().spec(samples)
        }
    }
}

/// Which of a volume shader's pipelines a draw takes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum VolumeVariant {
    /// Draws the volumes of the scene's own space.
    Color,
    /// Draws only their depth, for the pass glass draws in.
    Depth,
    /// Draws volumes in a portal's space, inside its window.
    Portal,
    /// Draws only the depth of volumes in a portal's space, for the pass
    /// glass draws in.
    PortalDepth,
}

impl VolumeVariant {
    pub(super) fn spec(self, samples: Multisample) -> PipelineSpec {
        let (label, fragment, color, stencil) = match self {
            Self::Color => (
                "Scene Volume Pipeline",
                "mesh_volume_fs",
                Some(Blend::Over),
                Stencil::Ignored,
            ),
            Self::Depth => (
                "Scene Volume Depth Pipeline",
                "mesh_volume_fs",
                None,
                Stencil::Ignored,
            ),
            Self::Portal => (
                "Scene Portal Volume Pipeline",
                "mesh_volume_portal_fs",
                Some(Blend::Over),
                Stencil::Inside,
            ),
            Self::PortalDepth => (
                "Scene Portal Volume Depth Pipeline",
                "mesh_volume_portal_fs",
                None,
                Stencil::Inside,
            ),
        };
        PipelineSpec {
            label,
            entry_points: ("mesh_volume_vs", fragment),
            color,
            depth: Depth::Tested,
            stencil,
            samples,
        }
    }
}
