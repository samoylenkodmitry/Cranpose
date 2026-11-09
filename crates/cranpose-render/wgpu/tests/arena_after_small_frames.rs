use cranpose_render_common::graph::LayerNode;
use cranpose_ui_graphics::{Color, Rect};

use crate::support;

const WIDTH: u32 = 160;
const HEIGHT: u32 = 120;
// Enough rects that a frame's arena tables grow far past their first size.
const MANY: usize = 6_000;
// Frames of one rect, well past the frames after which the arena lets a
// generation that large go.
const SMALL_FRAMES: usize = 120;

fn rects(count: usize) -> LayerNode {
    let children = (0..count)
        .map(|index| {
            let rect = Rect {
                x: (index % 40) as f32 * 4.0,
                y: (index / 40 % 30) as f32 * 4.0,
                width: 3.0,
                height: 3.0,
            };
            let shade = (index % 7) as f32 / 7.0;
            support::solid_rect(rect, Color(shade, 1.0 - shade, 0.5, 1.0))
        })
        .collect();
    support::page_graph(WIDTH, HEIGHT, children)
}

// A large frame after a long run of small ones, which leaves the arena far
// smaller than the large frame, draws what the large frame drew first.
fn draws_alike_after_small_frames(renderer: Result<support::LockedRenderer, String>) {
    let mut renderer = match renderer {
        Ok(renderer) => renderer,
        Err(err) => {
            eprintln!("skipping the arena after small frames: headless WGPU init failed: {err}");
            return;
        }
    };
    let first = support::capture_graph(&mut renderer, rects(MANY), WIDTH, HEIGHT);
    for _ in 0..SMALL_FRAMES {
        support::capture_graph(&mut renderer, rects(1), WIDTH, HEIGHT);
    }
    let again = support::capture_graph(&mut renderer, rects(MANY), WIDTH, HEIGHT);
    assert!(
        first.pixels == again.pixels,
        "the large frame after small ones draws as the first one did"
    );
}

#[test]
fn a_large_frame_after_small_ones_draws_as_the_first() {
    draws_alike_after_small_frames(support::headless_renderer());
}

#[test]
fn a_copied_arena_draws_a_large_frame_after_small_ones_as_the_first() {
    draws_alike_after_small_frames(support::headless_renderer_copying_uploads());
}
