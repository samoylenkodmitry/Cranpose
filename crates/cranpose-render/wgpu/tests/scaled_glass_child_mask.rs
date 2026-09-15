mod support;

use std::sync::atomic::{AtomicU32, Ordering};

use cranpose_liquid::{Glass, LiquidModifierExt, LiquidShape, LiquidTheme, LiquidThemeSpec};
use support::page::*;

const FRAME: u32 = 200;
const BUTTON: [f32; 4] = [80.0, 80.0, 40.0, 40.0];
const CENTER: (f32, f32) = (100.0, 100.0);
const PULSE: f32 = 1.45;
const BACKGROUND: Color = Color(0.12, 0.13, 0.2, 1.0);

static SCALE: AtomicU32 = AtomicU32::new(0);

type DifferingPixel = (usize, usize, [u8; 4], [u8; 4]);

fn scale() -> Option<f32> {
    let bits = SCALE.load(Ordering::Relaxed);
    (bits != 0).then(|| f32::from_bits(bits))
}

#[composable]
#[allow(non_snake_case)]
fn PulsingGlassButtonPage() {
    LiquidTheme(LiquidThemeSpec::default(), || {
        FramePage(FRAME, FRAME, BACKGROUND, || {
            let Some(scale) = scale() else {
                return;
            };
            let glass = Glass::regular().shape(LiquidShape::Circle).shadow(false);
            let button = rect_modifier(BUTTON).glass_effect(glass).then(
                Modifier::empty().graphics_layer_block(move |layer| {
                    layer.scale = scale;
                }),
            );
            Box(button, BoxSpec::new(), || {});
        });
    });
}

fn capture(scale: Option<f32>) -> Option<Vec<u8>> {
    SCALE.store(scale.map_or(0, f32::to_bits), Ordering::Relaxed);
    support::warm_app_frame(PulsingGlassButtonPage, FRAME, FRAME).map(|(frame, _)| frame.pixels)
}

fn distance_from_center(x: usize, y: usize) -> f32 {
    let dx = x as f32 + 0.5 - CENTER.0;
    let dy = y as f32 + 0.5 - CENTER.1;
    (dx * dx + dy * dy).sqrt()
}

/// The pixels of `frame` that differ from `plain` outside the circle of
/// `radius`, and how many differ inside it.
fn differing_around_circle(
    frame: &[u8],
    plain: &[u8],
    radius: f32,
) -> (Vec<DifferingPixel>, usize) {
    let mut outside = Vec::new();
    let mut inside = 0;
    for (x, y, a, b) in support::differing_pixels(FRAME, frame, plain) {
        if distance_from_center(x, y) > radius + 1.5 {
            outside.push((x, y, a, b));
        } else {
            inside += 1;
        }
    }
    (outside, inside)
}

/// A circular glass button scaled by its own graphics layer, as a favourite
/// star pulses, stays a circle: outside the scaled circle the page is what
/// it is without the button, as it is for the button at rest.
#[test]
fn a_scaled_circular_glass_button_leaves_the_page_outside_its_circle() {
    let Some(plain) = capture(None) else {
        return;
    };
    let radius = BUTTON[2] * 0.5;
    let resting = capture(Some(1.0)).expect("headless WGPU init failed mid-suite");
    let (outside, inside) = differing_around_circle(&resting, &plain, radius);
    assert!(
        inside > 500,
        "the resting button must draw its glass for this to test anything: {inside} pixels"
    );
    assert!(
        outside.is_empty(),
        "the resting button changed the page outside its circle: {}",
        support::describe_differing(&outside)
    );

    let pulsing = capture(Some(PULSE)).expect("headless WGPU init failed mid-suite");
    let (outside, inside) = differing_around_circle(&pulsing, &plain, radius * PULSE);
    assert!(
        inside > 1000,
        "the pulsing button must draw its glass for this to test anything: {inside} pixels"
    );
    assert!(
        outside.is_empty(),
        "the pulsing button changed the page outside its scaled circle, the square its glass \
         is scissored to instead of masked: {}",
        support::describe_differing(&outside)
    );
}
