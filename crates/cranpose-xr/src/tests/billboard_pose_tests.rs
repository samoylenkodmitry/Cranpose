use super::*;

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5)
}

const CENTER: [f32; 3] = [0.1, 0.9, -1.0];

#[test]
fn the_quad_faces_a_head_on_any_side() {
    // In front, behind, left, right, slanted below, and straight above and below
    let offsets = [
        [0.0, 0.0, 1.5],
        [0.0, 0.0, -1.5],
        [-1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.3, -0.5, -0.4],
        [0.0, 1.2, 0.0],
        [0.0, -1.2, 0.0],
    ];
    for offset in offsets {
        let head = [
            CENTER[0] + offset[0],
            CENTER[1] + offset[1],
            CENTER[2] + offset[2],
        ];
        let pose = billboard_pose(CENTER, head);
        assert_eq!(pose.position, CENTER);
        assert!(
            close(pose.rotate([0.0, 0.0, 1.0]), normalize(offset)),
            "{offset:?}"
        );
        let top = pose.rotate([0.0, 1.0, 0.0]);
        assert!(dot(top, offset).abs() < 1e-5, "{offset:?}");
        assert!((dot(top, top) - 1.0).abs() < 1e-5, "{offset:?}");
    }
}

#[test]
fn the_top_of_the_quad_keeps_up_while_the_head_walks_around() {
    for offset in [[0.0, 0.0, 1.5], [0.0, 0.0, -1.5], [1.0, 0.4, 0.0]] {
        let head = [
            CENTER[0] + offset[0],
            CENTER[1] + offset[1],
            CENTER[2] + offset[2],
        ];
        let top = billboard_pose(CENTER, head).rotate([0.0, 1.0, 0.0]);
        assert!(top[1] > 0.9, "{offset:?}: {top:?}");
    }
}

#[test]
fn a_head_at_the_center_still_gets_a_valid_turn() {
    let pose = billboard_pose(CENTER, CENTER);
    assert!(close(pose.rotate([0.0, 0.0, 1.0]), [0.0, 0.0, 1.0]));
    assert!(close(pose.rotate([0.0, 1.0, 0.0]), [0.0, 1.0, 0.0]));
}
