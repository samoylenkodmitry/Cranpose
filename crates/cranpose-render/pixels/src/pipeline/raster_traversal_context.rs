// What the walk above a layer passes to the pushes of the layer's items and
// subtree.

use cranpose_render_common::{
    graph::ProjectiveTransform, graph_scene::RenderDiagnostics, layer_shadow::ShadowLight,
};
use cranpose_ui_graphics::{Point, Rect};

use super::ContentStyle;
use crate::draw::TextRaster;

pub(crate) struct RasterTraversalContext<'a> {
    pub(super) parent_transform: &'a ProjectiveTransform,
    pub(super) parent_content_style: &'a ContentStyle,
    pub(super) parent_visual_clip: Option<Rect>,
    pub(super) inherited_translated_snap_anchor: Option<Point>,
    pub(super) inherited_translated_content_context: bool,
    pub(super) diagnostics: &'a RenderDiagnostics,
    /// The window's shadow light in device pixels, the space the raster
    /// scene is drawn in.
    pub(super) light: ShadowLight,
    /// Whether everything above is what the walk passed down to the layer's
    /// old record in the frame before's scene.
    pub(crate) same_as_before: bool,
    /// The fonts and glyphs a surface's text is drawn with.
    pub(super) text: &'a TextRaster,
    /// Whether the layer is the one whose surface the walk draws: it draws
    /// its items straight into the surface, and its parent drew its shadow.
    pub(super) surface_root: bool,
    /// Whether the walk lies in a scene's space, whose scene draws the
    /// mesh surfaces and 3D transforms there, as its eye sees them.
    pub(super) in_space: bool,
}

impl<'a> RasterTraversalContext<'a> {
    // What a window's root layer pushes under: `transform` maps the window
    // to device pixels, with no style, clip or snap anchor above it, and no
    // scene before it.
    pub(super) fn root(
        transform: &'a ProjectiveTransform,
        (diagnostics, text): (&'a RenderDiagnostics, &'a TextRaster),
        light: ShadowLight,
    ) -> Self {
        Self {
            parent_transform: transform,
            parent_content_style: &ContentStyle::PLAIN,
            parent_visual_clip: None,
            inherited_translated_snap_anchor: None,
            inherited_translated_content_context: false,
            diagnostics,
            light,
            same_as_before: false,
            text,
            surface_root: false,
            in_space: false,
        }
    }
}
