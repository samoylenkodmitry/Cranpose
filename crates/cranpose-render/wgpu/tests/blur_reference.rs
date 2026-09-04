mod support;

#[path = "../src/test_support.rs"]
mod shared_test_support;

use cranpose_render_common::graph::{ProjectiveTransform, RenderGraph, RenderNode};
use cranpose_ui_graphics::{Color, GraphicsLayer, Rect, RenderEffect};
use support::{region_pixels, solid_rect};

const FRAME: u32 = 160;
const RADIUS: f32 = 4.0;
const GLASS: Rect = Rect {
    x: 40.0,
    y: 40.0,
    width: 80.0,
    height: 80.0,
};

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// Bars and blocks of distinct colors under the glass, so every tap of the
/// kernel meets an edge somewhere.
fn page(with_blur: bool) -> RenderGraph {
    let mut children = vec![
        solid_rect(
            rect(0.0, 0.0, FRAME as f32, FRAME as f32),
            Color::from_rgb_u8(30, 40, 60),
        ),
        solid_rect(
            rect(50.0, 0.0, 9.0, FRAME as f32),
            Color::from_rgb_u8(240, 80, 40),
        ),
        solid_rect(
            rect(0.0, 70.0, FRAME as f32, 7.0),
            Color::from_rgb_u8(60, 220, 90),
        ),
        solid_rect(
            rect(80.0, 90.0, 25.0, 13.0),
            Color::from_rgb_u8(250, 240, 120),
        ),
        solid_rect(
            rect(95.0, 45.0, 3.0, 40.0),
            Color::from_rgb_u8(255, 255, 255),
        ),
    ];
    if with_blur {
        children.push(RenderNode::Layer(Box::new(
            shared_test_support::layer_node(
                rect(0.0, 0.0, GLASS.width, GLASS.height),
                ProjectiveTransform::translation(GLASS.x, GLASS.y),
                GraphicsLayer {
                    backdrop_effect: Some(RenderEffect::blur(RADIUS)),
                    ..GraphicsLayer::default()
                },
                Vec::new(),
            ),
        )));
    }
    support::page_graph(FRAME, FRAME, children)
}

/// The renderer's kernel: `ceil(radius)` taps each side, `sigma = radius /
/// 2`, truncated there and normalized.
fn kernel() -> Vec<f32> {
    let taps = RADIUS.ceil() as i32;
    let sigma = RADIUS * 0.5;
    let weights: Vec<f32> = (-taps..=taps)
        .map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp())
        .collect();
    let total: f32 = weights.iter().sum();
    weights.into_iter().map(|w| w / total).collect()
}

/// A separable blur of the 8-bit page, horizontal then vertical, clamped at
/// the frame's edges, in float.
fn reference_blur(pixels: &[u8]) -> Vec<f32> {
    let size = FRAME as i32;
    let kernel = kernel();
    let taps = kernel.len() as i32 / 2;
    let at = |x: i32, y: i32, c: usize| {
        let x = x.clamp(0, size - 1);
        let y = y.clamp(0, size - 1);
        pixels[((y * size + x) * 4) as usize + c] as f32
    };
    let mut horizontal = vec![0.0f32; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            for c in 0..4 {
                let mut sum = 0.0;
                for (k, weight) in kernel.iter().enumerate() {
                    sum += weight * at(x + k as i32 - taps, y, c);
                }
                horizontal[((y * size + x) * 4 + c as i32) as usize] = sum;
            }
        }
    }
    let at_h = |x: i32, y: i32, c: usize| {
        let y = y.clamp(0, size - 1);
        horizontal[((y * size + x) * 4 + c as i32) as usize]
    };
    let mut out = vec![0.0f32; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            for c in 0..4 {
                let mut sum = 0.0;
                for (k, weight) in kernel.iter().enumerate() {
                    sum += weight * at_h(x, y + k as i32 - taps, c);
                }
                out[((y * size + x) * 4 + c as i32) as usize] = sum;
            }
        }
    }
    out
}

#[test]
fn a_blur_matches_its_kernel_applied_by_the_cpu_within_one_step() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let plain = support::capture_graph(&mut renderer, page(false), FRAME, FRAME);
    let blurred = support::capture_graph(&mut renderer, page(true), FRAME, FRAME);
    let expected = reference_blur(&plain.pixels);
    let inside = rect(
        GLASS.x + 2.0,
        GLASS.y + 2.0,
        GLASS.width - 4.0,
        GLASS.height - 4.0,
    );
    let actual = region_pixels(&blurred, inside);
    let mut worst = 0.0f32;
    let mut worst_at = (0, 0, 0);
    let mut changed = 0usize;
    for (index, value) in actual.iter().enumerate() {
        let x = inside.x as usize + index / 4 % inside.width as usize;
        let y = inside.y as usize + index / 4 / inside.width as usize;
        let c = index % 4;
        let want = expected[(y * FRAME as usize + x) * 4 + c];
        let delta = (*value as f32 - want).abs();
        if delta > worst {
            worst = delta;
            worst_at = (x, y, c);
        }
        if *value != plain.pixels[(y * FRAME as usize + x) * 4 + c] {
            changed += 1;
        }
    }
    assert!(
        changed > actual.len() / 4,
        "the blur must change most pixels under the glass"
    );
    assert!(
        worst <= 1.0,
        "the blur diverges from its kernel by {worst} at {worst_at:?}; every weight and \
         tap offset must reproduce the kernel"
    );
}
