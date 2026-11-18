// The menu's morph over its open and close: where the glass shape stands,
// how it bulges and joins its source, and how its rows fade and scale in.

use cranpose_ui_graphics::{Point, Rect};

// Whether the menu grows out of its anchor or shrinks back into it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MenuMotion {
    Opening,
    Closing,
}

impl MenuMotion {
    pub(super) fn of(expanded: bool) -> Self {
        if expanded {
            Self::Opening
        } else {
            Self::Closing
        }
    }
}

pub(super) const MENU_GROW_DELAY: f32 = 0.050;
const MENU_SOURCE_SEPARATE_END: f32 = 0.06;
const MENU_CARD_WIDTH_GROW_START: f32 = 0.16;
const MENU_CARD_HEIGHT_GROW_START: f32 = 0.12;
const MENU_OVERSHOOT_SCALE: f32 = 0.30;
const MENU_WIDTH_EASE_POWER: f32 = 4.5;
const MENU_HEIGHT_EASE_POWER: f32 = 18.0;
const MENU_HEIGHT_OVERSHOOT: f32 = 0.15;
const MENU_HEIGHT_OVERSHOOT_END: f32 = 0.52;
const MENU_VERTICAL_REBOUND: f32 = 18.0;
pub(super) const MENU_VERTICAL_REBOUND_END: f32 = 0.70;
const MENU_SOURCE_HEIGHT_RATIO: f32 = 0.86;
const MENU_SOURCE_TARGET_Y_PROGRESS: f32 = 0.0;
const MENU_CONTENT_BLUR: f32 = 14.0;
const MENU_CONTENT_BLUR_POWER: f32 = 0.65;
const MENU_CONTENT_ALPHA_POWER: f32 = 0.45;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct MenuGeometryPhase {
    pub(super) path: f32,
    pub(super) width: f32,
    pub(super) height: f32,
}

