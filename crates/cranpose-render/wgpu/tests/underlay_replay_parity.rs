mod support;

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_liquid::{
    Glass, GlassButton, GlassButtonSpec, GlassSurface, LiquidShape, LiquidTheme, LiquidThemeSpec,
};
use cranpose_ui::{
    Color, Modifier, RenderEffect, TextStyle, composable,
    widgets::{Box, BoxSpec, Text},
};
use support::{FramePage, rect_modifier};

const FRAME_WIDTH: u32 = 320;
const FRAME_HEIGHT: u32 = 300;
const CARDS: [[f32; 4]; 2] = [[24.0, 30.0, 272.0, 110.0], [24.0, 160.0, 272.0, 110.0]];
const BUTTON_IN_CARD: [f32; 4] = [212.0, 36.0, 40.0, 40.0];
const BAR: [f32; 4] = [0.0, 120.0, 320.0, 60.0];

#[composable]
#[allow(non_snake_case)]
fn Stripes() {
    Box(
        rect_modifier([0.0, 0.0, 160.0, FRAME_HEIGHT as f32])
            .background(Color(0.55, 0.20, 0.12, 1.0)),
        BoxSpec::new(),
        || {},
    );
    for row in 0..15 {
        let y = row as f32 * 20.0;
        let shade = 0.2 + 0.05 * row as f32;
        Box(
            rect_modifier([120.0, y, 200.0, 10.0]).background(Color(shade, 0.9 - shade, 0.5, 1.0)),
            BoxSpec::new(),
            || {},
        );
    }
}

#[composable]
#[allow(non_snake_case)]
fn BlurCardsPage() {
    FramePage(
        FRAME_WIDTH,
        FRAME_HEIGHT,
        Color(0.08, 0.12, 0.30, 1.0),
        || {
            Stripes();
            for card in CARDS {
                Box(
                    rect_modifier(card)
                        .backdrop_effect(RenderEffect::blur(8.0))
                        .background(Color(1.0, 1.0, 1.0, 0.18))
                        .rounded_corners(14.0),
                    BoxSpec::new(),
                    || {
                        Text(
                            "Replayed blur",
                            Modifier::empty().offset(16.0, 16.0),
                            TextStyle::default(),
                        );
                        Box(
                            rect_modifier(BUTTON_IN_CARD)
                                .backdrop_effect(RenderEffect::blur(5.0))
                                .background(Color(1.0, 1.0, 1.0, 0.24))
                                .rounded_corners(20.0),
                            BoxSpec::new(),
                            || {},
                        );
                    },
                );
            }
        },
    );
}

fn card_glass() -> Glass {
    Glass::regular()
        .shape(LiquidShape::RoundedRect(20.0))
        .blur_radius(0.0)
        .refraction_depth(0.58)
        .refraction_curve(0.62)
        .dispersion(1.0)
        .transmission_refraction(0.72)
        .highlight(0.72)
}

#[composable]
#[allow(non_snake_case)]
fn GlassCardsPage() {
    GlassCards(false);
}

#[composable]
#[allow(non_snake_case)]
fn BarredGlassCardsPage() {
    GlassCards(true);
}

#[composable]
#[allow(non_snake_case)]
fn GlassCards(barred: bool) {
    LiquidTheme(LiquidThemeSpec::default(), move || {
        FramePage(
            FRAME_WIDTH,
            FRAME_HEIGHT,
            Color(0.08, 0.12, 0.30, 1.0),
            move || {
                Stripes();
                for card in CARDS {
                    GlassSurface(rect_modifier(card), card_glass(), move || {
                        Text(
                            if barred {
                                "Under a bar"
                            } else {
                                "Replayed glass"
                            },
                            Modifier::empty().offset(16.0, 16.0),
                            TextStyle::default(),
                        );
                        if !barred {
                            GlassButton(
                                rect_modifier(BUTTON_IN_CARD),
                                GlassButtonSpec::glass(),
                                || {},
                                || {},
                            );
                        }
                    });
                }
                if barred {
                    Box(
                        rect_modifier(BAR)
                            .backdrop_effect(RenderEffect::blur(6.0))
                            .background(Color(1.0, 1.0, 1.0, 0.2)),
                        BoxSpec::new(),
                        || {},
                    );
                }
            },
        );
    });
}

struct Capture {
    pixels: Vec<u8>,
    stats: cranpose_render_wgpu::RenderStatsSnapshot,
}

