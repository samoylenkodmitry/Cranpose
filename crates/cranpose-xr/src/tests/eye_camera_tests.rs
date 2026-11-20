use cranpose::{Size, space::NEAR_PLANE};

use super::*;
use crate::pose::turn_degrees;

const SIZE: (u32, u32) = (100, 100);

fn eye(position: [f32; 3], orientation: [f32; 4], edges: [f32; 4]) -> RenderViewpoint {
    RenderViewpoint {
        pose: Pose {
            position,
            orientation,
        },
        local_pose: Pose::IDENTITY,
        field_of_view: FieldOfView {
            angle_left: edges[0],
            angle_right: edges[1],
            angle_up: edges[2],
            angle_down: edges[3],
        },
    }
}

fn at_rest(edges: [f32; 4]) -> RenderViewpoint {
    eye([0.0; 3], [0.0, 0.0, 0.0, 1.0], edges)
}

// Where the point `room`, in meters in the room's axes, shows in an image of `SIZE` pixels.
fn shows(camera: &EyeCamera, room: [f32; 3]) -> (f32, f32) {
    let scene = scene_direction(room) * UNITS_PER_METER;
    let at = camera
        .camera
        .project(scene, Size::new(SIZE.0 as f32, SIZE.1 as f32))
        .expect("the point lies in front of the eye");
    (at.x, at.y)
}

fn assert_near((x, y): (f32, f32), (expected_x, expected_y): (f32, f32)) {
    assert!(
        (x - expected_x).abs() < 0.01 && (y - expected_y).abs() < 0.01,
        "shows at ({x}, {y}) and not at ({expected_x}, {expected_y})"
    );
}

// The point one meter ahead along the direction that makes `horizontal` and `vertical` tangents.
fn ahead(horizontal: f32, vertical: f32) -> [f32; 3] {
    [horizontal, vertical, -1.0]
}

#[test]
fn an_even_view_shows_its_edges_at_the_edges_of_the_image() {
    let quarter = std::f32::consts::FRAC_PI_4;
    let camera = EyeCamera::of(
        &at_rest([-quarter, quarter, quarter, -quarter]),
        SIZE,
        FirstRow::Top,
    )
    .expect("a camera");
    assert_near(shows(&camera, ahead(0.0, 0.0)), (50.0, 50.0));
    assert_near(shows(&camera, ahead(1.0, 0.0)), (100.0, 50.0));
    assert_near(shows(&camera, ahead(-1.0, 0.0)), (0.0, 50.0));
    // Up in the room is toward the image's top, and y grows downward
    assert_near(shows(&camera, ahead(0.0, 1.0)), (50.0, 0.0));
    assert_near(shows(&camera, ahead(0.0, -1.0)), (50.0, 100.0));
    assert_eq!(camera.field_of_view.angle_right, quarter);
}

#[test]
fn an_off_center_view_puts_its_center_in_the_middle_of_the_image() {
    // A left eye that reaches farther outward than toward the nose, wider than high
    let edges = [-1.0_f32, 0.6, 0.5, -0.7];
    let camera = EyeCamera::of(&at_rest(edges), SIZE, FirstRow::Top).expect("a camera");
    let [left, right, up, down] = edges.map(f32::tan);
    // The tangents of the view's own center show in the middle of the image
    assert_near(
        shows(&camera, ahead((left + right) / 2.0, (up + down) / 2.0)),
        (50.0, 50.0),
    );
    // The wide direction limits the focal length: its edges show at the image's edges
    assert!(shows(&camera, ahead(left, 0.0)).0.abs() < 0.01);
    assert!((shows(&camera, ahead(right, 0.0)).0 - 100.0).abs() < 0.01);
}

