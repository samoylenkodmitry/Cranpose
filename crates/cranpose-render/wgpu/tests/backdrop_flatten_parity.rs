mod support;

use std::sync::{
    Mutex,
    atomic::{AtomicU8, Ordering},
};

use cranpose_liquid::{
    Glass, GlassButton, GlassButtonSpec, GlassSurface, LiquidShape, LiquidTheme, LiquidThemeSpec,
};
use cranpose_ui::Alignment;
use support::page::*;

const FRAME_WIDTH: u32 = 320;
const FRAME_HEIGHT: u32 = 260;
const CARD_RADIUS: f32 = 20.0;
const CARDS: [[f32; 4]; 2] = [[20.0, 24.0, 280.0, 96.0], [20.0, 140.0, 280.0, 96.0]];
const TEXT_OFFSET: [f32; 2] = [18.0, 18.0];
const TOGGLE: &str = "CRANPOSE_NO_BACKDROP_FLATTEN";

const NESTED: u8 = 0;
const SIBLINGS: u8 = 1;
const NO_BUTTON: u8 = 2;
const CORNER_CIRCLE: u8 = 3;
const CORNER_CIRCLE_SIBLINGS: u8 = 4;
const CIRCLE: [f32; 4] = [226.0, 28.0, 40.0, 40.0];

static SCENE: Mutex<()> = Mutex::new(());
static ARRANGEMENT: AtomicU8 = AtomicU8::new(NESTED);
static BUTTON_OFFSET: [AtomicU8; 2] = [AtomicU8::new(72), AtomicU8::new(0)];

fn card_glass() -> Glass {
    Glass::regular()
        .shape(LiquidShape::RoundedRect(CARD_RADIUS))
        .blur_radius(0.0)
        .dispersion(1.0)
        .adaptive_frost(Color::WHITE, 0.42)
}

fn button_offset() -> (f32, f32) {
    (
        f32::from(BUTTON_OFFSET[0].load(Ordering::Relaxed)),
        f32::from(BUTTON_OFFSET[1].load(Ordering::Relaxed)),
    )
}

#[composable]
#[allow(non_snake_case)]
fn Dots() {
    for i in 0..14u32 {
        let x = (i as f32 * 61.0) % 300.0;
        let y = (i as f32 * 37.0) % 240.0;
        Box(
            rect_modifier([x, y, 16.0, 16.0])
                .background(Color(0.95, 0.7 - (i % 4) as f32 * 0.15, 0.35, 1.0))
                .rounded_corners(8.0),
            BoxSpec::new(),
            || {},
        );
    }
}

#[composable]
#[allow(non_snake_case)]
fn CardText() {
    Text(
        "Flattened card",
        Modifier::empty().offset(TEXT_OFFSET[0], TEXT_OFFSET[1]),
        TextStyle::default(),
    );
}

#[composable]
#[allow(non_snake_case)]
fn CardButton(offset: (f32, f32)) {
    let spec = if std::env::var("FLATTEN_NO_BUTTON_SHADOW").is_ok() {
        GlassButtonSpec::glass().with_glass(Glass::regular().shadow(false))
    } else {
        GlassButtonSpec::glass()
    };
    GlassButton(
        Modifier::empty().offset(offset.0, offset.1),
        spec,
        || {},
        || {},
    );
}

#[composable]
#[allow(non_snake_case)]
fn CornerCircle() {
    GlassSurface(
        rect_modifier(CIRCLE),
        Glass::regular()
            .shape(LiquidShape::Circle)
            .blur_radius(24.0)
            .shadow(false)
            .no_clip(),
        || {},
    );
}

#[composable]
#[allow(non_snake_case)]
fn GlassCardsPage() {
    LiquidTheme(LiquidThemeSpec::default(), || {
        FramePage(
            FRAME_WIDTH,
            FRAME_HEIGHT,
            Color(0.06, 0.05, 0.12, 1.0),
            || {
                Dots();
                let arrangement = ARRANGEMENT.load(Ordering::Relaxed);
                let offset = button_offset();
                for card in CARDS {
                    match arrangement {
                        NESTED => GlassSurface(rect_modifier(card), card_glass(), move || {
                            CardText();
                            CardButton(offset);
                        }),
                        SIBLINGS => {
                            GlassSurface(rect_modifier(card), card_glass(), || {});
                            Box(
                                rect_modifier(card),
                                BoxSpec::new().content_alignment(Alignment::CENTER),
                                move || {
                                    CardText();
                                    CardButton(offset);
                                },
                            );
                        }
                        CORNER_CIRCLE => {
                            GlassSurface(rect_modifier(card), card_glass(), move || {
                                Box(
                                    rect_modifier([0.0, 0.0, card[2], card[3]]),
                                    BoxSpec::new(),
                                    || {
                                        CardText();
                                        CornerCircle();
                                    },
                                );
                            })
                        }
                        CORNER_CIRCLE_SIBLINGS => {
                            GlassSurface(rect_modifier(card), card_glass(), || {});
                            Box(rect_modifier(card).clip_to_bounds(), BoxSpec::new(), || {
                                CardText();
                                CornerCircle();
                            });
                        }
                        _ => GlassSurface(rect_modifier(card), card_glass(), || {
                            CardText();
                        }),
                    }
                }
            },
        );
    });
}

