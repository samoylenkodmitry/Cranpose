use super::*;
use crate::pose::turn_degrees;

fn near(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
}

// A floor 2 by 4 meters at y = -1.5: its +Y axis is the normal and points up.
fn floor() -> Surface<'static> {
    Surface {
        center: Pose::at([0.0, -1.5, 0.0]),
        extents: [2.0, 4.0],
        vertices: &[],
        plane_type: PlaneType::HorizontalUpwardFacing,
    }
}

// A table top at y = -0.7, 1 meter square.
fn table() -> Surface<'static> {
    Surface {
        center: Pose::at([0.0, -0.7, -1.0]),
        extents: [1.0, 1.0],
        vertices: &[],
        plane_type: PlaneType::HorizontalUpwardFacing,
    }
}

// A wall at z = -3 that spans 4 meters across and 2.5 up. Its +Y axis is the normal and points
// toward +Z, into the room: a quarter turn about X takes +Y to +Z.
fn wall() -> Surface<'static> {
    Surface {
        center: Pose {
            position: [0.0, 0.0, -3.0],
            orientation: turn_degrees(90.0, 0.0, 0.0),
        },
        extents: [4.0, 2.5],
        vertices: &[],
        plane_type: PlaneType::Vertical,
    }
}

fn content(position: [f32; 3], size: [f32; 3]) -> Content {
    Content {
        pose: Pose::at(position),
        size,
    }
}

#[test]
fn a_panel_above_a_table_stands_on_it_by_its_bottom_edge() {
    // A panel 40 cm tall whose center is 30 cm above the table top: its bottom edge is 10 cm up
    let (gap, snapped) = snap(&table(), &content([0.1, -0.4, -1.0], [0.6, 0.4, 0.0])).unwrap();
    assert!((gap - 0.1).abs() < 1e-4, "{gap}");
    assert!(near(snapped.position, [0.1, -0.5, -1.0]), "{snapped:?}");
}

#[test]
fn a_panel_that_stands_on_a_table_keeps_the_way_it_faced() {
    let mut carried = content([0.0, -0.4, -1.0], [0.6, 0.4, 0.0]);
    carried.pose.orientation = turn_degrees(0.0, 35.0, 0.0);
    let (_, snapped) = snap(&table(), &carried).unwrap();
    let facing = snapped.rotate([0.0, 0.0, 1.0]);
    let expected = [
        35.0_f32.to_radians().sin(),
        0.0,
        35.0_f32.to_radians().cos(),
    ];
    assert!(near(facing, expected), "{facing:?}");
    assert!(near(snapped.rotate([0.0, 1.0, 0.0]), [0.0, 1.0, 0.0]));
}

#[test]
fn a_panel_that_a_carry_tilted_stands_upright_on_the_table() {
    let mut carried = content([0.0, -0.4, -1.0], [0.6, 0.4, 0.0]);
    carried.pose.orientation = turn_degrees(-25.0, 20.0, 5.0);
    let (_, snapped) = snap(&table(), &carried).unwrap();
    assert!(
        near(snapped.rotate([0.0, 1.0, 0.0]), [0.0, 1.0, 0.0]),
        "{snapped:?}"
    );
}

#[test]
fn a_volume_stands_on_the_floor_by_its_bottom_face() {
    // A box 50 cm tall whose bottom face is 5 cm above the floor
    let (_, snapped) = snap(&floor(), &content([0.0, -1.2, 0.0], [0.5, 0.5, 0.5])).unwrap();
    assert!(near(snapped.position, [0.0, -1.25, 0.0]), "{snapped:?}");
}

#[test]
fn a_panel_beyond_the_snap_distance_does_not_snap() {
    // The panel's bottom edge is 60 cm above the table
    assert!(snap(&table(), &content([0.0, 0.1, -1.0], [0.6, 0.4, 0.0])).is_none());
}

#[test]
fn a_panel_below_the_plane_does_not_snap_to_its_far_side() {
    // The center is 1 meter under the table top
    assert!(snap(&table(), &content([0.0, -1.7, -1.0], [0.6, 0.4, 0.0])).is_none());
}