#[test]
fn the_submitted_field_of_view_is_what_the_image_covers() {
    for edges in [
        [-1.0_f32, 0.6, 0.5, -0.7],
        [-0.5, 0.5, 0.9, -0.9],
        [-0.9, 0.7, 0.8, -0.8],
    ] {
        let camera = EyeCamera::of(&at_rest(edges), SIZE, FirstRow::Top).expect("a camera");
        let covered = camera.field_of_view;
        let [left, right, up, down] = [
            covered.angle_left,
            covered.angle_right,
            covered.angle_up,
            covered.angle_down,
        ]
        .map(f32::tan);
        let (middle_x, middle_y) = ((left + right) / 2.0, (up + down) / 2.0);
        assert_near(shows(&camera, ahead(left, middle_y)), (0.0, 50.0));
        assert_near(shows(&camera, ahead(right, middle_y)), (100.0, 50.0));
        assert_near(shows(&camera, ahead(middle_x, up)), (50.0, 0.0));
        assert_near(shows(&camera, ahead(middle_x, down)), (50.0, 100.0));
        // The image never covers less than the eye's own view
        assert!(covered.angle_left <= edges[0] + 1e-6);
        assert!(covered.angle_right >= edges[1] - 1e-6);
        assert!(covered.angle_up >= edges[2] - 1e-6);
        assert!(covered.angle_down <= edges[3] + 1e-6);
    }
}

#[test]
fn an_eye_sees_the_room_from_its_own_place() {
    let quarter = std::f32::consts::FRAC_PI_4;
    let edges = [-quarter, quarter, quarter, -quarter];
    let right_eye = eye([0.032, 1.6, 0.0], [0.0, 0.0, 0.0, 1.0], edges);
    let camera = EyeCamera::of(&right_eye, SIZE, FirstRow::Top).expect("a camera");
    // Straight ahead of the eye, a meter away and level with it
    assert_near(shows(&camera, [0.032, 1.6, -1.0]), (50.0, 50.0));
    // A point that lies straight ahead of the head's middle lies to the eye's left
    let (x, _) = shows(&camera, [0.0, 1.6, -1.0]);
    assert!(x < 50.0, "{x}");
}

#[test]
fn a_turned_eye_looks_where_it_points() {
    let quarter = std::f32::consts::FRAC_PI_4;
    let edges = [-quarter, quarter, quarter, -quarter];
    // A head turned a quarter turn to the left looks along -x
    let left_turn = eye([0.0; 3], turn_degrees(0.0, 90.0, 0.0), edges);
    let camera = EyeCamera::of(&left_turn, SIZE, FirstRow::Top).expect("a camera");
    assert_near(shows(&camera, [-1.0, 0.0, 0.0]), (50.0, 50.0));
    // A point ahead of the room, left of the way the eye looks, lies to the eye's right
    let (x, _) = shows(&camera, [-1.0, 0.0, -0.5]);
    assert!(x > 50.0, "{x}");
}

#[test]
fn rows_from_the_bottom_draw_the_picture_upside_down() {
    let quarter = std::f32::consts::FRAC_PI_4;
    let edges = [-quarter, quarter, quarter, -quarter];
    let upright = EyeCamera::of(&at_rest(edges), SIZE, FirstRow::Top).expect("a camera");
    let flipped = EyeCamera::of(&at_rest(edges), SIZE, FirstRow::Bottom).expect("a camera");
    let (x, up) = shows(&upright, ahead(0.5, 0.5));
    let (flipped_x, flipped_up) = shows(&flipped, ahead(0.5, 0.5));
    assert!(up < 50.0 && flipped_up > 50.0);
    assert!((x - flipped_x).abs() < 0.01 && (up + flipped_up - 100.0).abs() < 0.01);
    // The field of view that the layer submits does not change with the rows
    assert_eq!(upright.field_of_view, flipped.field_of_view);
}

#[test]
fn the_near_plane_is_the_scenes_in_meters() {
    let quarter = std::f32::consts::FRAC_PI_4;
    let camera = EyeCamera::of(
        &at_rest([-quarter, quarter, quarter, -quarter]),
        SIZE,
        FirstRow::Top,
    )
    .expect("a camera");
    assert_eq!(camera.camera.focal_length, 50.0);
    assert!((camera.near - 50.0 * NEAR_PLANE / UNITS_PER_METER).abs() < 1e-9);
}

#[test]
fn a_view_with_no_extent_has_no_camera() {
    assert_eq!(EyeCamera::of(&at_rest([0.0; 4]), SIZE, FirstRow::Top), None);
    assert_eq!(
        EyeCamera::of(&at_rest([f32::NAN, 0.5, 0.5, -0.5]), SIZE, FirstRow::Top),
        None
    );
    assert_eq!(
        EyeCamera::of(&at_rest([-0.5, 0.5, 0.5, -0.5]), (0, 100), FirstRow::Top),
        None
    );
}
