use super::*;
use crate::RenderViewpoint;

fn view() -> StreamedView {
    StreamedView {
        pose: Pose::IDENTITY,
        fov: FieldOfView::symmetric(1.0, 1.0),
        rect: CropRect {
            x: 0,
            y: 0,
            width: 320,
            height: 360,
        },
    }
}

fn views(left: StreamedView) -> StreamedViews {
    StreamedViews {
        left,
        right: StreamedView {
            rect: CropRect {
                x: 320,
                ..view().rect
            },
            ..view()
        },
    }
}

#[test]
fn proper_views_pass_and_a_rounded_quaternion_is_made_a_unit() {
    let mut left = view();
    left.pose.orientation = [0.0, 0.0, 0.0, 1.03];
    let checked = views(left).checked((640, 360)).expect("proper views");
    assert_eq!(checked.left.pose.orientation, [0.0, 0.0, 0.0, 1.0]);
}

#[test]
fn a_view_that_the_compositor_would_refuse_is_rejected() {
    let bad_poses = [
        Pose::at([f32::NAN, 0.0, 0.0]),
        Pose {
            orientation: [0.0; 4],
            ..Pose::IDENTITY
        },
        Pose {
            orientation: [0.0, 0.0, 0.0, 2.0],
            ..Pose::IDENTITY
        },
    ];
    for pose in bad_poses {
        let left = StreamedView { pose, ..view() };
        assert!(views(left).checked((640, 360)).is_err(), "{pose:?}");
    }
    let flat = FieldOfView {
        angle_left: 0.5,
        angle_right: -0.5,
        ..FieldOfView::symmetric(1.0, 1.0)
    };
    let wide = FieldOfView::symmetric(3.2, 1.0);
    for fov in [flat, wide] {
        let left = StreamedView { fov, ..view() };
        assert!(views(left).checked((640, 360)).is_err(), "{fov:?}");
    }
}

#[test]
fn a_rectangle_must_lie_inside_the_picture() {
    let outside = CropRect {
        x: 400,
        ..view().rect
    };
    let empty = CropRect {
        width: 0,
        ..view().rect
    };
    for rect in [outside, empty] {
        let left = StreamedView { rect, ..view() };
        assert!(views(left).checked((640, 360)).is_err(), "{rect:?}");
    }
    assert!(views(view()).checked((640, 360)).is_ok());
    assert!(views(view()).checked((639, 360)).is_err());
}

#[test]
fn an_eye_of_the_headset_gives_its_pose_and_field_of_view_to_a_streamed_view() {
    // The values that the Quest 3 reported for its left eye
    let eye = RenderViewpoint {
        pose: Pose::at([0.0, 1.6, 0.0]),
        local_pose: Pose::at([-0.032, 0.0, 0.0]),
        field_of_view: FieldOfView {
            angle_left: -0.942_477_8,
            angle_right: 0.698_131_7,
            angle_up: 0.767_944_9,
            angle_down: -0.959_931_1,
        },
    };
    let from_eye = StreamedView {
        pose: eye.pose,
        fov: eye.field_of_view,
        ..view()
    };
    let checked = views(from_eye)
        .checked((640, 360))
        .expect("the headset's own view");
    assert_eq!(checked.left.fov, eye.field_of_view);
}
