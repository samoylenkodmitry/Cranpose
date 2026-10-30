use cranpose_core::MemoryApplier;
use cranpose_ui::{Point, Rect};
use cranpose_ui_graphics::GraphicsLayer;

use super::{AbsOrigin, placed_transform, publish_retained_window_geometry};
use crate::{
    graph::{LayerNode, ProjectiveTransform},
    layer_transform::layer_transform_to_window,
};

#[derive(Clone, Copy)]
pub(super) struct TranslateGeometry {
    pub(super) content_offset: Point,
    pub(super) window_transform: ProjectiveTransform,
    pub(super) top_left: Point,
    child_origin: Point,
}

impl TranslateGeometry {
    pub(super) fn new(
        layout_state: &cranpose_ui::widgets::LayoutState,
        graphics_layer: &GraphicsLayer,
        local_bounds: Rect,
        parent_abs: AbsOrigin,
    ) -> Self {
        let content_offset = layout_state.content_offset();
        let top_left = Point {
            x: parent_abs.content_origin.x + layout_state.position().x,
            y: parent_abs.content_origin.y + layout_state.position().y,
        };
        let window_transform = layer_transform_to_window(
            parent_abs.window_transform,
            top_left,
            local_bounds,
            graphics_layer,
        );
        Self {
            content_offset,
            window_transform,
            top_left,
            child_origin: Point {
                x: top_left.x + content_offset.x,
                y: top_left.y + content_offset.y,
            },
        }
    }

    pub(super) fn child_abs(self) -> AbsOrigin {
        AbsOrigin {
            content_origin: self.child_origin,
            window_transform: self.window_transform,
        }
    }
}

pub(super) fn translate_retained_child(
    layer: &mut LayerNode,
    position: Point,
    content_offset: Point,
) {
    layer.transform_to_parent = placed_transform(
        layer.local_bounds,
        position,
        layer.graphics_layer.get(),
        content_offset,
    );
    layer.origin_in_parent = position;
}

/// Moves a child a translated container keeps to its placement in the
/// container laid out with `geometry`. A subtree that publishes window
/// origins publishes them again from where it now lies.
pub(super) fn move_kept_child(
    applier: &MemoryApplier,
    layer: &mut LayerNode,
    position: Point,
    geometry: TranslateGeometry,
) {
    translate_retained_child(layer, position, geometry.content_offset);
    if layer.has_origin_sinks {
        publish_retained_window_geometry(applier, layer, geometry.child_abs());
    }
}
