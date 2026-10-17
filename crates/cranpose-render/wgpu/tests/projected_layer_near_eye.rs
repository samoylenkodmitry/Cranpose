use cranpose_render_common::graph::{LayerNode, ProjectiveTransform, RenderNode};
use cranpose_ui_graphics::{Color, Rect};

use crate::support;

const FRAME: u32 = 200;
const WALL: Rect = Rect {
    x: 0.0,
    y: 0.0,
    width: 500.0,
    height: 200.0,
};
const RIGHT_OF_WALL: usize = 95;

fn wall_beside_the_eye() -> ProjectiveTransform {
    ProjectiveTransform::from_rows([[1.0, 0.0, -150.0], [1.0, 1.0, -200.0], [0.01, 0.0, -1.0]])
}

#[test]
fn a_wall_reaching_behind_the_eye_draws_only_its_part_in_front() {
    let mut renderer = match support::headless_renderer() {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping projected layer near the eye: {err}");
            return;
        }
    };
    let wall = LayerNode {
        local_bounds: WALL,
        transform_to_parent: wall_beside_the_eye(),
        children: vec![support::solid_rect(WALL, Color::from_rgb_u8(220, 30, 30))],
        ..Default::default()
    };
    let frame = support::capture_graph(
        &mut renderer,
        support::page_graph(FRAME, FRAME, vec![RenderNode::Layer(Box::new(wall))]),
        FRAME,
        FRAME,
    );
    let (left, right) = frame
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .filter(|(_, px)| px[0] > 160 && px[1] < 90 && px[2] < 90)
        .fold((0, 0), |(left, right), (index, _)| {
            if index % FRAME as usize > RIGHT_OF_WALL {
                (left, right + 1)
            } else {
                (left + 1, right)
            }
        });
    assert!(
        left > 5000,
        "the wall's part in front of the eye is missing: {left} red pixels on the left"
    );
    assert_eq!(
        right, 0,
        "the wall's part behind the eye was drawn mirrored on the right of the frame"
    );
}
