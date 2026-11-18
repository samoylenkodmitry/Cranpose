use super::*;
use crate::Ray;

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5)
}

fn beam(direction: [f32; 3]) -> Beam {
    Beam {
        ray: Ray {
            origin: [0.1, -0.2, 0.3],
            direction,
        },
        length: 2.0,
    }
}

// Six axes and one slanted direction, which take each branch of the quaternion's making.
const DIRECTIONS: [[f32; 3]; 7] = [
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.0, 0.0, -1.0],
    [1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0],
];

#[test]
fn the_quad_lies_along_the_beam_and_faces_the_head() {
    let head = [0.3, 1.2, 0.5];
    for direction in DIRECTIONS {
        let beam = beam(direction);
        let pose = quad_pose(&beam, head);
        let middle = add(beam.ray.origin, direction);
        assert!(close(pose.position, middle), "{direction:?}");
        assert!(
            close(pose.rotate([0.0, 1.0, 0.0]), direction),
            "{direction:?}"
        );
        let to_head = sub(head, middle);
        let across = sub(to_head, scale(direction, dot(to_head, direction)));
        assert!(
            close(pose.rotate([0.0, 0.0, 1.0]), normalize(across)),
            "{direction:?}"
        );
    }
}

#[test]
fn a_head_on_the_beams_line_gets_a_face_across_the_beam() {
    for direction in DIRECTIONS {
        let beam = beam(direction);
        let head = add(beam.ray.origin, scale(direction, -3.0));
        let pose = quad_pose(&beam, head);
        let facing = pose.rotate([0.0, 0.0, 1.0]);
        assert!(
            close(pose.rotate([0.0, 1.0, 0.0]), direction),
            "{direction:?}"
        );
        assert!(dot(facing, direction).abs() < 1e-5, "{direction:?}");
        assert!((dot(facing, facing) - 1.0).abs() < 1e-5, "{direction:?}");
    }
}
