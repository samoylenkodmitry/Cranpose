use std::f32::consts::{FRAC_PI_2, FRAC_PI_3, PI};

use cranpose_spatial::{Camera, RoomShape, Vec3, Wall, focal_length_for};
use cranpose_ui_graphics::{Point, Rect, Size};

const VIEWPORT: Size = Size {
    width: 1200.0,
    height: 800.0,
};

fn room() -> (RoomShape, f32) {
    let focal_length = focal_length_for(VIEWPORT.height, FRAC_PI_3);
    (
        RoomShape::for_viewport(VIEWPORT, focal_length),
        focal_length,
    )
}

fn wall_rect(shape: &RoomShape, wall: Wall) -> Rect {
    Rect::from_size(shape.wall_size(wall))
}

fn close(actual: Point, expected: Point) -> bool {
    (actual.x - expected.x).abs() < 0.01 && (actual.y - expected.y).abs() < 0.01
}

#[test]
fn the_screen_camera_shows_the_north_wall_as_the_flat_screen() {
    let (shape, focal_length) = room();
    let projection = shape
        .screen_camera(focal_length)
        .projection(&shape.plane(Wall::North), VIEWPORT);
    for point in [
        Point::new(0.0, 0.0),
        Point::new(VIEWPORT.width, 0.0),
        Point::new(317.0, 541.0),
        Point::new(VIEWPORT.width, VIEWPORT.height),
    ] {
        assert!(close(projection.map_point(point), point), "{point:?}");
    }
}

#[test]
fn a_wall_point_lands_where_the_eye_sees_it() {
    let (shape, focal_length) = room();
    let camera = shape.standing_camera(focal_length);
    let projection = camera.projection(&shape.plane(Wall::North), VIEWPORT);
    let distance = shape.depth - camera.position.z;
    let local = Point::new(100.0, 700.0);
    let expected = Point::new(
        VIEWPORT.width * 0.5 + focal_length * (local.x - camera.position.x) / distance,
        VIEWPORT.height * 0.5 + focal_length * (local.y - camera.position.y) / distance,
    );
    assert!(close(projection.map_point(local), expected));
}

#[test]
fn a_wall_behind_the_camera_covers_nothing() {
    let (shape, focal_length) = room();
    let camera = shape.standing_camera(focal_length);
    let south = camera.projection(&shape.plane(Wall::South), VIEWPORT);
    let bounds = south.bounds_for_rect(wall_rect(&shape, Wall::South));
    assert_eq!((bounds.width, bounds.height), (0.0, 0.0));
    assert!(
        south
            .front_polygon(wall_rect(&shape, Wall::South))
            .is_empty()
    );
}

#[test]
fn a_side_wall_reaching_behind_the_camera_is_cut_where_it_passes_the_eye() {
    let (shape, focal_length) = room();
    let camera = shape.standing_camera(focal_length);
    let west = camera.projection(&shape.plane(Wall::West), VIEWPORT);
    let rect = wall_rect(&shape, Wall::West);
    assert!(!west.rect_in_front(rect));
    let front = west.front_polygon(rect);
    assert_eq!(front.corners().len(), 4);
    let behind_eye = Point::new(camera.position.z * 0.5, VIEWPORT.height * 0.5);
    assert!(!west.in_front(behind_eye));
    let ahead_of_eye = Point::new(shape.depth * 0.9, VIEWPORT.height * 0.5);
    assert!(west.in_front(ahead_of_eye));
    let bounds = west.bounds_for_rect(rect);
    assert!(bounds.x.is_finite() && bounds.width.is_finite());
    assert!(bounds.x + bounds.width > 0.0 && bounds.x < VIEWPORT.width * 0.5);
}

#[test]
fn a_screen_point_maps_back_only_onto_the_part_of_a_wall_in_front() {
    let (shape, focal_length) = room();
    let camera = shape.standing_camera(focal_length);
    let west = camera.projection(&shape.plane(Wall::West), VIEWPORT);
    let Some(back) = west.inverse() else {
        panic!("the west wall's projection inverts");
    };
    let on_wall = Point::new(shape.depth * 0.8, 300.0);
    let seen = west.map_point(on_wall);
    let Some(found) = back.map_point_in_front(seen) else {
        panic!("a point in front maps back");
    };
    assert!(close(found, on_wall));
    let behind = Point::new(camera.position.z * 0.25, 300.0);
    let ghost = west.map_point(behind);
    assert_eq!(back.map_point_in_front(ghost), None);
}

#[test]
fn a_camera_turns_the_short_way_round() {
    let (shape, focal_length) = room();
    let from = Camera {
        yaw: PI - 0.1,
        ..shape.standing_camera(focal_length)
    };
    let to = Camera {
        yaw: -PI + 0.1,
        ..from
    };
    let halfway = from.lerp(&to, 0.5);
    assert!((halfway.yaw - PI).abs() < 1.0e-4, "{}", halfway.yaw);
}

#[test]
fn walking_into_a_wall_stops_short_of_it() {
    let (shape, _) = room();
    let kept = shape.keep_inside(Vec3::new(-500.0, 10.0, shape.depth + 500.0), 80.0);
    assert_eq!(kept, Vec3::new(80.0, 10.0, shape.depth - 80.0));
}

#[test]
fn a_camera_facing_east_reads_the_east_wall_left_to_right() {
    let (shape, focal_length) = room();
    let camera = Camera {
        yaw: FRAC_PI_2,
        ..shape.standing_camera(focal_length)
    };
    let east = camera.projection(&shape.plane(Wall::East), VIEWPORT);
    let left = east.map_point(Point::new(shape.depth * 0.6, 400.0));
    let right = east.map_point(Point::new(shape.depth * 0.7, 400.0));
    assert!(left.x < right.x);
}