fn cold_capture(replay: bool, page: fn(), frames: usize) -> Option<Vec<Capture>> {
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_NO_BACKDROP_FLATTEN", Some("1"));
    let (_lock, mut renderer) = match support::headless_renderer_parts() {
        Ok(parts) => parts,
        Err(err) => {
            eprintln!("skipping (headless WGPU init failed): {err}");
            return None;
        }
    };
    renderer.set_underlay_replay_enabled(replay);
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(renderer, root_key, page);
    shell.set_viewport(FRAME_WIDTH as f32, FRAME_HEIGHT as f32);
    shell.set_buffer_size(FRAME_WIDTH, FRAME_HEIGHT);
    let mut captures = Vec::with_capacity(frames);
    for _ in 0..frames {
        shell.update();
        let frame = shell
            .renderer()
            .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
            .expect("frame capture should succeed");
        let stats = shell.renderer().last_frame_stats().expect("frame stats");
        captures.push(Capture {
            pixels: frame.pixels,
            stats,
        });
    }
    Some(captures)
}

struct PixelDifference {
    max_delta: u8,
    differing: usize,
    bounds: Option<[u32; 4]>,
}

fn pixel_difference(reference: &[u8], replayed: &[u8]) -> PixelDifference {
    assert_eq!(reference.len(), replayed.len());
    let mut max_delta = 0u8;
    let mut differing = 0usize;
    let mut bounds: Option<[u32; 4]> = None;
    for (index, (a, b)) in reference
        .as_chunks::<4>()
        .0
        .iter()
        .zip(replayed.as_chunks::<4>().0)
        .enumerate()
    {
        let delta = a
            .iter()
            .zip(b)
            .map(|(x, y)| x.abs_diff(*y))
            .max()
            .unwrap_or(0);
        max_delta = max_delta.max(delta);
        if delta > 0 {
            differing += 1;
            let x = index as u32 % FRAME_WIDTH;
            let y = index as u32 / FRAME_WIDTH;
            bounds = Some(match bounds {
                None => [x, y, x, y],
                Some([left, top, right, bottom]) => {
                    [left.min(x), top.min(y), right.max(x), bottom.max(y)]
                }
            });
        }
    }
    PixelDifference {
        max_delta,
        differing,
        bounds,
    }
}

fn parity(label: &str, page: fn()) {
    let Some(reference) = cold_capture(false, page, 2) else {
        return;
    };
    let Some(replayed) = cold_capture(true, page, 2) else {
        return;
    };
    for (frame, (reference, replayed)) in reference.iter().zip(&replayed).enumerate() {
        let difference = pixel_difference(&reference.pixels, &replayed.pixels);
        println!(
            "{label} frame {frame}: {} pixels within {:?} differ by up to {}",
            difference.differing, difference.bounds, difference.max_delta
        );
        assert!(
            difference.max_delta <= 1,
            "{label} frame {frame}: replaying the pending composites into the underlay copy is the same blend on the same pixels as flushing them first, but {} pixels within {:?} differ by up to {}",
            difference.differing,
            difference.bounds,
            difference.max_delta
        );
        let card_pixels = CARDS.len() * (CARDS[0][2] * CARDS[0][3]) as usize;
        assert!(
            difference.differing * 20 < card_pixels,
            "{label} frame {frame}: a shader composite interpolates its uv at the moved viewport, which may round a sparse few samples one LSB apart, but {} of {card_pixels} card pixels differ",
            difference.differing
        );
    }
    let root_area = u64::from(FRAME_WIDTH) * u64::from(FRAME_HEIGHT);
    let card_area = (CARDS[0][2] * CARDS[0][3]) as u64;
    let least_saving = root_area - card_area * CARDS.len() as u64;
    let reference_pixels = reference[0].stats.pass_pixels;
    let replayed_pixels = replayed[0].stats.pass_pixels;
    println!(
        "{label}: pass pixels reference={reference_pixels} replayed={replayed_pixels} passes {} vs {}",
        reference[0].stats.pass_count, replayed[0].stats.pass_count
    );
    assert!(
        reference_pixels >= replayed_pixels + least_saving,
        "{label}: the cards' self-flushes must leave the full target, at the price of one card-sized replay each, so the frame's pass pixels drop by at least {least_saving}: {reference_pixels} before, {replayed_pixels} after"
    );
}

#[test]
fn replaying_blur_cards_into_their_underlay_copies_matches_the_flushed_render_exactly() {
    parity("blur cards", BlurCardsPage);
}

#[test]
fn replaying_glass_cards_into_their_underlay_copies_matches_the_flushed_render_exactly() {
    parity("glass cards", GlassCardsPage);
}

#[test]
fn a_bar_capturing_over_pending_cards_replays_them_instead_of_flushing_the_target() {
    parity("barred glass cards", BarredGlassCardsPage);
}
