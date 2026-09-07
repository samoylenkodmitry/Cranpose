mod support;

#[path = "../src/test_support.rs"]
mod shared_test_support;

use cranpose_render_common::graph::{ProjectiveTransform, RenderGraph, RenderNode};
use cranpose_render_wgpu::CapturedFrame;
use cranpose_ui_graphics::{
    GradientBlurDirection, GraphicsLayer, Rect, RenderEffect, SubstrateSpec, gradient_blur_effect,
};
use support::{ReferenceEdge, SubstrateProbeRead, region_pixels};

const FRAME_WIDTH: u32 = 240;
const FRAME_HEIGHT: u32 = 120;
const GLASS: Rect = Rect {
    x: 40.0,
    y: 20.0,
    width: 96.0,
    height: 72.0,
};
const SUBSTRATE_RADIUS: f32 = 12.0;
const BAND_TOP: f32 = 30.0;
const BAND_HEIGHT: f32 = 60.0;
const WIDE_RADIUS: f32 = 12.0;

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn effect_layer(bounds: Rect, effect: RenderEffect) -> RenderNode {
    RenderNode::Layer(Box::new(shared_test_support::layer_node(
        rect(0.0, 0.0, bounds.width, bounds.height),
        ProjectiveTransform::translation(bounds.x, bounds.y),
        GraphicsLayer {
            backdrop_effect: Some(effect),
            ..GraphicsLayer::default()
        },
        Vec::new(),
    )))
}

fn page(layer: Option<RenderNode>) -> RenderGraph {
    let mut children = support::striped_page(FRAME_WIDTH, FRAME_HEIGHT);
    children.extend(layer);
    support::page_graph(FRAME_WIDTH, FRAME_HEIGHT, children)
}

fn capture(renderer: &mut support::LockedRenderer, graph: RenderGraph) -> CapturedFrame {
    support::capture_graph(renderer, graph, FRAME_WIDTH, FRAME_HEIGHT)
}

/// The page under `region` blurred by `radius` on the CPU, the region's
/// edge held, as the blur of a capture of exactly that region is.
fn reference_blur_of(page: &CapturedFrame, region: Rect, radius: f32) -> Vec<f32> {
    let pixels: Vec<f32> = region_pixels(page, region)
        .iter()
        .map(|value| f32::from(*value))
        .collect();
    support::reference_blur(
        &pixels,
        region.width as usize,
        region.height as usize,
        4,
        radius,
        ReferenceEdge::Clamp,
    )
}

/// The worst channel deviation of `frame` over `rows` of `region` from
/// `expected`, laid out over `region`.
fn worst_deviation(
    frame: &CapturedFrame,
    region: Rect,
    expected: &[f32],
    rows: std::ops::Range<usize>,
    inset: usize,
) -> (f32, (usize, usize)) {
    let actual = region_pixels(frame, region);
    let width = region.width as usize;
    let mut worst = (0.0f32, (0, 0));
    for y in rows {
        for x in inset..width - inset {
            for channel in 0..3 {
                let index = (y * width + x) * 4 + channel;
                let delta = (f32::from(actual[index]) - expected[index]).abs();
                if delta > worst.0 {
                    worst = (delta, (x, y));
                }
            }
        }
    }
    worst
}

/// A substrate blurred by twelve pixels runs at a quarter of its capture's
/// size and lands within this many levels of the CPU kernel away from its
/// edge (11.0 measured on a page of four-pixel stripes): the block average
/// of four, the truncation of the kernel at whole scratch texels and the
/// interpolation back, where a substrate never copied back into its slot
/// reads the pool's stale texels, 140 and more away. Its edge texels,
/// whose blocks reach past the region and hold to it, read the mean of a
/// block where the kernel weighs the corner texel, 32 away on the stripes.
const BLURRED_SUBSTRATE_BUDGET: f32 = 14.0;
const BLURRED_SUBSTRATE_EDGE_BUDGET: f32 = 36.0;

#[test]
fn a_blurred_substrate_is_the_capture_blurred_by_its_radius() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let plain = capture(&mut renderer, page(None));
    let probe = capture(
        &mut renderer,
        page(Some(effect_layer(
            GLASS,
            support::substrate_probe(
                SubstrateSpec::Blur {
                    radius_px: SUBSTRATE_RADIUS,
                },
                SubstrateProbeRead::Held,
            ),
        ))),
    );
    let expected = reference_blur_of(&plain, GLASS, SUBSTRATE_RADIUS);
    let height = GLASS.height as usize;
    let interior = worst_deviation(&probe, GLASS, &expected, 4..height - 4, 4);
    let edge = worst_deviation(&probe, GLASS, &expected, 0..height, 0);
    assert!(
        interior.0 <= BLURRED_SUBSTRATE_BUDGET && edge.0 <= BLURRED_SUBSTRATE_EDGE_BUDGET,
        "the substrate blurred by {SUBSTRATE_RADIUS} diverges from the kernel by {} at {:?} \
         inside and by {} at {:?} at its edge",
        interior.0,
        interior.1,
        edge.0,
        edge.1
    );
}

