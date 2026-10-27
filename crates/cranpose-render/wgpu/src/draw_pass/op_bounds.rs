use cranpose_ui_graphics::Rect;

use crate::{
    render::{image_draw_bounds, run_draw_bounds, text_draw_bounds},
    rrect_shadow::rrect_shadow_bounds,
    scene::{CompositorScene, DrawOp, DrawOpKind},
};

/// The logical rect `op` may draw into: a shape, image or text by its
/// snapped bounds within its clip, an unblurred shadow by the union of its
/// parts. A blurred shadow draws nothing itself (it resolves to a
/// composite).
pub(crate) fn op_draw_bounds(
    scene: &CompositorScene,
    op: &DrawOp,
    root_scale: f32,
) -> Option<Rect> {
    match op.kind {
        DrawOpKind::Run(index) => run_draw_bounds(&scene.runs[index], root_scale),
        DrawOpKind::Image(index) => image_draw_bounds(&scene.images[index], root_scale),
        DrawOpKind::Text(index) => text_draw_bounds(&scene.texts[index], root_scale),
        DrawOpKind::RRectShadow(index) => {
            rrect_shadow_bounds(&scene.rrect_shadows[index], root_scale)
        }
        DrawOpKind::Shadow(index) => {
            let shadow = &scene.shadow_draws[index];
            if shadow.requires_surface() {
                return None;
            }
            shadow_caster_bounds(shadow, root_scale)
                .into_iter()
                .chain(
                    shadow
                        .texts
                        .iter()
                        .filter_map(|text| text_draw_bounds(text, root_scale)),
                )
                .reduce(|a, b| a.union(b))
        }
    }
}

/// The snapped bounds of an unblurred shadow's casters, within the
/// shadow's clip.
fn shadow_caster_bounds(shadow: &crate::scene::ShadowDraw, root_scale: f32) -> Option<Rect> {
    shadow
        .shapes
        .as_ref()
        .and_then(|run| run_draw_bounds(run, root_scale))
}

/// Whether `op` draws any pixel inside `viewport_rect`.
pub(crate) fn op_is_visible_in_rect(
    scene: &CompositorScene,
    op: &DrawOp,
    viewport_rect: Rect,
    root_scale: f32,
) -> bool {
    op_draw_bounds(scene, op, root_scale)
        .is_some_and(|bounds| bounds.intersect(viewport_rect).is_some())
}
