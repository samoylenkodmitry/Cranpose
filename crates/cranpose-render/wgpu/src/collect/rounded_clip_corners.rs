use cranpose_render_common::{
    graph::{DrawRunNode, LayerNode, PrimitiveNode, RenderNode},
    primitive_emit::resolve_clip,
};
use cranpose_ui_graphics::{
    DrawPrimitive, LayerShape, Point, RecordLane, Rect, RenderEffect, ShadowPrimitive, expand_rect,
    primitive_coverage_rect,
};

use super::{AFFINE_TOLERANCE, RasterScale, child_needs_surface, direct_translation};
use crate::scene::LayerRoundedClip;

pub(super) fn rounded_clip_for_layer(layer: &LayerNode) -> Option<LayerRoundedClip> {
    if !layer.graphics_layer.clip {
        return None;
    }
    let LayerShape::Rounded(shape) = layer.graphics_layer.shape else {
        return None;
    };
    let radii = shape.resolve(layer.local_bounds.width, layer.local_bounds.height);
    let radii = [
        radii.top_left,
        radii.top_right,
        radii.bottom_left,
        radii.bottom_right,
    ];
    if radii.iter().all(|radius| *radius <= f32::EPSILON) {
        return None;
    }
    Some(LayerRoundedClip {
        rect: layer.local_bounds,
        radii,
    })
}

/// The four corner squares of a rounded rect are the only places a rounded
/// clip differs from its rect clip. Content whose coverage stays inside the
/// corner circles there is clipped identically by both.
pub(super) struct RoundedClipCorners {
    rect: Rect,
    radii: [f32; 4],
    raster_scale: RasterScale,
    aa_margin: f32,
}

impl RoundedClipCorners {
    pub(super) fn of(clip: LayerRoundedClip, raster_scale: RasterScale) -> Self {
        Self {
            rect: clip.rect,
            radii: clip.radii,
            raster_scale,
            aa_margin: raster_scale.aa_margin(),
        }
    }

    /// Whether `region` lies inside the rounded rect: for every corner square
    /// it enters, its point farthest from that corner's circle centre is still
    /// within the circle.
    pub(super) fn admits(&self, region: Rect) -> bool {
        if ![region.x, region.y, region.width, region.height]
            .into_iter()
            .all(f32::is_finite)
        {
            return false;
        }
        let Rect {
            x,
            y,
            width,
            height,
        } = self.rect;
        let right = x + width;
        let bottom = y + height;
        let corners = [
            (self.radii[0], x, y, 1.0, 1.0),
            (self.radii[1], right, y, -1.0, 1.0),
            (self.radii[2], x, bottom, 1.0, -1.0),
            (self.radii[3], right, bottom, -1.0, -1.0),
        ];
        let region_right = region.x + region.width;
        let region_bottom = region.y + region.height;
        for (radius, corner_x, corner_y, sign_x, sign_y) in corners {
            if radius <= 0.0 {
                continue;
            }
            let square_left = corner_x.min(corner_x + sign_x * radius);
            let square_right = corner_x.max(corner_x + sign_x * radius);
            let square_top = corner_y.min(corner_y + sign_y * radius);
            let square_bottom = corner_y.max(corner_y + sign_y * radius);
            let overlap_left = region.x.max(square_left);
            let overlap_right = region_right.min(square_right);
            let overlap_top = region.y.max(square_top);
            let overlap_bottom = region_bottom.min(square_bottom);
            if overlap_left >= overlap_right || overlap_top >= overlap_bottom {
                continue;
            }
            let centre_x = corner_x + sign_x * radius;
            let centre_y = corner_y + sign_y * radius;
            let farthest_x = if sign_x > 0.0 {
                overlap_left
            } else {
                overlap_right
            };
            let farthest_y = if sign_y > 0.0 {
                overlap_top
            } else {
                overlap_bottom
            };
            let dx = farthest_x - centre_x;
            let dy = farthest_y - centre_y;
            if dx * dx + dy * dy > radius * radius {
                return false;
            }
        }
        true
    }
}

pub(super) fn content_admits_rounded_clip(
    layer: &LayerNode,
    clip: LayerRoundedClip,
    raster_scale: RasterScale,
) -> bool {
    let corners = RoundedClipCorners::of(clip, raster_scale);
    layer_admits_corners(layer, Point::default(), None, &corners)
}

