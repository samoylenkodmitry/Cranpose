// A child whose backdrop must read what lies beneath it in the child's own
// space, under a transform or a composite that is not plain.

use cranpose_render_common::graph::ProjectiveTransform;
use cranpose_ui_graphics::BlendMode;

use super::{ChildLayer, LayerSceneRecycler, direct_translation, uniform_scale_translation};
use crate::scene::{BackdropLayer, CompositorScene, LayerRoundedClip, SceneCapacityHint};

pub(super) fn with_backdrop_in_own_space(
    mut child: ChildLayer,
    recycler: &mut LayerSceneRecycler,
) -> ChildLayer {
    if child.backdrop.is_none()
        || (uniform_scale_translation(child.transform).is_some()
            && child.alpha == 1.0
            && child.blend_mode == BlendMode::SrcOver)
    {
        return child;
    }
    let content = recycler.take(SceneCapacityHint::default());
    let mut outer = ChildLayer {
        z_index: child.z_index,
        node_id: child.node_id,
        local_bounds: child.local_bounds,
        transform: child.transform,
        clip: child.clip,
        rounded_clip: None,
        alpha: child.alpha,
        blend_mode: child.blend_mode,
        effect: None,
        backdrop: None,
        backdrop_alpha: 1.0,
        snap_anchor: child.snap_anchor,
        surface_scale: child.surface_scale,
        content_hash: child.content_hash,
        cache_policy: child.cache_policy,
        in_place: false,
        content,
        mesh: child.mesh.take(),
    };
    let mut inner = ChildLayer {
        z_index: outer.content.scene.next_z(),
        transform: ProjectiveTransform::identity(),
        clip: None,
        alpha: 1.0,
        blend_mode: BlendMode::SrcOver,
        snap_anchor: None,
        surface_scale: 1.0,
        in_place: false,
        ..child
    };
    detach_flat_backdrop(&mut inner, &mut outer.content.scene);
    outer.content.scene.next_z += 1;
    outer.content.children.push(inner);
    outer.content.refresh_backdrop_summary();
    outer
}

pub(super) fn detach_flat_backdrop(child: &mut ChildLayer, scene: &mut CompositorScene) {
    if child.alpha != 1.0
        || child.blend_mode != BlendMode::SrcOver
        || child.content.contains_backdrop()
    {
        return;
    }
    let Some(offset) = direct_translation(child.transform) else {
        return;
    };
    let Some(effect) = child.backdrop.take() else {
        return;
    };
    scene.push_backdrop_layer(BackdropLayer {
        node_id: child.node_id,
        alpha: child.backdrop_alpha,
        rect: child.local_bounds.translate(offset.x, offset.y),
        clip: child.clip,
        reach: None,
        rounded_clip: child.rounded_clip.map(|clip| LayerRoundedClip {
            rect: clip.rect.translate(offset.x, offset.y),
            radii: clip.radii,
        }),
        snap_anchor: child.snap_anchor,
        effect,
        z_index: 0,
    });
    child.z_index = scene.next_z();
}
