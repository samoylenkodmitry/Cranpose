// Which content snaps to the pixel grid as one rigid block, and where.

use cranpose_render_common::{
    layer_transform::apply_layer_affine_to_rect,
    scene_store::{LayerRef, RecordKind},
};
use cranpose_ui_graphics::{GraphicsLayer, Point, Rect};

use super::{AFFINE_TOLERANCE, direct_translation};
use crate::scene::SnapAnchor;

fn graphics_layer_is_rigid(layer: &GraphicsLayer) -> bool {
    (layer.scale - 1.0).abs() <= AFFINE_TOLERANCE
        && (layer.scale_x - 1.0).abs() <= AFFINE_TOLERANCE
        && (layer.scale_y - 1.0).abs() <= AFFINE_TOLERANCE
        && layer.rotation_x.abs() <= AFFINE_TOLERANCE
        && layer.rotation_y.abs() <= AFFINE_TOLERANCE
        && layer.rotation_z.abs() <= AFFINE_TOLERANCE
}

pub(super) fn rigid_snap_anchor(layer_bounds: Rect, layer: &GraphicsLayer) -> Option<SnapAnchor> {
    if !graphics_layer_is_rigid(layer) {
        return None;
    }
    let mapped = apply_layer_affine_to_rect(layer_bounds, layer_bounds, layer);
    Some(SnapAnchor::rigid(Point::new(mapped.x, mapped.y)))
}

pub(super) fn layer_has_pixel_sensitive_subtree(layer: LayerRef<'_>) -> bool {
    layer.own_draw_facts().pixel_sensitive || layer.children().any(child_is_pixel_sensitive)
}

// Whether `child` draws text or an image its parent moves with it: a leaf,
// or a layer placed by a translation alone.
fn child_is_pixel_sensitive(child: LayerRef<'_>) -> bool {
    match child.kind() {
        RecordKind::Leaf => child.own_draw_facts().pixel_sensitive,
        RecordKind::Layer | RecordKind::Wrapper | RecordKind::DrawGroup => {
            direct_translation(child.transform_to_parent()).is_some()
                && layer_has_pixel_sensitive_subtree(child)
        }
    }
}
