use cranpose_render_common::graph::{LayerNode, ProjectiveTransform};
use cranpose_ui_graphics::Rect;

use super::{ContentStyle, RasterTraversalContext, populate_draws_from_graph};
use crate::scene::{RasterScene, Subtree};

/// Draws `surfaces`, which the eye of a scene sees, each placed by its
/// parent map, with the scene layer's content style and clip.
#[cold]
#[inline(never)]
pub(super) fn push_seen_surfaces(
    surfaces: Vec<(&LayerNode, ProjectiveTransform)>,
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

/// The surfaces the eye of the scene `layer` shows sees in the space it
/// stands in, far ones first, each with the map its layer's parent would
/// take for the layer to land where the camera shows it, given `transform`
/// from `layer` to device pixels; `None` when `layer` shows no scene.
#[cold]
#[inline(never)]
pub(super) fn seen_surfaces<'a>(
    layer: &'a LayerNode,
    transform: &ProjectiveTransform,
) -> Option<Vec<(&'a LayerNode, ProjectiveTransform)>> {
    use cranpose_render_common::space::{EyeView, SpaceTree, node_size};
    let tree = SpaceTree::of(layer)?;
    let view = EyeView::of(&tree, node_size(layer));
    let rect = layer.node_rect();
    let to_device = ProjectiveTransform::translation(rect.x, rect.y).then(*transform);
    let mut surfaces: Vec<(f32, &'a LayerNode, ProjectiveTransform)> = view
        .surfaces
        .iter()
        .filter(|seen| seen.seen == 0)
        .filter_map(|seen| {
            let content = seen
                .surface
                .content_transform(&view.camera, view.size, seen.size)?;
            let parent = seen
                .layer
                .transform_to_parent
                .inverse()?
                .then(content)
                .then(to_device);
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
