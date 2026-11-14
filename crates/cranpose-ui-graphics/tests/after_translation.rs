use cranpose_ui_graphics::{Point, ProjectiveTransform};

// A pixel walk places a child at its origin through `after_translation`;
// the child's draws must land where the full product puts them, under any
// parent transform, perspective included.
#[test]
fn a_translation_first_maps_as_the_full_product() {
    let parents = [
        ProjectiveTransform::identity(),
        ProjectiveTransform::translation(12.0, -7.5),
        ProjectiveTransform::uniform_scale(2.5).then(ProjectiveTransform::translation(1.0, 2.0)),
        ProjectiveTransform::from_homogeneous([
            [-2.0, 0.0, 5.0],
            [0.0, 0.5, -3.0],
            [0.0, 0.0, 1.0],
        ]),
        ProjectiveTransform::from_homogeneous([
            [0.0, -1.0, 30.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
        ]),
        ProjectiveTransform::from_homogeneous([
            [0.9, 0.2, 4.0],
            [-0.1, 1.1, 8.0],
            [0.001, 0.002, 1.0],
        ]),
    ];
    let offsets = [(0.0, 0.0), (3.25, -1.5), (-40.0, 120.75), (0.1, 0.2)];
    let points = [
        Point::new(0.0, 0.0),
        Point::new(17.5, -3.0),
        Point::new(-8.0, 64.0),
    ];
    for parent in parents {
        for (tx, ty) in offsets {
            let product = ProjectiveTransform::translation(tx, ty).then(parent);
            let placed = parent.after_translation(tx, ty);
            assert_eq!(placed, product, "{parent:?} after ({tx}, {ty})");
            for point in points {
                assert_eq!(placed.map_point(point), product.map_point(point));
            }
        }
    }
}
