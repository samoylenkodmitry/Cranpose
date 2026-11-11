use cranpose_render_common::{graph::ProjectiveTransform, scene_store::LayerRef};
use cranpose_ui_graphics::Rect;

use super::{ContentStyle, RasterTraversalContext, populate_draws_from_graph};
use crate::scene::{RasterScene, Subtree};

#[cold]
#[inline(never)]
pub(super) fn push_seen_surfaces(
    surfaces: Vec<(LayerRef<'_>, ProjectiveTransform)>,
    scene: &mut RasterScene,
    context: &RasterTraversalContext<'_>,
    (content_style, visual_clip): (&ContentStyle, Option<Rect>),
) {
    for (surface, parent_transform) in surfaces {
        populate_draws_from_graph(
            Subtree::Layer(surface),
            scene,
            &RasterTraversalContext {
                parent_transform: &parent_transform,
                parent_content_style: content_style,
                parent_visual_clip: visual_clip,
                inherited_translated_snap_anchor: None,
                inherited_translated_content_context: false,
                diagnostics: context.diagnostics,
                light: context.light,
                same_as_before: false,
                text: context.text,
                surface_root: false,
                in_space: true,
            },
            &mut None,
        );
    }
}

#[cold]
#[inline(never)]
pub(super) fn seen_surfaces<'a>(
    layer: LayerRef<'a>,
    transform: &ProjectiveTransform,
) -> Option<Vec<(LayerRef<'a>, ProjectiveTransform)>> {
    use cranpose_render_common::space::{EyeView, SpaceTree};
    let tree = SpaceTree::of(layer)?;
    // The scene fills its layer's bounds.
    let bounds = layer.local_bounds();
    let view = EyeView::of(&tree, bounds.size());
    let to_device = ProjectiveTransform::translation(bounds.x, bounds.y).then(*transform);
    let mut surfaces: Vec<(f32, LayerRef<'a>, ProjectiveTransform)> = view
        .surfaces
        .iter()
        .filter(|seen| seen.seen == 0)
        .filter_map(|seen| {
            let parent = seen
                .layer
                .transform_to_parent()
                .inverse()?
                .then(view.content_to_window(seen, to_device)?);
            let (low, high) = seen.surface.mesh.bounds();
            let middle = seen.surface.transform.transform_point(low.lerp(high, 0.5));
            let depth = view.camera.view.transform_point(middle).z;
            Some((depth, seen.layer, parent))
        })
        .collect();
    surfaces.sort_by(|a, b| b.0.total_cmp(&a.0));
    Some(
        surfaces
            .into_iter()
            .map(|(_, layer, parent)| (layer, parent))
            .collect(),
    )
}
