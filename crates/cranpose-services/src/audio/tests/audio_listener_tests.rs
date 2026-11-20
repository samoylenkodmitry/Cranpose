use super::*;

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1.0e-5)
}

// The turn of `degrees` about the y axis: a positive angle turns the head's face to the left
fn yaw(degrees: f32) -> [f32; 4] {
    let (sin, cos) = (degrees.to_radians() / 2.0).sin_cos();
    [0.0, sin, 0.0, cos]
}

#[test]
fn a_head_that_looks_ahead_sees_the_room_axes() {
    let head = AudioListener::AT_ORIGIN;
    assert!(close(head.local([1.0, 2.0, -3.0]), [1.0, 2.0, -3.0]));
}

#[test]
fn the_place_is_relative_to_the_head() {
    let head = AudioListener {
        position: [1.0, 1.5, 2.0],
        ..AudioListener::AT_ORIGIN
    };
    assert!(close(head.local([1.0, 1.5, 1.0]), [0.0, 0.0, -1.0]));
}

#[test]
fn a_head_turned_to_the_left_has_what_stood_ahead_on_its_right() {
    // Turned 90 degrees to the left, the head looks along -x and its right side faces -z
    let head = AudioListener {
        orientation: yaw(90.0),
        ..AudioListener::AT_ORIGIN
    };
    assert!(close(head.local([0.0, 0.0, -1.0]), [1.0, 0.0, 0.0]));
    assert!(close(head.local([-1.0, 0.0, 0.0]), [0.0, 0.0, -1.0]));
}
