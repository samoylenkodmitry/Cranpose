mod support;

use std::sync::atomic::{AtomicBool, Ordering};

use cranpose_liquid::{Glass, GlassSurface, LiquidShape, LiquidTheme, LiquidThemeSpec};
use support::page::*;

const FRAME_WIDTH: u32 = 320;
const FRAME_HEIGHT: u32 = 200;
const CARD: [f32; 4] = [40.0, 40.0, 240.0, 96.0];

static SHADOW: AtomicBool = AtomicBool::new(true);

#[composable]
#[allow(non_snake_case)]
fn ClippedGlassCardPage() {
    LiquidTheme(LiquidThemeSpec::default(), || {
        FramePage(
            FRAME_WIDTH,
            FRAME_HEIGHT,
            Color(0.82, 0.84, 0.9, 1.0),
            || {
                let glass = Glass::regular()
                    .shape(LiquidShape::RoundedRect(20.0))
                    .blur_radius(0.0)
                    .shadow(SHADOW.load(Ordering::Relaxed));
                GlassSurface(rect_modifier(CARD), glass, || {
                    Text(
                        "Shadowed card",
                        Modifier::empty().offset(18.0, 18.0),
                        TextStyle::default(),
                    );
                });
            },
        );
    });
}

fn capture(shadow: bool) -> Option<Vec<u8>> {
    SHADOW.store(shadow, Ordering::Relaxed);
    let (_lock, mut shell) = support::app_shell_for(
        ClippedGlassCardPage,
        FRAME_WIDTH,
        FRAME_HEIGHT,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        |_| {},
    )?;
    shell
        .renderer()
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("warm-up capture should succeed");
    let frame = shell
        .renderer()
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("frame capture should succeed");
    assert_eq!(shell.renderer().device_error_count_for_tests(), 0);
    Some(frame.pixels)
}

fn inside_card(x: usize, y: usize) -> bool {
    let (x, y) = (x as f32, y as f32);
    (CARD[0]..CARD[0] + CARD[2]).contains(&x) && (CARD[1]..CARD[1] + CARD[3]).contains(&y)
}

#[test]
fn a_clipped_glass_cards_drop_shadow_falls_outside_the_card() {
    let Some(with_shadow) = capture(true) else {
        return;
    };
    let without_shadow = capture(false).expect("headless WGPU init failed mid-suite");
    let mut outside_differing = 0usize;
    let mut outside_max_delta = 0u8;
    for (index, (a, b)) in with_shadow
        .as_chunks::<4>()
        .0
        .iter()
        .zip(without_shadow.as_chunks::<4>().0)
        .enumerate()
    {
        let (x, y) = (index % FRAME_WIDTH as usize, index / FRAME_WIDTH as usize);
        if inside_card(x, y) {
            continue;
        }
        let delta = a
            .iter()
            .zip(b)
            .map(|(p, q)| p.abs_diff(*q))
            .max()
            .unwrap_or(0);
        outside_max_delta = outside_max_delta.max(delta);
        outside_differing += usize::from(delta > 0);
    }
    eprintln!(
        "shadow on vs off outside the card: {outside_differing} pixels differ, max delta \
         {outside_max_delta}"
    );
    assert!(
        outside_differing > 500 && outside_max_delta >= 8,
        "a drop shadow chained before the clipping glass layer must darken the page around the \
         card; {outside_differing} pixels differ outside it by at most {outside_max_delta} \
         levels, so the layer's shape clip swallowed its own shadow"
    );
}
