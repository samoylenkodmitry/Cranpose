use cranpose_ui_graphics::{ProjectiveTransform, Rect};

/// The bounds of every mapped corner, as a transform of any kind has them.
fn corner_bounds(transform: &ProjectiveTransform, rect: Rect) -> Rect {
    let corners = transform.map_rect(rect);
    let xs = corners.map(|corner| corner[0]);
    let ys = corners.map(|corner| corner[1]);
    let min_x = xs.into_iter().fold(f32::INFINITY, f32::min);
    let min_y = ys.into_iter().fold(f32::INFINITY, f32::min);
    let max_x = xs.into_iter().fold(f32::NEG_INFINITY, f32::max);
    let max_y = ys.into_iter().fold(f32::NEG_INFINITY, f32::max);
    Rect {
        x: min_x,
        y: min_y,
        width: max_x - min_x,
        height: max_y - min_y,
    }
}

#[test]
fn scaled_and_translated_bounds_match_the_mapped_corners() {
    let rect = Rect {
        x: 3.5,
        y: -2.0,
        width: 40.25,
        height: 17.0,
    };
    let transforms = [
        ProjectiveTransform::identity(),
        ProjectiveTransform::translation(12.0, -7.5),
        ProjectiveTransform::uniform_scale(2.5).then(ProjectiveTransform::translation(1.0, 2.0)),
        // A mirrored axis maps the left edge to the right.
        ProjectiveTransform::from_homogeneous([
            [-2.0, 0.0, 5.0],
            [0.0, 0.5, -3.0],
            [0.0, 0.0, 1.0],
        ]),
        ProjectiveTransform::from_homogeneous([
            [1.0, 0.0, 0.0],
            [0.0, -1.0, 240.0],
            [0.0, 0.0, 1.0],
        ]),
    ];
    for transform in transforms {
        assert_eq!(
            transform.bounds_for_rect(rect),
            corner_bounds(&transform, rect),
            "{transform:?}"
        );
    }
}

#[test]
fn turned_and_perspective_bounds_cover_every_corner() {
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 10.0,
        height: 20.0,
    };
    let turned = ProjectiveTransform::from_homogeneous([
        [0.0, -1.0, 30.0],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
    ]);
    assert_eq!(
        turned.bounds_for_rect(rect),
        Rect {
            x: 10.0,
            y: 0.0,
            width: 20.0,
            height: 10.0,
        }
    );
    let perspective =
        ProjectiveTransform::from_homogeneous([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.01, 0.0, 1.0]]);
    assert_eq!(
        perspective.bounds_for_rect(rect),
        corner_bounds(&perspective, rect)
    );
}