fn layer_admits_corners(
    layer: &LayerNode,
    offset: Point,
    clip: Option<Rect>,
    corners: &RoundedClipCorners,
) -> bool {
    let inherited_clip = resolve_clip(
        clip,
        layer
            .visual_clip_rect()
            .map(|rect| rect.translate(offset.x, offset.y)),
    );
    let check = |rect: Rect, primitive_clip: Option<Rect>| -> bool {
        let rect = rect.translate(offset.x, offset.y);
        let clip = resolve_clip(
            inherited_clip,
            primitive_clip.map(|clip| clip.translate(offset.x, offset.y)),
        );
        let visible = match clip {
            Some(clip) => rect.intersect(clip),
            None => Some(rect),
        };
        visible.is_none_or(|visible| corners.admits(expand_rect(visible, corners.aa_margin)))
    };
    layer
        .children
        .iter()
        .all(|child| node_admits_corners(child, offset, inherited_clip, corners, &check))
}

fn node_admits_corners(
    node: &RenderNode,
    offset: Point,
    inherited_clip: Option<Rect>,
    corners: &RoundedClipCorners,
    check: &impl Fn(Rect, Option<Rect>) -> bool,
) -> bool {
    match node {
        RenderNode::Primitive(entry) => match &entry.node {
            PrimitiveNode::Draw(draw) => {
                primitive_stays_clear(&draw.primitive, corners.raster_scale, |rect| {
                    check(rect, draw.clip)
                })
            }
            PrimitiveNode::Text(text) => text.draw_bounds().is_some_and(|rect| check(rect, None)),
        },
        RenderNode::DrawRun(run) => {
            run.coverage_rects().all(|rect| check(rect, None))
                && (!run.summary.has_shadow
                    || run_others_stay_clear(run, corners.raster_scale, |rect| check(rect, None)))
        }
        RenderNode::Layer(child) => {
            if child.graphics_layer.shadow_elevation > 0.0 {
                return false;
            }
            let Some(translation) = direct_translation(child.transform_to_parent) else {
                return child_stays_clear(child, offset, corners);
            };
            if child_needs_surface(child) {
                return child_stays_clear(child, offset, corners);
            }
            let child_offset = Point::new(offset.x + translation.x, offset.y + translation.y);
            layer_admits_corners(child, child_offset, inherited_clip, corners)
        }
    }
}

/// The radius a layer whose rounded clip cuts into its content draws in
/// place with, its records taking the clip's coverage as its surface's
/// composite would: a uniform one, with nothing in the corners but shapes
/// and images drawn straight under that clip. Text, shadows, layers that
/// composite and anything under a clip of its own stay out of the corners,
/// which the rect clip alone then leaves whole.
pub(super) fn content_takes_rounded_clip(
    layer: &LayerNode,
    clip: LayerRoundedClip,
    raster_scale: RasterScale,
) -> Option<f32> {
    let radius = clip.radii[0];
    let uniform = clip
        .radii
        .iter()
        .all(|corner| (corner - radius).abs() <= AFFINE_TOLERANCE);
    (uniform
        && layer.visual_clip_rect() == Some(clip.rect)
        && layer_takes_corners(
            layer,
            Point::default(),
            &RoundedClipCorners::of(clip, raster_scale),
        ))
    .then_some(radius)
}

fn layer_takes_corners(layer: &LayerNode, offset: Point, corners: &RoundedClipCorners) -> bool {
    layer.children.iter().all(|node| match node {
        RenderNode::Primitive(entry) => match &entry.node {
            PrimitiveNode::Draw(draw)
                if draw.clip.is_none() && takes_rounded_clip(&draw.primitive) =>
            {
                true
            }
            PrimitiveNode::Draw(draw) => {
                primitive_stays_clear(&draw.primitive, corners.raster_scale, |rect| {
                    stays_clear(rect, offset, corners)
                })
            }
            PrimitiveNode::Text(text) => text
                .draw_bounds()
                .is_some_and(|rect| stays_clear(rect, offset, corners)),
        },
        RenderNode::DrawRun(run) => run_takes_corners(run, offset, corners),
        RenderNode::Layer(child) => child_takes_corners(child, offset, corners),
    })
}

