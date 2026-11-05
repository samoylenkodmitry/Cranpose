// A screen whose every frame changes nodes that hold a change and also move
// or fade: a list that scrolls by itself, each row a chain of drawn
// containers around a bar that changes every frame, under a card whose
// alpha changes every frame over rows like them. A renderer that
// lowers such a path again each frame pays for every container on it, so
// each benchmark that shows the screen keeps that cost in view: the
// framebuffer bench and the watch sprites bench with `SCREEN=paths`, and
// the ES3C35P's screens in turn. The classic ESP32 runs out of heap on it.

use cranpose::prelude::*;

use super::{
    frame_time::{Time, frame_time},
    text_style::text_style as style,
};

const ROWS: usize = 40;
// The rows of the card that fades, after the list's rows.
const CARD_ROWS: usize = 3;
const ROW_HEIGHT: f32 = 30.0;
// The list moves 60 pixels a second, a pixel a frame at 60 Hz, and starts
// over after half its rows, so it never reaches its end.
const SCROLL_PIXELS_PER_MS: f32 = 0.06;
const SCROLL_PERIOD_MS: u64 = 10_000;
const NAMES: [&str; 8] = [
    "Photos", "Audio", "Notes", "Maps", "Inbox", "Books", "Video", "Docs",
];
const INITIALS: [&str; 8] = ["P", "A", "N", "M", "I", "B", "V", "D"];

const BACKGROUND: Color = Color::from_rgb_u8(15, 18, 32);
const CARD: Color = Color::from_rgb_u8(27, 31, 51);
const TRACK: Color = Color::from_rgb_u8(42, 48, 80);
const LINE: Color = Color::from_rgb_u8(60, 68, 110);
const MUTED: Color = Color::from_rgb_u8(140, 147, 176);
const FILLS: [Color; 4] = [
    Color::from_rgb_u8(67, 97, 238),
    Color::from_rgb_u8(76, 201, 240),
    Color::from_rgb_u8(247, 37, 133),
    Color::from_rgb_u8(78, 205, 196),
];

// A level from 0 to 1 that rises and falls over `period` milliseconds,
// `phase` milliseconds ahead.
fn level(t: u64, period: u64, phase: u64) -> f32 {
    let at = (t + phase) % period;
    let half = period / 2;
    let rise = if at < half { at } else { period - at };
    rise as f32 / half as f32
}

// A track whose fill changes every frame: the change at the end of each
// path.
fn live_bar(index: usize, time: Time) -> Modifier {
    let fill = FILLS[index % FILLS.len()];
    let period = 1_200 + 97 * index as u64;
    let phase = 131 * index as u64;
    Modifier::empty()
        .fill_max_width()
        .height(2.0)
        .background(TRACK)
        .draw_behind(move |scope| {
            let size = scope.size();
            scope.draw_rect_at(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: size.width * level(time.get(), period, phase),
                    height: size.height,
                },
                Brush::solid(fill),
            );
        })
}

// An avatar, a name and a live bar in a well inside a frame, on a card: the
// containers nest as cards and fields do, and each draws. They draw fills,
// whose rows cost little beside the work on the nodes: a rounded stroke
// costs the ESP32-S3 more per pixel than the rest of the frame.
#[composable]
fn PathRow(index: usize, time: Time) {
    let kind = index % NAMES.len();
    Row(
        Modifier::empty()
            .fill_max_width()
            .height(ROW_HEIGHT)
            .padding_vertical(1.0)
            .background(CARD)
            .rounded_corners(6.0)
            .padding_horizontal(6.0),
        RowSpec::default()
            .vertical_alignment(VerticalAlignment::CenterVertically)
            .horizontal_arrangement(LinearArrangement::spaced_by(6.0)),
        move || {
            Box(
                Modifier::empty()
                    .size(Size::new(16.0, 16.0))
                    .background(FILLS[index % FILLS.len()])
                    .rounded_corners(8.0),
                BoxSpec::default().content_alignment(Alignment::CENTER),
                move || {
                    Text(INITIALS[kind], Modifier::empty(), style(Color::WHITE, 9.0));
                },
            );
            Column(
                Modifier::empty().weight(1.0),
                ColumnSpec::default(),
                move || {
                    Text(NAMES[kind], Modifier::empty(), style(MUTED, 10.0));
                    Box(
                        Modifier::empty()
                            .fill_max_width()
                            .background(LINE)
                            .rounded_corners(4.0)
                            .padding(1.0),
                        BoxSpec::default(),
                        move || {
                            Box(
                                Modifier::empty()
                                    .fill_max_width()
                                    .background(BACKGROUND)
                                    .rounded_corners(3.0)
                                    .padding(2.0),
                                BoxSpec::default(),
                                move || {
                                    Box(live_bar(index, time), BoxSpec::default(), || {});
                                },
                            );
                        },
                    );
                },
            );
        },
    );
}

// The screen, titled `title`.
#[composable]
pub fn ChangingPaths(title: &'static str) {
    let list = rememberLazyListState();
    let time = frame_time(move |t| {
        let scroll = (t % SCROLL_PERIOD_MS) as f32 * SCROLL_PIXELS_PER_MS;
        list.scroll_to_item((scroll / ROW_HEIGHT) as usize, scroll % ROW_HEIGHT);
    });
    Column(
        Modifier::empty()
            .fill_max_size()
            .background(BACKGROUND)
            .padding(8.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(6.0)),
        move || {
            Text(title, Modifier::empty(), style(Color::WHITE, 14.0));
            // A card whose alpha changes every frame, over rows that change
            // too.
            Column(
                Modifier::empty()
                    .fill_max_width()
                    .graphics_layer_block(move |layer| {
                        layer.alpha = 0.4 + 0.6 * level(time.get(), 2_000, 0);
                    })
                    .background(LINE)
                    .rounded_corners(8.0)
                    .padding(4.0),
                ColumnSpec::default(),
                move || {
                    for index in ROWS..ROWS + CARD_ROWS {
                        PathRow(index, time);
                    }
                },
            );
            LazyColumn(
                Modifier::empty()
                    .fill_max_width()
                    .weight(1.0)
                    .clip_to_bounds(),
                list,
                LazyColumnSpec::default(),
                move |scope| {
                    scope.items(LazyItems::new(ROWS), move |index| PathRow(index, time));
                },
            );
        },
    );
}