struct Frame {
    pixels: Vec<u8>,
    cold_passes: u32,
}

fn capture(arrangement: u8, no_flatten: bool) -> Option<Frame> {
    ARRANGEMENT.store(arrangement, Ordering::Relaxed);
    cranpose_render_wgpu::set_debug_toggle(TOGGLE, no_flatten.then_some("1"));
    let captured = capture_with_current_toggles();
    cranpose_render_wgpu::set_debug_toggle(TOGGLE, None);
    captured
}

fn capture_with_current_toggles() -> Option<Frame> {
    let (_lock, mut shell) = support::app_shell_for(
        GlassCardsPage,
        FRAME_WIDTH,
        FRAME_HEIGHT,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        |_| {},
    )?;
    shell
        .renderer()
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("warm-up capture should succeed");
    let cold_passes = shell
        .renderer()
        .last_frame_stats()
        .expect("frame stats")
        .pass_count;
    let frame = shell
        .renderer()
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("frame capture should succeed");
    assert_eq!(shell.renderer().device_error_count_for_tests(), 0);
    Some(Frame {
        pixels: frame.pixels,
        cold_passes,
    })
}

struct Delta {
    max: u8,
    differing: usize,
    compared: usize,
}

fn delta_where(a: &[u8], b: &[u8], mut include: impl FnMut(usize, usize) -> bool) -> Delta {
    assert_eq!(a.len(), b.len());
    let mut delta = Delta {
        max: 0,
        differing: 0,
        compared: 0,
    };
    for (index, (p, q)) in a
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.as_chunks::<4>().0)
        .enumerate()
    {
        let (x, y) = (index % FRAME_WIDTH as usize, index / FRAME_WIDTH as usize);
        if !include(x, y) {
            continue;
        }
        let d = p
            .iter()
            .zip(q)
            .map(|(u, v)| u.abs_diff(*v))
            .max()
            .unwrap_or(0);
        delta.max = delta.max.max(d);
        delta.differing += usize::from(d > 0);
        delta.compared += 1;
    }
    delta
}

fn dump_delta_map(a: &[u8], b: &[u8]) {
    let mut rows = vec![String::new(); FRAME_HEIGHT as usize / 4];
    for (index, (p, q)) in a
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.as_chunks::<4>().0)
        .enumerate()
    {
        let (x, y) = (index % FRAME_WIDTH as usize, index / FRAME_WIDTH as usize);
        if x % 4 != 0 || y % 4 != 0 {
            continue;
        }
        let d = p
            .iter()
            .zip(q)
            .map(|(u, v)| u.abs_diff(*v))
            .max()
            .unwrap_or(0);
        rows[y / 4].push(match d {
            0 => '.',
            1..=2 => ':',
            3..=16 => '+',
            _ => '#',
        });
    }
    for row in rows {
        eprintln!("{row}");
    }
}

fn inside_a_card(x: usize, y: usize) -> bool {
    let (x, y) = (x as f32, y as f32);
    CARDS.iter().any(|card| {
        (card[0]..card[0] + card[2]).contains(&x) && (card[1]..card[1] + card[3]).contains(&y)
    })
}

fn in_a_corner_triangle(x: usize, y: usize) -> bool {
    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
    CARDS.iter().any(|card| {
        let (left, top) = (card[0], card[1]);
        let (right, bottom) = (card[0] + card[2], card[1] + card[3]);
        [
            (left + CARD_RADIUS, top + CARD_RADIUS),
            (right - CARD_RADIUS, top + CARD_RADIUS),
            (left + CARD_RADIUS, bottom - CARD_RADIUS),
            (right - CARD_RADIUS, bottom - CARD_RADIUS),
        ]
        .iter()
        .any(|&(cx, cy)| {
            let in_square = (px - cx).abs() <= CARD_RADIUS
                && (py - cy).abs() <= CARD_RADIUS
                && (px < left + CARD_RADIUS || px > right - CARD_RADIUS)
                && (py < top + CARD_RADIUS || py > bottom - CARD_RADIUS);
            in_square && (px - cx).hypot(py - cy) > CARD_RADIUS + 1.0
        })
    })
}

