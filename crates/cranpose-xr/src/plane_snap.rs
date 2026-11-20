//! Where a movable composable comes to rest on a plane of the room.

use crate::{
    PlaneType, Pose,
    pose::turn_of_axes,
    vector::{add, cross, dot, normalize, perpendicular, scale, sub},
};

// How far from the plane a composable's resting face may be and still snap to it, in meters.
pub(crate) const SNAP_DISTANCE: f32 = 0.25;

// How far beyond the plane's edge a composable's center may be and still snap to it, in meters.
pub(crate) const EDGE_MARGIN: f32 = 0.30;

// A direction whose part along an axis is longer than this is too close to that axis to say which way
// it points across it.
const DEGENERATE: f32 = 1e-3;

// A plane as the snap sees it: the pose of its center, whose +Y axis is the normal, the size of
// its bounding rectangle along the pose's X and Z axes and the polygon of its vertices along the
// same axes. A plane with no polygon is its rectangle.
pub(crate) struct Surface<'a> {
    pub(crate) center: Pose,
    pub(crate) extents: [f32; 2],
    pub(crate) vertices: &'a [[f32; 2]],
    pub(crate) plane_type: PlaneType,
}

// The composable that a user carries: its center and turn in the room, and its width, height and
// depth in meters. A panel is flat, so its depth is zero. A composable faces along its +Z axis.
pub(crate) struct Content {
    pub(crate) pose: Pose,
    pub(crate) size: [f32; 3],
}

// Where `content` comes to rest on `surface`, with the gap in meters between the resting face and
// the plane before the snap, or `None` when the content is too far from the plane or from its edge.
//
// As in Jetpack XR, the content's +Z axis points out of the plane, so a panel lies flat along it. A
// composable rests with its back on the plane: a panel with its center on it, and a volume half its
// depth out. On a wall it stands upright. On a floor, a table or a ceiling its top edge points away
// from where it faced before, so a panel that faced the user reads from the user's side.
pub(crate) fn snap(surface: &Surface<'_>, content: &Content) -> Option<(f32, Pose)> {
    let normal = surface.center.rotate([0.0, 1.0, 0.0]);
    let across = surface.center.rotate([1.0, 0.0, 0.0]);
    let along = surface.center.rotate([0.0, 0.0, 1.0]);
    let from_center = sub(content.pose.position, surface.center.position);
    let height = dot(from_center, normal);
    if !within(surface, [dot(from_center, across), dot(from_center, along)]) {
        return None;
    }
    let facing = match surface.plane_type {
        PlaneType::Vertical if height < 0.0 => scale(normal, -1.0),
        PlaneType::Vertical
        | PlaneType::HorizontalUpwardFacing
        | PlaneType::HorizontalDownwardFacing => normal,
    };
    let reach = content.size[2] / 2.0;
    let gap = (dot(from_center, facing) - reach).abs();
    if gap > SNAP_DISTANCE {
        return None;
    }
    let on_plane = sub(content.pose.position, scale(normal, height));
    let position = add(on_plane, scale(facing, reach));
    let orientation = facing_with_top(surface.plane_type, facing, &content.pose)?;
    Some((
        gap,
        Pose {
            position,
            orientation,
        },
    ))
}

// The turn that points a composable's +Z axis along `facing` and its +Y axis along the top that
// suits the plane, or `None` when no top can be found.
fn facing_with_top(plane_type: PlaneType, facing: [f32; 3], held: &Pose) -> Option<[f32; 4]> {
    let tops = match plane_type {
        // A wall holds the composable upright
        PlaneType::Vertical => [[0.0, 1.0, 0.0], [0.0, 1.0, 0.0]],
        // A flat plane holds it with its top away from where it faced, or where its top pointed if it
        // already faced along the plane's normal
        PlaneType::HorizontalUpwardFacing | PlaneType::HorizontalDownwardFacing => [
            scale(held.rotate([0.0, 0.0, 1.0]), -1.0),
            held.rotate([0.0, 1.0, 0.0]),
        ],
    };
    let top = tops
        .into_iter()
        .map(|top| perpendicular(top, facing))
        .find(|top| dot(*top, *top) > DEGENERATE)?;
    let y = normalize(top);
    Some(turn_of_axes(cross(y, facing), y, facing))
}

// Whether the point `at`, in meters along the surface's X and Z axes, lies on the surface or no
// farther than EDGE_MARGIN from its edge.
fn within(surface: &Surface<'_>, at: [f32; 2]) -> bool {
    if surface.vertices.len() < 3 {
        let [width, depth] = surface.extents;
        return at[0].abs() <= width / 2.0 + EDGE_MARGIN
            && at[1].abs() <= depth / 2.0 + EDGE_MARGIN;
    }
    inside_polygon(surface.vertices, at) || edge_distance(surface.vertices, at) <= EDGE_MARGIN
}

// Whether `point` lies inside the polygon: a ray toward +x crosses its edges an odd number of times.
fn inside_polygon(polygon: &[[f32; 2]], point: [f32; 2]) -> bool {
    let mut inside = false;
    let mut previous = polygon[polygon.len() - 1];
    for &vertex in polygon {
        if (vertex[1] > point[1]) != (previous[1] > point[1]) {
            let crossing = (previous[0] - vertex[0]) * (point[1] - vertex[1])
                / (previous[1] - vertex[1])
                + vertex[0];
            if point[0] < crossing {
                inside = !inside;
            }
        }
        previous = vertex;
    }
    inside
}

// The distance from `point` to the nearest edge of the polygon.
fn edge_distance(polygon: &[[f32; 2]], point: [f32; 2]) -> f32 {
    let mut nearest = f32::INFINITY;
    let mut previous = polygon[polygon.len() - 1];
    for &vertex in polygon {
        let edge = [vertex[0] - previous[0], vertex[1] - previous[1]];
        let length_squared = edge[0] * edge[0] + edge[1] * edge[1];
        let along = if length_squared > 0.0 {
            (((point[0] - previous[0]) * edge[0] + (point[1] - previous[1]) * edge[1])
                / length_squared)
                .clamp(0.0, 1.0)
        } else {
            0.0
        };
        let near = [previous[0] + edge[0] * along, previous[1] + edge[1] * along];
        nearest = nearest.min((point[0] - near[0]).hypot(point[1] - near[1]));
        previous = vertex;
    }
    nearest
}

#[cfg(test)]
#[path = "tests/plane_snap_tests.rs"]
mod tests;
