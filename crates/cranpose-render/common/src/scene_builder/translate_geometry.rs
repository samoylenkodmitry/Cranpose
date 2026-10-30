use cranpose_ui::{Point, Rect};
use cranpose_ui_graphics::GraphicsLayer;

use super::AbsOrigin;
use crate::{graph::ProjectiveTransform, layer_transform::layer_transform_to_window};

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
