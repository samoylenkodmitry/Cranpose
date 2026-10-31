// Which content snaps to the pixel grid as one rigid block, and where.

use cranpose_render_common::{
    graph::{LayerNode, PrimitiveNode, RenderNode},
    layer_transform::apply_layer_affine_to_rect,
};
use cranpose_ui_graphics::{DrawPrimitive, GraphicsLayer, Point, Rect};

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

pub(super) fn primitive_is_pixel_sensitive(primitive: &DrawPrimitive) -> bool {
    match primitive {
        DrawPrimitive::Blend { primitive, .. } => primitive_is_pixel_sensitive(primitive),
        DrawPrimitive::Image { .. } | DrawPrimitive::Text(_) => true,
        _ => false,
    }
}

pub(super) fn primitive_is_drawn(primitive: &DrawPrimitive) -> bool {
    !matches!(primitive, DrawPrimitive::Content | DrawPrimitive::Shadow(_))
}

pub(super) fn layer_has_pixel_sensitive_subtree(layer: &LayerNode) -> bool {
    layer.children.iter().any(node_is_pixel_sensitive)
}

pub(super) fn node_is_pixel_sensitive(node: &RenderNode) -> bool {
    match node {
        RenderNode::Primitive(entry) => match &entry.node {
            PrimitiveNode::Text(_) => true,
            PrimitiveNode::Draw(draw) => primitive_is_pixel_sensitive(&draw.primitive),
        },
        RenderNode::DrawRun(run) => run.summary.has_text || run.summary.has_pixel_sensitive,
        RenderNode::Layer(child) => {
            direct_translation(child.transform_to_parent).is_some()
                && layer_has_pixel_sensitive_subtree(child)
        }
    }
}
