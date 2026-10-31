// The children of a layer that shows a scene or lies in a scene's space,
// and the surfaces the scene's eye sees.

use std::sync::Arc;

use cranpose_render_common::{
    graph::{LayerNode, RenderNode, for_each_in_draw_order},
    space::{EyeView, SpaceTree, node_size},
};

use super::{
    LayerMotion, LayerScene, LayerSceneRecycler, MeshChild, SceneDraw, SeenDraw, WalkContext,
    WalkSettings, collect_child, collect_inline, collect_isolated_child, node_is_pixel_sensitive,
    push_content,
};
use crate::scene::ContentContext;

/// The scene `layer` shows to the window, as the eye of the scene sees it,
/// with `context` the walk of its children; `None` when it shows none.
fn window_scene<'a>(
    layer: &'a LayerNode,
    context: &WalkContext<'_>,
) -> Option<(Arc<SceneDraw>, EyeView<'a>)> {
    let tree = SpaceTree::of(layer)?;
    let view = EyeView::of(&tree, node_size(layer));
    let scene = SceneDraw {
        camera: view.camera,
        light: view.seen[0].light,
        rect: layer
            .node_rect()
            .translate(context.offset.x, context.offset.y),
        clip: context.visual_clip,
        seen: view
            .seen
            .iter()
            .map(|seen| SeenDraw {
                through: seen.through,
                light: seen.light,
                door: seen.door,
            })
            .collect(),
    };
    Some((Arc::new(scene), view))
}

/// Collects the children of a layer that shows a scene or lies in a
/// scene's space, under `context`. The layers in 3D there are the scene's:
/// its surfaces draw together where its first child in 3D comes, flat
/// children before it under them and the rest over them. Kept out of line:
/// a flat layer's walk stops at the check before it.
#[cold]
#[inline(never)]
pub(super) fn collect_space_children(
    layer: &LayerNode,
    motion: &mut LayerMotion,
    (context, content): (WalkContext<'_>, &ContentContext<'_>),
    out: &mut LayerScene,
    recycler: &mut LayerSceneRecycler,
) -> bool {
    // Only a scene outside every other scene's space shows to the window;
    // one inside is a doorway its window scene draws.
    let mut pending = (!context.settings.in_space)
        .then(|| window_scene(layer, &context))
        .flatten();
    let in_space = context.settings.in_space || pending.is_some();
    let settings = WalkSettings {
        in_space,
        ..*context.settings
    };
    let context = WalkContext {
        settings: &settings,
        ..context
    };
    let mut has_pixel_sensitive_subtree = false;
    for_each_in_draw_order(&layer.children, |child| {
        has_pixel_sensitive_subtree |= match child {
            RenderNode::Layer(child_layer) => {
                if let Some((draw, view)) = pending.take_if(|_| child_layer.has_space()) {
                    collect_seen_surfaces(&draw, view, motion, context, out, recycler);
                }
                !(in_space && child_layer.graphics_layer.space.is_some())
                    && collect_child(child_layer, motion, context, out, recycler)
            }
            RenderNode::Inline(inline) => collect_inline(inline, context, out),
            RenderNode::Primitive(_) | RenderNode::DrawRun(_) => {
                push_content(out, child, content);
                node_is_pixel_sensitive(child)
            }
        };
    });
    has_pixel_sensitive_subtree
}

/// Collects each surface the eye of the scene `draw` sees, for its 3D pass,
/// placed where the eye sees it.
fn collect_seen_surfaces(
    draw: &Arc<SceneDraw>,
    view: EyeView<'_>,
    motion: &mut LayerMotion,
    context: WalkContext<'_>,
    out: &mut LayerScene,
    recycler: &mut LayerSceneRecycler,
) {
    for seen in view.surfaces {
        let mesh = MeshChild {
            scene: Arc::clone(draw),
            surface: seen.surface,
            content: seen.layer.node_rect(),
            seen: seen.seen,
        };
        collect_isolated_child(
            seen.layer,
            motion,
            context,
            Some(Arc::new(mesh)),
            out,
            recycler,
        );
    }
}