#[test]
fn a_flattened_card_draws_its_content_like_siblings_and_drops_its_surface_passes() {
    let _scene = SCENE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    BUTTON_OFFSET[0].store(72, Ordering::Relaxed);
    BUTTON_OFFSET[1].store(0, Ordering::Relaxed);
    let Some(flattened) = capture(NESTED, false) else {
        return;
    };
    let siblings = capture(SIBLINGS, false).expect("headless WGPU init failed mid-suite");
    let isolated = capture(NESTED, true).expect("headless WGPU init failed mid-suite");

    if std::env::var("FLATTEN_DUMP").is_ok() {
        dump_delta_map(&flattened.pixels, &siblings.pixels);
    }
    if let Ok(row) = std::env::var("FLATTEN_ROW") {
        let y: usize = row.parse().expect("row");
        for (label, frame) in [("flat", &flattened.pixels), ("sibl", &siblings.pixels)] {
            let line: Vec<String> = (200..260)
                .map(|x| {
                    let p = &frame[(y * FRAME_WIDTH as usize + x) * 4..][..3];
                    format!("{:3}", (p[0] as u32 + p[1] as u32 + p[2] as u32) / 3)
                })
                .collect();
            eprintln!("{label} y={y}: {}", line.join(" "));
        }
    }
    let delta = delta_where(&flattened.pixels, &siblings.pixels, |_, _| true);
    eprintln!(
        "flattened vs siblings: max delta {}, differing {} of {}; cold passes flattened {} \
         isolated {}",
        delta.max, delta.differing, delta.compared, flattened.cold_passes, isolated.cold_passes
    );
    assert!(
        delta.max <= 1,
        "content flattened into the parent must draw like the same content placed as a \
         sibling over a bare card; a pixel moved by {} levels",
        delta.max
    );
    assert!(
        delta.differing * 50 < delta.compared,
        "{} of {} pixels differ from the sibling arrangement; rounding noise touches only \
         anti-aliased edges",
        delta.differing,
        delta.compared
    );
    assert!(
        flattened.cold_passes + 2 * CARDS.len() as u32 <= isolated.cold_passes,
        "flattening must drop at least the content surface's own passes per card: {} vs {}",
        flattened.cold_passes,
        isolated.cold_passes
    );
}

#[test]
fn a_flattened_cards_corner_shadow_stays_inside_the_rounded_clip() {
    let _scene = SCENE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    BUTTON_OFFSET[0].store(90, Ordering::Relaxed);
    BUTTON_OFFSET[1].store(6, Ordering::Relaxed);
    let Some(flattened) = capture(NESTED, false) else {
        return;
    };
    let bare = capture(NO_BUTTON, false).expect("headless WGPU init failed mid-suite");
    let isolated = capture(NESTED, true).expect("headless WGPU init failed mid-suite");
    assert!(
        flattened.cold_passes < isolated.cold_passes,
        "the corner-hugging button must still flatten: {} vs {} passes",
        flattened.cold_passes,
        isolated.cold_passes
    );

    let shadowed = delta_where(&flattened.pixels, &bare.pixels, |x, y| {
        inside_a_card(x, y) && !in_a_corner_triangle(x, y)
    });
    assert!(
        shadowed.differing > 200,
        "the button and its shadow must show inside the card: {} pixels differ",
        shadowed.differing
    );
    let leaked = delta_where(&flattened.pixels, &bare.pixels, in_a_corner_triangle);
    eprintln!(
        "corner triangles: max delta {} over {} pixels; shadowed pixels {}",
        leaked.max, leaked.compared, shadowed.differing
    );
    assert!(
        leaked.max <= 1,
        "a shadow drawn into the parent leaked {} levels into a corner outside the card's \
         rounded clip",
        leaked.max
    );
}

#[test]
fn a_nested_glass_whose_reach_enters_the_corner_cut_still_flattens_the_card_exactly() {
    let _scene = SCENE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(flattened) = capture(CORNER_CIRCLE, false) else {
        return;
    };
    let isolated = capture(CORNER_CIRCLE, true).expect("headless WGPU init failed mid-suite");
    assert!(
        flattened.cold_passes < isolated.cold_passes,
        "a nested glass only reads through the parent's clip, so its reach never blocks \
         flattening: {} vs {} passes",
        flattened.cold_passes,
        isolated.cold_passes
    );
    let siblings =
        capture(CORNER_CIRCLE_SIBLINGS, false).expect("headless WGPU init failed mid-suite");
    let inside = delta_where(&flattened.pixels, &siblings.pixels, |_, _| true);
    eprintln!(
        "corner circle: max delta {} over {} pixels, {} differing",
        inside.max, inside.compared, inside.differing
    );
    assert!(
        inside.max <= 1 && inside.differing * 50 < inside.compared,
        "a flattened card with a round glass near its corner must draw like the sibling \
         arrangement: max delta {} over {} differing pixels",
        inside.max,
        inside.differing
    );
}