fn child_takes_corners(child: &LayerNode, offset: Point, corners: &RoundedClipCorners) -> bool {
    let plain = child.visual_clip_rect().is_none()
        && rounded_clip_for_layer(child).is_none()
        && child.graphics_layer.shadow_elevation <= 0.0
        && !child_needs_surface(child);
    match direct_translation(child.transform_to_parent) {
        Some(translation) if plain => layer_takes_corners(
            child,
            Point::new(offset.x + translation.x, offset.y + translation.y),
            corners,
        ),
        _ => child_stays_clear(child, offset, corners),
    }
}

fn child_stays_clear(child: &LayerNode, offset: Point, corners: &RoundedClipCorners) -> bool {
    if !child.draws_within_bounds() || child.graphics_layer.shadow_elevation > 0.0 {
        return false;
    }
    let padding = cranpose_render_common::graph::CONTAINED_DRAW_SLACK
        + child.effect().map_or(0.0, RenderEffect::output_padding);
    stays_clear(
        child
            .transform_to_parent
            .bounds_for_rect(expand_rect(child.local_bounds, padding)),
        offset,
        corners,
    )
}

/// Whether a run's shapes and images may take the corners: the rest of its
/// other lane (text, shadows) stays out of them.
fn run_takes_corners(run: &DrawRunNode, offset: Point, corners: &RoundedClipCorners) -> bool {
    run_others_all(run, |primitive| {
        takes_rounded_clip(primitive)
            || primitive_stays_clear(primitive, corners.raster_scale, |rect| {
                stays_clear(rect, offset, corners)
            })
    })
}

fn run_others_stay_clear(
    run: &DrawRunNode,
    raster_scale: RasterScale,
    clear: impl Fn(Rect) -> bool,
) -> bool {
    run_others_all(run, |primitive| {
        primitive_stays_clear(primitive, raster_scale, &clear)
    })
}

/// Whether every primitive of the run's other lane is one `keeps`.
fn run_others_all(run: &DrawRunNode, keeps: impl Fn(&DrawPrimitive) -> bool) -> bool {
    let recording = &*run.recording;
    recording
        .segments_in(&run.segments)
        .all(|segment| match segment.lane {
            RecordLane::Shapes
            | RecordLane::Content
            | RecordLane::GroupStart
            | RecordLane::GroupEnd => true,
            RecordLane::Others => recording.others()[segment.range()].iter().all(&keeps),
        })
}

fn primitive_stays_clear(
    primitive: &DrawPrimitive,
    raster_scale: RasterScale,
    clear: impl Fn(Rect) -> bool,
) -> bool {
    let mut primitive = primitive;
    while let DrawPrimitive::Blend {
        primitive: inner, ..
    } = primitive
    {
        primitive = inner;
    }
    let bounds = match primitive {
        DrawPrimitive::Shadow(ShadowPrimitive::Drop {
            shape, blur_radius, ..
        }) => {
            let reach = raster_scale.shadow_reach(*blur_radius);
            primitive_coverage_rect(shape).map(|rect| expand_rect(rect, reach))
        }
        DrawPrimitive::Shadow(ShadowPrimitive::Inner { clip_rect, .. }) => Some(*clip_rect),
        DrawPrimitive::Content => return true,
        _ => primitive_coverage_rect(primitive),
    };
    match bounds {
        Some(rect) => {
            [rect.x, rect.y, rect.width, rect.height]
                .into_iter()
                .all(f32::is_finite)
                && clear(rect)
        }
        None => false,
    }
}

fn stays_clear(rect: Rect, offset: Point, corners: &RoundedClipCorners) -> bool {
    corners.admits(expand_rect(
        rect.translate(offset.x, offset.y),
        corners.aa_margin,
    ))
}

/// Whether a primitive takes a rounded clip's coverage in its own shader: a
/// shape record in the shape shader, or an image in the image shader.
fn takes_rounded_clip(primitive: &DrawPrimitive) -> bool {
    matches!(
        primitive,
        DrawPrimitive::Rect { .. }
            | DrawPrimitive::RoundRect { .. }
            | DrawPrimitive::Arc { .. }
            | DrawPrimitive::Line { .. }
            | DrawPrimitive::Trapezoid { .. }
            | DrawPrimitive::Image { .. }
    )
}
