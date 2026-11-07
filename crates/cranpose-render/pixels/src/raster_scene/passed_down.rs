use cranpose_render_common::graph::ProjectiveTransform;
use cranpose_ui_graphics::{ColorFilter, Point, Rect};

/// What a layer passes down to the walks of its child layers.
#[derive(Clone, PartialEq)]
pub(crate) struct PassedDown {
    pub transform: ProjectiveTransform,
    pub alpha: f32,
    pub color_filter: Option<ColorFilter>,
    pub clip: Option<Rect>,
    pub snap_anchor: Option<Point>,
    pub translated: bool,
}

impl PassedDown {
    // What a window's root passes down at `scale`: its map to device pixels,
    // and no style, clip or snap anchor.
    pub(crate) fn root(scale: f32) -> Self {
        Self {
            transform: ProjectiveTransform::uniform_scale(scale),
            alpha: 1.0,
            color_filter: None,
            clip: None,
            snap_anchor: None,
            translated: false,
        }
    }
}
