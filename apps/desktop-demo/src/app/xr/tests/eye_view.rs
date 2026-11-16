use std::f32::consts::PI;

use cranpose_ui::{space::Vec3, Size};

use super::{cran, EyeView, EYE_HEIGHT, UNITS_PER_METER};

// An off-center view must put the image center where the headset expects it, or the two eyes
// disagree and the scene swims.
#[test]
fn an_uneven_view_moves_the_image_center() {
    let size = Size {
        width: 400.0,
        height: 400.0,
    };
    let screen_x = |view: &EyeView, point: [f32; 3]| {
        let layout = view.layout(size);
        let seen = view.camera(layout).view.transform_point(cran(point));
        size.width * 0.5 + layout.focal_length * seen.x / seen.z
    };
    let even = EyeView {
        position: [0.0, EYE_HEIGHT, 0.0],
        orientation: [0.0, 0.0, 0.0, 1.0],
        fov: [-1.0, 1.0, 1.0, -1.0],
    };
    assert!((screen_x(&even, [0.0, EYE_HEIGHT, -2.0]) - 200.0).abs() < 1e-2);
    // three quarters of the view lie left of straight ahead, so straight ahead shows at 3/4
    let uneven = EyeView {
        fov: [-1.5, 0.5, 1.0, -1.0],
        ..even
    };
    for depth in [1.0, 5.0] {
        let x = screen_x(&uneven, [0.0, EYE_HEIGHT, -depth]);
        assert!((x - 300.0).abs() < 1e-2, "depth {depth}: {x}");
    }
    // the view's left edge shows at the box's left edge
    let edge = screen_x(&uneven, [-1.5 * 2.0, EYE_HEIGHT, -2.0]);
    assert!(edge.abs() < 1e-2, "{edge}");
}

#[test]
fn the_camera_follows_position_and_turn() {
    let ahead = EyeView::middle(0.0, 0.0, 1.2, 1.0);
    let seen = ahead
        .camera(ahead.layout(Size {
            width: 100.0,
            height: 100.0,
        }))
        .view
        .transform_point(cran([0.0, EYE_HEIGHT, -2.0]));
    assert!(
        seen.x.abs() < 1e-2 && seen.y.abs() < 1e-2 && (seen.z - 2.0 * UNITS_PER_METER).abs() < 1e-2,
        "{seen:?}"
    );
    let above = ahead
        .camera(ahead.layout(Size {
            width: 100.0,
            height: 100.0,
        }))
        .view
        .transform_point(cran([0.0, EYE_HEIGHT + 0.5, -2.0]));
    assert!(
        above.y < 0.0,
        "a point above shows above the middle: {above:?}"
    );

    let turned_left = EyeView::middle(PI / 2.0, 0.0, 1.2, 1.0);
    let left = turned_left
        .camera(turned_left.layout(Size {
            width: 100.0,
            height: 100.0,
        }))
        .view
        .transform_point(cran([-2.0, EYE_HEIGHT, 0.0]));
    assert!(
        left.x.abs() < 1e-2 && (left.z - 2.0 * UNITS_PER_METER).abs() < 1e-2,
        "{left:?}"
    );
}

#[test]
fn the_two_eyes_stand_apart_and_mirror_their_views() {
    let [left, right] = EyeView::pair(0.4, -0.2);
    let apart = Vec3::from_array(right.position) - Vec3::from_array(left.position);
    assert!((apart.length() - 0.064).abs() < 1e-5);
    // the right eye lies along the turned head's right
    let (sin, cos) = 0.4f32.sin_cos();
    assert!(apart.dot(Vec3::new(cos, 0.0, -sin)) > 0.06, "{apart:?}");
    assert_eq!(left.fov[0], -right.fov[1]);
}
