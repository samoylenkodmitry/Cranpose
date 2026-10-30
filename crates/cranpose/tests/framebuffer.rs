use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use cranpose::{
    framebuffer::{Framebuffer, PixelRect, Rgb565Order},
    prelude::*,
};
use cranpose_animation::{
    AnimationSpec, RepeatMode, StartOffset, infiniteRepeatable, rememberInfiniteTransition,
};

#[path = "support/dashboard.rs"]
mod dashboard;

const RED: Color = Color(1.0, 0.0, 0.0, 1.0);
const GREEN: Color = Color(0.0, 1.0, 0.0, 1.0);

fn center(framebuffer: &mut Framebuffer) -> [u8; 4] {
    framebuffer.changed_areas(&mut Vec::new());
    let area = PixelRect {
        x: framebuffer.width() / 2,
        y: framebuffer.height() / 2,
        width: 1,
        height: 1,
    };
    let mut pixel = [0; 4];
    framebuffer.draw_area(area, &mut pixel);
    pixel
}

#[test]
fn the_host_theme_reaches_content() {
    for (theme, dark) in [(SystemTheme::Dark, true), (SystemTheme::Light, false)] {
        let seen = Rc::new(Cell::new(None));
        let sink = Rc::clone(&seen);
        let mut framebuffer = Framebuffer::new(40, 30, 1.0, theme, &[], move || {
            sink.set(Some(isSystemInDarkTheme()));
        })
        .expect("valid surface");
        framebuffer.set_active(true);
        framebuffer.tick(1);
        assert_eq!(seen.get(), Some(dark));
    }
}

#[test]
fn a_still_picture_lets_the_host_sleep() {
    let mut framebuffer = Framebuffer::new(40, 30, 1.0, SystemTheme::Light, &[], || {
        Spacer(Modifier::empty().fill_max_size().background(RED));
    })
    .expect("valid surface");
    framebuffer.set_active(true);
    assert!(framebuffer.tick(1));
    assert_eq!(center(&mut framebuffer), [255, 0, 0, 255]);
    let schedule = framebuffer.frame_schedule();
    assert!(!schedule.needs_update);
    assert!(!schedule.needs_frame);
    assert_eq!(schedule.next_deadline, None);
    assert!(!framebuffer.tick(16_000_001));
    let mut areas = Vec::new();
    framebuffer.changed_areas(&mut areas);
    assert!(
        areas.is_empty(),
        "a still picture changes nothing: {areas:?}"
    );
}

#[test]
fn an_animation_asks_for_the_next_frame() {
    let mut framebuffer = Framebuffer::new(40, 30, 1.0, SystemTheme::Light, &[], || {
        let transition = rememberInfiniteTransition("sweep");
        let spec = infiniteRepeatable(
            AnimationSpec::linear(100),
            RepeatMode::Restart,
            StartOffset::default(),
        );
        let fraction = transition.animateFloat(0.0, 1.0, spec, "fraction").value();
        Spacer(
            Modifier::empty()
                .fill_max_width_fraction(fraction)
                .fill_max_height()
                .background(RED),
        );
    })
    .expect("valid surface");
    framebuffer.set_active(true);
    framebuffer.tick(1);
    let schedule = framebuffer.frame_schedule();
    assert!(schedule.needs_frame || schedule.needs_update);
    assert!(framebuffer.tick(60_000_000));
}

