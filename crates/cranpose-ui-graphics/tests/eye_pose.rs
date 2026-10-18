//! An eye's pose: how it turns, which way it walks, and how a doorway into a
//! space that lies turned turns the eye with it.

use std::sync::Arc;

use cranpose_ui_graphics::{
    Rect, Size,
    space::{EyePose, EyeWorld, Mat4, Mesh, PITCH_LIMIT, Vec3},
};

fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1.0e-4
}

#[test]
fn an_eye_turned_up_past_its_limit_stops_there_square_and_whole() {
    let mut pose = EyePose::standing(Vec3::ZERO, 0.0, 0.0);
    pose.turn(0.0, 3.0);
    assert!(
        (pose.pitch() - PITCH_LIMIT).abs() < 1.0e-4,
        "the eye looks up as far as it may, not {}",
        pose.pitch()
    );
    assert!(pose.forward.dot(pose.down).abs() < 1.0e-5);
    assert!((pose.forward.length() - 1.0).abs() < 1.0e-5);
    assert!((pose.down.length() - 1.0).abs() < 1.0e-5);
}

#[test]
fn an_eye_walks_level_ahead_whatever_it_looks_up_at() {
    let mut pose = EyePose::standing(Vec3::ZERO, 0.5, 0.0);
    pose.turn(0.0, 0.8);
    let (ahead, right) = pose.walking_axes();
    assert!(
        close(ahead, Vec3::new(0.5f32.sin(), 0.0, 0.5f32.cos())),
        "ahead stays level along the eye's turn: {ahead:?}"
    );
    assert!(close(right, Vec3::new(0.5f32.cos(), 0.0, -(0.5f32.sin()))));
}

#[test]
fn a_doorway_into_a_turned_space_turns_the_eye_with_it() {
    // The inner space lies a quarter turn about y from the world's: its z
    // runs along the world's x, through a doorway square to x at x = 100.
    let mut world = EyeWorld::default();
    let outer = world.add_space(Arc::from([]), Mat4::IDENTITY);
    let inner_to_world = Mat4::rotation_y(std::f32::consts::FRAC_PI_2)
        .then(&Mat4::translation(Vec3::new(100.0, 0.0, 0.0)));
    let inner = world.add_space(Arc::from([7]), inner_to_world);
    // The unit square, its z along the world's x, spanning y and z from -50
    // to 50 at x = 100.
    let door = Mat4::from_axes(
        Vec3::new(0.0, 0.0, -100.0),
        Vec3::new(0.0, 100.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(100.0, -50.0, 50.0),
    );
    world.add_doorway((outer, inner), door);
    let start = EyePose::standing(Vec3::ZERO, std::f32::consts::FRAC_PI_2, 0.0);
    let walked = world.walk(&start, Vec3::new(150.0, 0.0, 0.0), 10.0);
    assert_eq!(&*walked.doorways, &[7], "the step goes through the doorway");
    let to_inner = inner_to_world.inverse().expect("a turn and a move invert");
    assert!(
        close(
            walked.position,
            to_inner.transform_point(Vec3::new(150.0, 0.0, 0.0))
        ),
        "the eye stands where it walked to, in the inner space: {:?}",
        walked.position
    );
    assert!(
        close(
            walked.forward,
            to_inner.transform_vector(Vec3::new(1.0, 0.0, 0.0))
        ),
        "the eye looks on the way it looked, in the inner space's terms: {:?}",
        walked.forward
    );
}

#[test]
fn an_eye_keeps_its_radius_in_the_units_of_a_smaller_space() {
    // The inner space's units are half the world's.
    let mut world = EyeWorld::default();
    world.add_space(Arc::from([]), Mat4::IDENTITY);
    let inner_to_world = Mat4::scaling(Vec3::new(0.5, 0.5, 0.5));
    let inner = world.add_space(Arc::from([7]), inner_to_world);
    // A wall across the inner space at its z = 200.
    let wall = Mat4::translation(Vec3::new(-0.5, -0.5, 0.0))
        .then(&Mat4::scaling(Vec3::new(1000.0, 1000.0, 1.0)))
        .then(&Mat4::translation(Vec3::new(0.0, 0.0, 200.0)))
        .then(&inner_to_world);
    world.add_solid(
        (inner, Arc::new(Mesh::plane())),
        wall,
        (Size::new(1000.0, 1000.0), std::iter::empty::<Rect>()),
    );
    let start = EyePose {
        doorways: Arc::from([7]),
        ..EyePose::standing(Vec3::ZERO, 0.0, 0.0)
    };
    let walked = world.walk(&start, Vec3::new(0.0, 0.0, 1000.0), 10.0);
    assert!(
        (walked.position.z - 190.0).abs() < 0.5,
        "the eye stops 10 of the inner space's units before its wall, at {}",
        walked.position.z
    );
}

/// A world of a room and the space behind a doorway in it: the unit square
/// scaled to 200 by 200, its middle at the origin, on the plane z = 100,
/// with the inner space beyond it along z, in the same coordinates.
fn doorway_world() -> EyeWorld {
    let mut world = EyeWorld::default();
    let outer = world.add_space(Arc::from([]), Mat4::IDENTITY);
    let inner = world.add_space(Arc::from([7]), Mat4::IDENTITY);
    let door = Mat4::from_axes(
        Vec3::new(200.0, 0.0, 0.0),
        Vec3::new(0.0, 200.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(-100.0, -100.0, 100.0),
    );
    world.add_doorway((outer, inner), door);
    world
}

#[test]
fn an_eye_behind_a_doorway_stays_behind_its_plane_beside_it() {
    let world = doorway_world();
    // In the inner space, beside the doorway's rect, walking back across
    // the doorway's plane.
    let start = EyePose {
        doorways: Arc::from([7]),
        ..EyePose::standing(Vec3::new(400.0, 0.0, 300.0), 0.0, 0.0)
    };
    let walked = world.walk(&start, Vec3::new(0.0, 0.0, -400.0), 10.0);
    assert_eq!(&*walked.doorways, &[7], "the eye stays in the inner space");
    assert!(
        (walked.position.z - 110.0).abs() < 0.5,
        "the eye stops its radius behind the doorway's plane, at {}",
        walked.position.z
    );
}

#[test]
fn an_eye_behind_a_doorway_walks_back_out_through_it() {
    let world = doorway_world();
    let start = EyePose {
        doorways: Arc::from([7]),
        ..EyePose::standing(Vec3::new(0.0, 0.0, 300.0), 0.0, 0.0)
    };
    let walked = world.walk(&start, Vec3::new(0.0, 0.0, -400.0), 10.0);
    assert!(
        walked.doorways.is_empty(),
        "a step back through the doorway's rect leaves the inner space"
    );
    assert!((walked.position.z + 100.0).abs() < 0.5, "{:?}", walked.position);
}