pub(super) fn menu_geometry_phase(motion: MenuMotion, appear: f32) -> MenuGeometryPhase {
    let expanded = motion == MenuMotion::Opening;
    if expanded && appear > 1.0 {
        let settle = 1.0 + (appear - 1.0) * MENU_OVERSHOOT_SCALE;
        return MenuGeometryPhase {
            path: settle,
            width: settle,
            height: settle,
        };
    }

    let appear = appear.clamp(0.0, 1.0);
    if !expanded {
        let normalized = ((appear - 0.015) / 0.985).clamp(0.0, 1.0);
        return MenuGeometryPhase {
            path: normalized,
            width: 1.0 - (1.0 - normalized).powf(2.5),
            height: 1.0 - (1.0 - normalized).powf(14.0),
        };
    }

    if appear < MENU_GROW_DELAY {
        let source_merge = smoothstep(0.10, 0.58, appear / MENU_GROW_DELAY);
        return MenuGeometryPhase {
            path: 0.0,
            width: source_merge,
            height: source_merge,
        };
    }

    let path = ((appear - MENU_GROW_DELAY) / (1.0 - MENU_GROW_DELAY)).clamp(0.0, 1.0);
    let width_growth =
        ((path - MENU_CARD_WIDTH_GROW_START) / (1.0 - MENU_CARD_WIDTH_GROW_START)).clamp(0.0, 1.0);
    let height_growth = ((path - MENU_CARD_HEIGHT_GROW_START)
        / (1.0 - MENU_CARD_HEIGHT_GROW_START))
        .clamp(0.0, 1.0);
    let overshoot_phase = ((path - 0.50) / 0.50).clamp(0.0, 1.0);
    let overshoot = 0.040 * (std::f32::consts::PI * overshoot_phase).sin().max(0.0);
    let height_overshoot_phase = (height_growth / MENU_HEIGHT_OVERSHOOT_END).clamp(0.0, 1.0);
    let height_overshoot = MENU_HEIGHT_OVERSHOOT
        * (std::f32::consts::PI * height_overshoot_phase)
            .sin()
            .max(0.0);
    MenuGeometryPhase {
        path,
        width: 1.0 - (1.0 - width_growth).powf(MENU_WIDTH_EASE_POWER) + overshoot,
        height: 1.0 - (1.0 - height_growth).powf(MENU_HEIGHT_EASE_POWER) + height_overshoot,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct MenuShape {
    pub(super) center_x: f32,
    pub(super) center_y: f32,
    pub(super) width: f32,
    pub(super) height: f32,
    pub(super) radius: f32,
}

impl MenuShape {
    pub(super) fn capsule(center_x: f32, center_y: f32, width: f32, height: f32) -> Self {
        Self {
            center_x,
            center_y,
            width,
            height,
            radius: -1.0,
        }
    }

    pub(super) fn from_window_rect(rect: Rect, node_origin: Point) -> Option<Self> {
        (rect.width > 0.0 && rect.height > 0.0).then(|| {
            Self::capsule(
                rect.x + rect.width * 0.5 - node_origin.x,
                rect.y + rect.height * 0.5 - node_origin.y,
                rect.width,
                rect.height,
            )
        })
    }

    pub(super) fn as_glass_shape(self) -> (f32, f32, f32, f32, f32) {
        (
            self.center_x,
            self.center_y,
            self.width,
            self.height,
            self.radius,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct MenuMorphGeometry {
    pub(super) primary: MenuShape,
    pub(super) source: MenuShape,
    pub(super) target: MenuShape,
    pub(super) path: f32,
}

pub(super) fn menu_source_shape(
    anchor: MenuShape,
    absorbed: &[MenuShape],
    target: MenuShape,
) -> MenuShape {
    let mut left = anchor.center_x - anchor.width * 0.5;
    let mut right = anchor.center_x + anchor.width * 0.5;
    let mut top = anchor.center_y - anchor.height * 0.5;
    let mut bottom = anchor.center_y + anchor.height * 0.5;
    for shape in absorbed {
        left = left.min(shape.center_x - shape.width * 0.5);
        right = right.max(shape.center_x + shape.width * 0.5);
        top = top.min(shape.center_y - shape.height * 0.5);
        bottom = bottom.max(shape.center_y + shape.height * 0.5);
    }

    let width = right - left;
    let cluster_height = bottom - top;
    let height = cluster_height
        .max(width * MENU_SOURCE_HEIGHT_RATIO)
        .min(target.height);
    let cluster_center_y = (top + bottom) * 0.5;
    MenuShape::capsule(
        (left + right) * 0.5,
        cluster_center_y + (target.center_y - cluster_center_y) * MENU_SOURCE_TARGET_Y_PROGRESS,
        width,
        height,
    )
}

pub(super) fn interpolate_menu_shape(
    start: MenuShape,
    target: MenuShape,
    width_progress: f32,
    height_progress: f32,
) -> MenuShape {
    let lerp = |a: f32, b: f32, progress: f32| a + (b - a) * progress;
    MenuShape::capsule(
        lerp(start.center_x, target.center_x, width_progress),
        lerp(start.center_y, target.center_y, height_progress),
        lerp(start.width, target.width, width_progress),
        lerp(start.height, target.height, height_progress),
    )
}

pub(super) fn menu_vertical_rebound(path: f32) -> f32 {
    if !(0.0..MENU_VERTICAL_REBOUND_END).contains(&path) {
        return 0.0;
    }

    let normalized = path / MENU_VERTICAL_REBOUND_END;
    let onset = smoothstep(0.0, 0.012, path);
    MENU_VERTICAL_REBOUND
        * onset
        * (std::f32::consts::PI * normalized.powf(0.45))
            .sin()
            .max(0.0)
}

pub(super) fn menu_morph_geometry(
    motion: MenuMotion,
    appear: f32,
    anchor: MenuShape,
    absorbed: &[MenuShape],
    target: MenuShape,
) -> MenuMorphGeometry {
    let expanded = motion == MenuMotion::Opening;
    let phase = menu_geometry_phase(motion, appear);
    let source = menu_source_shape(anchor, absorbed, target);
    let mut primary = if expanded && phase.path < MENU_SOURCE_SEPARATE_END {
        anchor
    } else {
        let start = if expanded { source } else { anchor };
        interpolate_menu_shape(start, target, phase.width, phase.height)
    };
    if expanded && phase.path >= MENU_SOURCE_SEPARATE_END {
        let descent = smoothstep(0.10, 0.90, phase.path);
        primary.center_y = source.center_y + (target.center_y - source.center_y) * descent;
        primary.center_y +=
            (target.center_y - source.center_y).signum() * menu_vertical_rebound(phase.path);
    }
    let blob_radius = primary.height * 0.5;
    let squareness = smoothstep(0.55, 0.88, phase.path);
    primary.radius = if !expanded {
        blob_radius
    } else if phase.path >= 1.0 {
        target.radius
    } else {
        blob_radius + (target.radius - blob_radius) * squareness
    };
    MenuMorphGeometry {
        primary,
        source,
        target,
        path: phase.path,
    }
}

pub(super) fn menu_ellipse_blend(path: f32) -> f32 {
    0.5 * smoothstep(0.06, 0.22, path) * (1.0 - smoothstep(0.62, 0.88, path))
}

pub(super) fn menu_content_progress(motion: MenuMotion, appear: f32, reveal: f32) -> f32 {
    let expanded = motion == MenuMotion::Opening;
    if expanded {
        smoothstep(0.17, 0.82, reveal)
    } else {
        smoothstep(0.20, 0.75, appear)
    }
}

pub(super) fn menu_content_blur(progress: f32) -> f32 {
    MENU_CONTENT_BLUR * (1.0 - progress.clamp(0.0, 1.0)).powf(MENU_CONTENT_BLUR_POWER)
}

pub(super) fn menu_content_alpha(progress: f32) -> f32 {
    progress.clamp(0.0, 1.0).powf(MENU_CONTENT_ALPHA_POWER)
}

pub(super) fn menu_content_scale(progress: f32) -> f32 {
    0.80 + 0.20 * progress.clamp(0.0, 1.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct MenuAbsorbedVisualPhase {
    pub(super) foreground_alpha: f32,
    pub(super) backdrop_alpha: f32,
    pub(super) foreground_blur: f32,
    pub(super) scale_x: f32,
    pub(super) scale_y: f32,
}

pub(super) fn menu_absorbed_visual_phase(appear: f32, path: f32) -> MenuAbsorbedVisualPhase {
    let appear = appear.clamp(0.0, 1.0);
    let path = path.clamp(0.0, 1.0);
    let shrink = smoothstep(0.0, 0.24, path);
    let base_scale = 1.0 - 0.25 * shrink;
    let stretch = smoothstep(0.30, 0.56, path);
    let handoff = smoothstep(0.30, 0.55, appear);
    let readable_alpha = 1.0 + (0.40 - 1.0) * handoff;
    MenuAbsorbedVisualPhase {
        foreground_alpha: readable_alpha * (1.0 - smoothstep(0.45, 0.85, path)),
        backdrop_alpha: 0.62 * smoothstep(0.45, 0.85, path),
        foreground_blur: 7.0 * smoothstep(0.45, 0.85, path),
        scale_x: base_scale * (1.0 + 0.20 * stretch),
        scale_y: base_scale * (1.0 + 0.28 * stretch),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct MenuSurfacePhase {
    pub(super) anchor_presence: f32,
    pub(super) glue: f32,
    pub(super) wobble: f32,
    pub(super) bulge: f32,
}

pub(super) fn menu_surface_phase(motion: MenuMotion, appear: f32, path: f32) -> MenuSurfacePhase {
    let expanded = motion == MenuMotion::Opening;
    let appear = appear.clamp(0.0, 1.0);
    let path = path.clamp(0.0, 1.0);
    let activity = (std::f32::consts::PI * path).sin().max(0.0);
    if expanded && path <= f32::EPSILON {
        let recoil = (appear / MENU_GROW_DELAY).clamp(0.0, 1.0);
        return MenuSurfacePhase {
            anchor_presence: 0.0,
            glue: 0.0,
            wobble: 0.18 * (std::f32::consts::PI * recoil).sin().max(0.0),
            bulge: 0.0,
        };
    }
    if expanded {
        return MenuSurfacePhase {
            anchor_presence: 0.0,
            glue: 0.0,
            wobble: 0.08 * activity,
            bulge: 0.35 * activity,
        };
    }
    MenuSurfacePhase {
        anchor_presence: 0.0,
        glue: 0.0,
        wobble: 0.04 * activity,
        bulge: 0.25 * activity,
    }
}

pub(super) fn menu_absorbed_shape_presence(path: f32) -> f32 {
    1.0 - smoothstep(MENU_SOURCE_SEPARATE_END, 0.30, path)
}

pub(super) fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