#[test]
fn a_panel_beside_the_table_snaps_only_within_the_edge_margin() {
    // The table reaches 50 cm each side of its center: 20 cm beyond the edge is inside the margin
    assert!(snap(&table(), &content([0.7, -0.4, -1.0], [0.6, 0.4, 0.0])).is_some());
    assert!(snap(&table(), &content([1.0, -0.4, -1.0], [0.6, 0.4, 0.0])).is_none());
}

#[test]
fn a_panel_near_a_wall_lies_flat_against_it_facing_into_the_room() {
    let (gap, snapped) = snap(&wall(), &content([0.5, 0.2, -2.9], [0.8, 0.5, 0.0])).unwrap();
    assert!((gap - 0.1).abs() < 1e-4, "{gap}");
    assert!(near(snapped.position, [0.5, 0.2, -3.0]), "{snapped:?}");
    assert!(
        near(snapped.rotate([0.0, 0.0, 1.0]), [0.0, 0.0, 1.0]),
        "{snapped:?}"
    );
    assert!(
        near(snapped.rotate([0.0, 1.0, 0.0]), [0.0, 1.0, 0.0]),
        "{snapped:?}"
    );
}

#[test]
fn a_volume_near_a_wall_rests_with_its_back_against_it() {
    // A box 40 cm deep: its center rests 20 cm from the wall
    let (_, snapped) = snap(&wall(), &content([0.0, 0.0, -2.6], [0.4, 0.4, 0.4])).unwrap();
    assert!(near(snapped.position, [0.0, 0.0, -2.8]), "{snapped:?}");
}

#[test]
fn content_that_the_wall_hides_faces_the_other_way_when_it_is_behind_the_plane() {
    // Behind a wall that faces +Z the panel is on the far side, and it faces -Z there
    let (_, snapped) = snap(&wall(), &content([0.0, 0.0, -3.1], [0.8, 0.5, 0.0])).unwrap();
    assert!(
        near(snapped.rotate([0.0, 0.0, 1.0]), [0.0, 0.0, -1.0]),
        "{snapped:?}"
    );
}

#[test]
fn a_ceiling_holds_a_panel_by_its_top_edge() {
    let ceiling = Surface {
        center: Pose {
            position: [0.0, 1.0, 0.0],
            orientation: turn_degrees(0.0, 0.0, 180.0),
        },
        extents: [3.0, 3.0],
        vertices: &[],
        plane_type: PlaneType::HorizontalDownwardFacing,
    };
    // The panel is 40 cm tall and hangs with its top edge 10 cm under the ceiling
    let (_, snapped) = snap(&ceiling, &content([0.0, 0.7, 0.0], [0.6, 0.4, 0.0])).unwrap();
    assert!(near(snapped.position, [0.0, 0.8, 0.0]), "{snapped:?}");
}

#[test]
fn a_polygon_decides_where_the_surface_is_and_a_concave_corner_is_outside() {
    // An L shaped floor: the square (-1, -1) to (1, 1) with its upper right quarter cut away
    let vertices = [
        [-1.0, -1.0],
        [1.0, -1.0],
        [1.0, 0.0],
        [0.0, 0.0],
        [0.0, 1.0],
        [-1.0, 1.0],
    ];
    let l_floor = Surface {
        center: Pose::at([0.0, 0.0, 0.0]),
        extents: [2.0, 2.0],
        vertices: &vertices,
        plane_type: PlaneType::HorizontalUpwardFacing,
    };
    let on = |x: f32, z: f32| snap(&l_floor, &content([x, 0.2, z], [0.4, 0.4, 0.0]));
    assert!(on(-0.5, -0.5).is_some());
    assert!(on(0.5, -0.5).is_some());
    // The cut-away quarter is outside the polygon: a point 25 cm from its edges is within the
    // margin, and one 50 cm from them is not
    assert!(on(0.25, 0.25).is_some());
    assert!(on(0.5, 0.5).is_none());
    assert!(on(1.5, 1.5).is_none());
}

#[test]
fn the_nearest_surface_wins_in_the_store_and_the_gap_is_reported() {
    // Two planes at once: the floor, and the table that is closer
    let panel = content([0.0, -0.45, -1.0], [0.6, 0.4, 0.0]);
    let on_floor = snap(&floor(), &panel);
    let on_table = snap(&table(), &panel).unwrap();
    assert!(on_floor.is_none());
    assert!((on_table.0 - 0.05).abs() < 1e-4);
}
