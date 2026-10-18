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

/// A plain layer returns a rect's bounds without mapping its corners: they
/// must be the corners' bounds to the bit, as the general path finds them.
#[test]
fn a_plain_layer_bounds_a_rect_as_its_corners_do() {
    use cranpose_ui_graphics::{
        GraphicsLayer,
        layer_transform::{apply_layer_to_quad, apply_layer_to_rect},
        transform::quad_bounds,
    };
    let layer = GraphicsLayer::default();
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 400.0,
        height: 300.0,
    };
    let values = [0.0, 0.1, 100.3, -7.25, 1.0e-3, 16_777_215.0, 3.0e7];
    for &x in &values {
        for &y in &values {
            for &width in &[0.0, 0.1, 0.3, 12.5, 1.0e6] {
                let rect = Rect {
                    x,
                    y,
                    width,
                    height: width * 0.5,
                };
                let direct = apply_layer_to_rect(rect, bounds, &layer);
                let corners = quad_bounds(apply_layer_to_quad(rect, bounds, &layer));
                assert_eq!(
                    [direct.x, direct.y, direct.width, direct.height].map(f32::to_bits),
                    [corners.x, corners.y, corners.width, corners.height].map(f32::to_bits),
                    "{rect:?}"
                );
            }
        }
    }
}