fn band() -> Rect {
    rect(0.0, BAND_TOP, FRAME_WIDTH as f32, BAND_HEIGHT)
}

/// The capture a gradient blur over `band()` reads: the band grown by the
/// wide radius, where its clamp edge lies.
fn band_capture() -> Rect {
    let reach = WIDE_RADIUS.ceil();
    rect(
        0.0,
        BAND_TOP - reach,
        FRAME_WIDTH as f32,
        BAND_HEIGHT + 2.0 * reach,
    )
}

fn gradient_band(renderer: &mut support::LockedRenderer) -> CapturedFrame {
    capture(
        renderer,
        page(Some(effect_layer(
            band(),
            gradient_blur_effect(WIDE_RADIUS, 0.0, GradientBlurDirection::TopToBottom),
        ))),
    )
}

/// A gradient blur's top row asks for the wide radius and reads the wide
/// level whole: within the level row's budget of the CPU kernel at that
/// radius.
#[test]
fn a_gradient_blur_realises_its_wide_radius_as_the_wide_level() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let plain = capture(&mut renderer, page(None));
    let frame = gradient_band(&mut renderer);
    let capture_rect = band_capture();
    let expected = reference_blur_of(&plain, capture_rect, WIDE_RADIUS);
    let top_row = (BAND_TOP - capture_rect.y) as usize + 1;
    let (worst, at) = worst_deviation(
        &frame,
        capture_rect,
        &expected,
        top_row..top_row + 1,
        WIDE_RADIUS as usize + 2,
    );
    assert!(
        worst <= LEVEL_ROW_BUDGET,
        "the band's top row diverges from the radius-{WIDE_RADIUS} kernel by {worst} at {at:?}"
    );
}

/// The row of the band whose radius is `share` of the wide one: the taper
/// is a smoothstep of the band's height.
fn band_row_for(share: f32) -> usize {
    let progress = 1.0 - share;
    let mut low = 0.0f32;
    let mut high = 1.0f32;
    for _ in 0..40 {
        let mid = (low + high) * 0.5;
        if mid * mid * (3.0 - 2.0 * mid) < progress {
            low = mid;
        } else {
            high = mid;
        }
    }
    let local = (low + high) * 0.5;
    (BAND_TOP + BAND_HEIGHT * local - band_capture().y).round() as usize
}

/// The worst deviation of the band's row asking for `share` of the wide
/// radius from the CPU kernel at that radius.
fn band_row_deviation(frame: &CapturedFrame, plain: &CapturedFrame, share: f32) -> f32 {
    let capture_rect = band_capture();
    let expected = reference_blur_of(plain, capture_rect, WIDE_RADIUS * share);
    let row = band_row_for(share);
    worst_deviation(
        frame,
        capture_rect,
        &expected,
        row..row + 1,
        WIDE_RADIUS as usize + 2,
    )
    .0
}

/// A row asking for exactly a level's radius reads that level whole and
/// lands within this many levels of the kernel at that radius (3.3
/// measured at the wide level, 4.4 at the half); a row midway between two
/// levels blends them and lies within twice that of the kernel at its
/// radius on a page of four-pixel stripes (9.6 measured between the half
/// and the wide level, 4.4 between the quarter and the half), where
/// blending two levels four times apart in radius lands 44 away and a
/// level never copied back past 100.
const LEVEL_ROW_BUDGET: f32 = 7.0;
const BLENDED_LEVEL_BUDGET: f32 = 14.0;

#[test]
fn a_gradient_blur_blends_its_levels_between_sharp_and_wide() {
    let Ok(mut renderer) = support::headless_renderer() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let plain = capture(&mut renderer, page(None));
    let frame = gradient_band(&mut renderer);
    let bottom = rect(0.0, BAND_TOP + BAND_HEIGHT - 2.0, FRAME_WIDTH as f32, 1.0);
    assert_eq!(
        region_pixels(&frame, bottom),
        region_pixels(&plain, bottom),
        "the band's bottom row asks for no blur and shows the page"
    );
    let rows = [
        (0.5, LEVEL_ROW_BUDGET),
        (0.75, BLENDED_LEVEL_BUDGET),
        (0.375, BLENDED_LEVEL_BUDGET),
    ];
    let deviations: Vec<(f32, f32, f32)> = rows
        .iter()
        .map(|(share, budget)| (*share, band_row_deviation(&frame, &plain, *share), *budget))
        .collect();
    assert!(
        deviations
            .iter()
            .all(|(_, deviation, budget)| deviation <= budget),
        "the band's rows diverge from the kernels at their radii (share of the wide radius, \
         deviation, budget): {deviations:?}"
    );
}