#[test]
fn a_delay_wakes_a_sleeping_host_and_its_change_presents() {
    let mut framebuffer = Framebuffer::new(40, 30, 1.0, SystemTheme::Light, &[], || {
        let done = rememberMutableStateOf(|| false);
        LaunchedEffectAsync((), move |_| {
            Box::pin(async move {
                delay(Duration::from_millis(20)).await;
                done.set(true);
            })
        });
        let color = if done.get() { GREEN } else { RED };
        Spacer(Modifier::empty().fill_max_size().background(color));
    })
    .expect("valid surface");
    let woken = Arc::new(AtomicBool::new(false));
    let sink = Arc::clone(&woken);
    framebuffer.set_frame_waker(move || sink.store(true, Ordering::Release));
    framebuffer.set_active(true);
    framebuffer.tick(1);
    assert_eq!(center(&mut framebuffer), [255, 0, 0, 255]);
    let schedule = framebuffer.frame_schedule();
    assert!(!schedule.needs_frame);

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut frame_time = 1;
    while center(&mut framebuffer) != [0, 255, 0, 255] {
        assert!(Instant::now() < deadline, "the delay never woke the host");
        if woken.swap(false, Ordering::AcqRel) {
            framebuffer.run_pending_tasks();
            frame_time += 16_000_000;
            framebuffer.tick(frame_time);
        } else {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

/// Draws the whole picture of `framebuffer` in strips of `rows` rows, as a
/// host with a small buffer does.
fn picture_in_strips(framebuffer: &mut Framebuffer, rows: u32) -> Vec<u8> {
    let (width, height) = (framebuffer.width(), framebuffer.height());
    let mut picture = vec![0; width as usize * height as usize * 4];
    let mut top = 0;
    while top < height {
        let area = PixelRect {
            x: 0,
            y: top,
            width,
            height: rows.min(height - top),
        };
        let start = top as usize * width as usize * 4;
        let len = area.width as usize * area.height as usize * 4;
        framebuffer.draw_area(area, &mut picture[start..start + len]);
        top += area.height;
    }
    picture
}

fn text_framebuffer() -> Framebuffer {
    let mut framebuffer = Framebuffer::new(120, 60, 1.0, SystemTheme::Light, &[], || {
        Column(
            Modifier::empty().padding(3.0),
            ColumnSpec::default(),
            || {
                Text(
                    "Strips Ag",
                    Modifier::empty(),
                    TextStyle {
                        span_style: SpanStyle {
                            font_size: text::TextUnit::Sp(24.0),
                            ..SpanStyle::default()
                        },
                        ..TextStyle::default()
                    },
                );
                Text(
                    "cut through glyphs",
                    Modifier::empty(),
                    TextStyle::default(),
                );
            },
        );
    })
    .expect("valid surface");
    framebuffer.set_active(true);
    assert!(framebuffer.tick(1));
    framebuffer.changed_areas(&mut Vec::new());
    framebuffer
}

#[test]
fn text_drawn_in_strips_matches_text_drawn_whole() {
    let mut framebuffer = text_framebuffer();
    let whole = picture_in_strips(&mut framebuffer, 60);
    assert!(
        whole.as_chunks::<4>().0.iter().any(|pixel| pixel[0] < 128),
        "the text draws"
    );
    for rows in [1, 3, 8] {
        assert!(
            picture_in_strips(&mut framebuffer, rows) == whole,
            "strips of {rows} rows draw the picture whole strips draw"
        );
    }
}

#[test]
fn preparing_an_area_draws_the_same_picture() {
    let mut drawn = text_framebuffer();
    let whole = picture_in_strips(&mut drawn, 60);
    let mut prepared = text_framebuffer();
    let area = PixelRect {
        x: 0,
        y: 0,
        width: prepared.width(),
        height: prepared.height(),
    };
    prepared.prepare_area(area);
    assert!(picture_in_strips(&mut prepared, 8) == whole);
}

// A screen at step `first` that goes to step 1 on its second frame. The
// step changes a line of text, so that frame composes and lays out, and
// colors the draws of three boxes: one that only draws, one after the text
// that the wider line moves, and one whose width the step sets.
fn stepping_framebuffer(first: u32) -> Framebuffer {
    let mut framebuffer = Framebuffer::new(80, 40, 1.0, SystemTheme::Light, &[], move || {
        let step = rememberMutableStateOf(|| first);
        LaunchedEffectAsync((), move |scope| {
            Box::pin(async move {
                scope.runtime().frame_clock().next_frame().await;
                step.set(1);
            })
        });
        let colored = move |width: f32| {
            Modifier::empty()
                .size(Size::new(width, 8.0))
                .draw_behind(move |scope| {
                    let color = if step.get() == 0 { RED } else { GREEN };
                    scope.draw_rect(Brush::solid(color));
                })
        };
        Column(
            Modifier::empty().fill_max_size(),
            ColumnSpec::default(),
            move || {
                Row(Modifier::empty(), RowSpec::default(), move || {
                    let line = if step.get() == 0 {
                        "a"
                    } else {
                        "a longer line"
                    };
                    Text(line, Modifier::empty(), TextStyle::default());
                    Box(colored(8.0), BoxSpec::default(), || {});
                });
                Box(colored(20.0), BoxSpec::default(), || {});
                let width = if step.get() == 0 { 10.0 } else { 30.0 };
                Box(colored(width), BoxSpec::default(), || {});
            },
        );
    })
    .expect("valid surface");
    framebuffer.set_active(true);
    for frame in 1..=4 {
        framebuffer.tick(frame * 16_666_667);
        framebuffer.run_pending_tasks();
    }
    framebuffer.changed_areas(&mut Vec::new());
    framebuffer
}

// A node whose draws read changed state draws them again in a frame that
// also composed and laid out other nodes, as it does where it moved or
// resized in that frame.
#[test]
fn draws_that_change_with_a_layout_show_as_a_new_screen_shows_them() {
    let mut stepped = stepping_framebuffer(0);
    let mut fresh = stepping_framebuffer(1);
    let picture = picture_in_strips(&mut fresh, 40);
    assert!(
        picture.as_chunks::<4>().0.contains(&[0, 255, 0, 255]),
        "the boxes draw"
    );
    assert!(picture_in_strips(&mut stepped, 40) == picture);
}

// The states of a growing screen: how many bars its box draws, whether its
// column holds two more boxes, and whether its last box draws green.
#[derive(Clone, Copy)]
struct Growth {
    bars: MutableState<u32>,
    grown: MutableState<bool>,
    green: MutableState<bool>,
}

// A box that draws `bars` green bars side by side.
fn drawn_bars(bars: MutableState<u32>) -> Modifier {
    Modifier::empty()
        .size(Size::new(40.0, 10.0))
        .draw_behind(move |scope| {
            for index in 0..bars.get() {
                scope.draw_rect_at(
                    Rect {
                        x: index as f32 * 12.0,
                        y: 0.0,
                        width: 10.0,
                        height: 10.0,
                    },
                    Brush::solid(GREEN),
                );
            }
        })
}

// A bar that draws green when `green`, else red.
fn drawn_bar(green: MutableState<bool>) -> Modifier {
    Modifier::empty()
        .size(Size::new(30.0, 6.0))
        .draw_behind(move |scope| {
            let color = if green.get() { GREEN } else { RED };
            scope.draw_rect(Brush::solid(color));
        })
}

// A screen of a box that draws `bars` bars beside a box that stays, a column
// that holds two more boxes between the two it keeps when `grown`, and a box
// below that the column moves down, drawn green when `green`. The screen
// keeps its states in `held` for the test to change, and its first picture
// is drawn.
fn growing_framebuffer(
    (bars, grown, green): (u32, bool, bool),
    held: Rc<Cell<Option<Growth>>>,
) -> Framebuffer {
    let mut framebuffer = Framebuffer::new(80, 80, 1.0, SystemTheme::Light, &[], move || {
        let growth = remember(|| Growth {
            bars: mutableStateOf(bars),
            grown: mutableStateOf(grown),
            green: mutableStateOf(green),
        })
        .with(|growth| *growth);
        held.set(Some(growth));
        let bar = |color: Color| {
            Modifier::empty()
                .size(Size::new(30.0, 6.0))
                .background(color)
        };
        Column(
            Modifier::empty().fill_max_size(),
            ColumnSpec::default(),
            move || {
                Row(Modifier::empty(), RowSpec::default(), move || {
                    Box(drawn_bars(growth.bars), BoxSpec::default(), || {});
                    Box(bar(RED), BoxSpec::default(), || {});
                });
                Column(Modifier::empty(), ColumnSpec::default(), move || {
                    Box(bar(RED), BoxSpec::default(), || {});
                    if growth.grown.get() {
                        Box(bar(GREEN), BoxSpec::default(), || {});
                        Box(bar(GREEN), BoxSpec::default(), || {});
                    }
                    Box(bar(RED), BoxSpec::default(), || {});
                });
                Box(drawn_bar(growth.green), BoxSpec::default(), || {});
            },
        );
    })
    .expect("valid surface");
    framebuffer.set_active(true);
    framebuffer.tick(16_666_667);
    framebuffer.changed_areas(&mut Vec::new());
    framebuffer
}

// A kept picture whose layers come to draw more or fewer items than before,
// or to hold more or fewer child layers, shows what a new screen shows, at
// each of its steps.
#[test]
fn layers_that_draw_more_or_less_show_as_a_new_screen_shows_them() {
    let held = Rc::new(Cell::new(None));
    let mut kept = growing_framebuffer((1, false, false), Rc::clone(&held));
    let growth = held.get().expect("the screen holds its states");
    let steps = [
        (3, false, false),
        (3, false, true),
        (3, true, true),
        (1, true, false),
        (2, false, true),
    ];
    for (frame, step) in (2..).zip(steps) {
        let (bars, grown, green) = step;
        growth.bars.set(bars);
        growth.grown.set(grown);
        growth.green.set(green);
        assert!(
            kept.tick(frame * 16_666_667),
            "the step changes the picture"
        );
        kept.changed_areas(&mut Vec::new());
        let mut fresh = growing_framebuffer(step, Rc::new(Cell::new(None)));
        assert!(
            picture_in_strips(&mut kept, 80) == picture_in_strips(&mut fresh, 80),
            "{step:?}: the kept picture is the new screen's"
        );
    }
}

// A frame a host keeps: the areas a framebuffer changes are drawn into it,
// in strips of eight rows as a board with a small buffer draws them.
struct KeptFrame {
    width: u32,
    kept: Vec<u8>,
    redrawn: Vec<u8>,
    strip: Vec<u8>,
    areas: Vec<PixelRect>,
}

impl KeptFrame {
    fn new(width: u32, height: u32) -> Self {
        let bytes = (width * height * 4) as usize;
        Self {
            width,
            kept: vec![0; bytes],
            redrawn: vec![0; bytes],
            strip: Vec::new(),
            areas: Vec::new(),
        }
    }

    // Draws the areas `framebuffer` changed into the kept frame, and checks
    // that it shows what a whole redraw shows.
    fn draw_changed_areas(&mut self, framebuffer: &mut Framebuffer, frame: u64) {
        let width = self.width;
        framebuffer.changed_areas(&mut self.areas);
        for area in &self.areas {
            for top in (area.y..area.y + area.height).step_by(8) {
                let strip = PixelRect {
                    y: top,
                    height: 8.min(area.y + area.height - top),
                    ..*area
                };
                self.strip
                    .resize(strip.width as usize * strip.height as usize * 4, 0);
                framebuffer.draw_area(strip, &mut self.strip);
                let row_bytes = strip.width as usize * 4;
                for (row, source) in self.strip.chunks_exact(row_bytes).enumerate() {
                    let start = ((strip.y as usize + row) * width as usize + strip.x as usize) * 4;
                    self.kept[start..start + row_bytes].copy_from_slice(source);
                }
            }
        }
        let whole = PixelRect {
            x: 0,
            y: 0,
            width,
            height: framebuffer.height(),
        };
        framebuffer.draw_area(whole, &mut self.redrawn);
        let differing = self
            .kept
            .as_chunks::<4>()
            .0
            .iter()
            .zip(self.redrawn.as_chunks::<4>().0)
            .position(|(a, b)| a != b);
        assert_eq!(
            differing.map(|pixel| (pixel % width as usize, pixel / width as usize)),
            None,
            "frame {frame}: the kept frame differs from a whole redraw at (x, y); areas {:?}",
            self.areas
        );
    }
}

// A host that keeps its frame and draws only the changed areas shows what a
// whole redraw shows, every frame of a screen that changes most of itself
// each frame and gains and loses list rows as it scrolls.
#[test]
fn the_dashboard_drawn_by_changed_areas_matches_whole_redraws() {
    const WIDTH: u32 = 320;
    const HEIGHT: u32 = 480;
    let mut framebuffer = Framebuffer::new(WIDTH, HEIGHT, 1.0, SystemTheme::Dark, &[], || {
        dashboard::Dashboard("Cranpose");
    })
    .expect("valid surface");
    framebuffer.set_active(true);
    let mut kept = KeptFrame::new(WIDTH, HEIGHT);
    for frame in 0..240u64 {
        framebuffer.tick(frame * 16_666_667);
        framebuffer.run_pending_tasks();
        kept.draw_changed_areas(&mut framebuffer, frame);
    }
}

// A label that changes in a box as wide as the screen changes the pixels
// across its glyphs only, wherever the box aligns them, and a kept frame
// shows it as a whole redraw does while it grows and shrinks.
#[test]
fn a_changing_label_in_a_wide_box_changes_only_the_area_of_its_glyphs() {
    const WIDTH: u32 = 240;
    const HEIGHT: u32 = 40;
    let held = Rc::new(Cell::new(None));
    let sink = Rc::clone(&held);
    let mut framebuffer =
        Framebuffer::new(WIDTH, HEIGHT, 1.0, SystemTheme::Light, &[], move || {
            let count = remember(|| mutableStateOf(1u32)).with(|count| *count);
            sink.set(Some(count));
            Column(
                Modifier::empty().fill_max_width(),
                ColumnSpec::default(),
                move || {
                    for text_align in [text::TextAlign::Start, text::TextAlign::Center] {
                        let style = TextStyle {
                            paragraph_style: ParagraphStyle {
                                text_align,
                                ..ParagraphStyle::default()
                            },
                            ..TextStyle::default()
                        };
                        Text(
                            count.get().to_string(),
                            Modifier::empty().fill_max_width(),
                            style,
                        );
                    }
                },
            );
        })
        .expect("valid surface");
    framebuffer.set_active(true);
    framebuffer.tick(16_666_667);
    let mut kept = KeptFrame::new(WIDTH, HEIGHT);
    kept.draw_changed_areas(&mut framebuffer, 1);
    let count = held.get().expect("the screen holds its count");
    for (frame, value) in (2..).zip([1000, 7, 52, 52_000, 3]) {
        count.set(value);
        framebuffer.tick(frame * 16_666_667);
        kept.draw_changed_areas(&mut framebuffer, frame);
        assert!(!kept.areas.is_empty(), "frame {frame}: the label changes");
        for area in &kept.areas {
            assert!(
                area.width <= WIDTH / 4,
                "frame {frame}: {area:?} reaches past the glyphs of {value}"
            );
        }
    }
}

// The rightmost column of a label in a box as wide as the screen, drawn
// black over white with `shadow`, that is not white.
fn rightmost_ink(shadow: Option<text::Shadow>) -> Option<u32> {
    const WIDTH: u32 = 120;
    const HEIGHT: u32 = 30;
    let mut framebuffer =
        Framebuffer::new(WIDTH, HEIGHT, 1.0, SystemTheme::Light, &[], move || {
            let style = TextStyle {
                span_style: SpanStyle {
                    color: Some(Color::BLACK),
                    shadow,
                    ..SpanStyle::default()
                },
                ..TextStyle::default()
            };
            Box(
                Modifier::empty().fill_max_size().background(Color::WHITE),
                BoxSpec::default(),
                move || {
                    Text("1", Modifier::empty().fill_max_width(), style.clone());
                },
            );
        })
        .expect("valid surface");
    framebuffer.set_active(true);
    framebuffer.tick(16_666_667);
    framebuffer.changed_areas(&mut Vec::new());
    let mut picture = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    framebuffer.draw_area(
        PixelRect {
            x: 0,
            y: 0,
            width: WIDTH,
            height: HEIGHT,
        },
        &mut picture,
    );
    picture
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .filter(|(_, pixel)| **pixel != [255, 255, 255, 255])
        .map(|(index, _)| index as u32 % WIDTH)
        .max()
}

// A blurred shadow spreads past the glyphs of its label, wherever the
// label's box lets it.
#[test]
fn a_blurred_shadow_shows_past_the_glyphs_of_a_label_in_a_wide_box() {
    let glyphs = rightmost_ink(None).expect("the label draws");
    let shadow = text::Shadow {
        color: Color::BLACK,
        offset: Point::new(0.0, 0.0),
        blur_radius: 8.0,
    };
    let shadowed = rightmost_ink(Some(shadow)).expect("the label draws");
    assert!(
        shadowed > glyphs + 4,
        "the shadow ends at column {shadowed}, the glyphs at {glyphs}"
    );
}

// A display of 16-bit pixels shows what the bytes show, cut to 5, 6 and 5
// bits, but for two things, each a step of a channel at most: a drawing that
// blends over another blends over the 16-bit pixel, and a gradient comes
// undithered, as the dither of its bytes moves less than those bits keep.
// On the dashboard two such steps meet at most.
#[test]
fn the_dashboard_drawn_as_rgb565_keeps_the_high_bits_of_its_bytes() {
    const WIDTH: u32 = 320;
    const HEIGHT: u32 = 480;
    let mut framebuffer = Framebuffer::new(WIDTH, HEIGHT, 1.0, SystemTheme::Dark, &[], || {
        dashboard::Dashboard("Cranpose");
    })
    .expect("valid surface");
    framebuffer.set_active(true);
    let whole = PixelRect {
        x: 0,
        y: 0,
        width: WIDTH,
        height: HEIGHT,
    };
    let pixels = (WIDTH * HEIGHT) as usize;
    let mut bytes = vec![0u8; pixels * 4];
    let mut little = vec![0u16; pixels];
    let mut big = vec![0u16; pixels];
    for frame in [0u64, 45, 120] {
        framebuffer.tick(frame * 16_666_667);
        framebuffer.run_pending_tasks();
        framebuffer.changed_areas(&mut Vec::new());
        framebuffer.draw_area(whole, &mut bytes);
        framebuffer.draw_area_rgb565(whole, &mut little, Rgb565Order::LittleEndian);
        framebuffer.draw_area_rgb565(whole, &mut big, Rgb565Order::BigEndian);
        let mut stepped = 0;
        for ((rgba, &little), &big) in bytes.as_chunks::<4>().0.iter().zip(&little).zip(&big) {
            assert_eq!(little.to_le_bytes(), big.to_be_bytes(), "frame {frame}");
            let cut = [rgba[0] >> 3, rgba[1] >> 2, rgba[2] >> 3];
            let shown =
                [little >> 11, (little >> 5) & 0x3F, little & 0x1F].map(|channel| channel as u8);
            if cut != shown {
                stepped += 1;
                for (cut, shown) in cut.into_iter().zip(shown) {
                    assert!(
                        cut.abs_diff(shown) <= 2,
                        "frame {frame}: {rgba:?} shown as {little:#06x}"
                    );
                }
            }
        }
        assert!(
            stepped < pixels / 20,
            "frame {frame}: {stepped} pixels differ"
        );
    }
}
