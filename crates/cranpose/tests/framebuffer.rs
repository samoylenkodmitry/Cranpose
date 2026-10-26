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

// A host that keeps its frame and draws only the changed areas, in strips
// of eight rows as a board with a small buffer does, shows what a whole
// redraw shows, every frame of a screen that changes most of itself each
// frame and gains and loses list rows as it scrolls.
#[test]
fn the_dashboard_drawn_by_changed_areas_matches_whole_redraws() {
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
    let mut kept = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    let mut redrawn = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    let mut area_pixels = Vec::new();
    let mut areas = Vec::new();
    for frame in 0..240u64 {
        framebuffer.tick(frame * 16_666_667);
        framebuffer.run_pending_tasks();
        framebuffer.changed_areas(&mut areas);
        for area in &areas {
            for top in (area.y..area.y + area.height).step_by(8) {
                let strip = PixelRect {
                    y: top,
                    height: 8.min(area.y + area.height - top),
                    ..*area
                };
                area_pixels.resize(strip.width as usize * strip.height as usize * 4, 0);
                framebuffer.draw_area(strip, &mut area_pixels);
                let row_bytes = strip.width as usize * 4;
                for (row, source) in area_pixels.chunks_exact(row_bytes).enumerate() {
                    let start = ((strip.y as usize + row) * WIDTH as usize + strip.x as usize) * 4;
                    kept[start..start + row_bytes].copy_from_slice(source);
                }
            }
        }
        framebuffer.draw_area(whole, &mut redrawn);
        let differing = kept
            .as_chunks::<4>()
            .0
            .iter()
            .zip(redrawn.as_chunks::<4>().0)
            .position(|(a, b)| a != b);
        assert_eq!(
            differing.map(|pixel| (pixel % WIDTH as usize, pixel / WIDTH as usize)),
            None,
            "frame {frame}: the kept frame differs from a whole redraw at (x, y); areas {areas:?}"
        );
    }
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
