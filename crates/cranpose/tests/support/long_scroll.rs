// A screen whose every frame scrolls a long column that holds still: a
// short viewport over sixty rows, each a card with a dot, two lines of text
// and a count. The column is not lazy, so every row moves each frame while
// the viewport shows only a few: a renderer pays here for each node that
// moves, with little drawing to hide that cost. The framebuffer bench shows
// the screen with `SCREEN=scroll`.

use cranpose::prelude::*;

use super::{frame_time::frame_time, text_style::text_style as style};

const ROWS: usize = 60;
const ROW_HEIGHT: f32 = 24.0;
const VIEWPORT_HEIGHT: f32 = 96.0;
// The column moves 60 pixels a second, a pixel a frame at 60 Hz, and starts
// over before it reaches its end.
const SCROLL_PIXELS_PER_MS: f32 = 0.06;
const SCROLL_PERIOD_MS: u64 = 10_000;
const NAMES: [&str; 8] = [
    "Photos", "Audio", "Notes", "Maps", "Inbox", "Books", "Video", "Docs",
];
const DETAILS: [&str; 4] = ["12 items", "Shared", "2 days ago", "Synced"];

const BACKGROUND: Color = Color::from_rgb_u8(15, 18, 32);
const CARD: Color = Color::from_rgb_u8(27, 31, 51);
const MUTED: Color = Color::from_rgb_u8(140, 147, 176);
const FILLS: [Color; 4] = [
    Color::from_rgb_u8(67, 97, 238),
    Color::from_rgb_u8(76, 201, 240),
    Color::from_rgb_u8(247, 37, 133),
    Color::from_rgb_u8(78, 205, 196),
];

#[composable]
fn ScrolledRow(index: usize) {
    Row(
        Modifier::empty()
            .fill_max_width()
            .height(ROW_HEIGHT)
            .padding_vertical(1.0)
            .background(CARD)
            .rounded_corners(4.0)
            .padding_horizontal(4.0),
        RowSpec::default()
            .vertical_alignment(VerticalAlignment::CenterVertically)
            .horizontal_arrangement(LinearArrangement::spaced_by(4.0)),
        move || {
            Box(
                Modifier::empty()
                    .size(Size::new(10.0, 10.0))
                    .background(FILLS[index % FILLS.len()])
                    .rounded_corners(5.0),
                BoxSpec::default(),
                || {},
            );
            Column(
                Modifier::empty().weight(1.0),
                ColumnSpec::default(),
                move || {
                    Text(
                        NAMES[index % NAMES.len()],
                        Modifier::empty(),
                        style(Color::WHITE, 9.0),
                    );
                    Text(
                        DETAILS[index % DETAILS.len()],
                        Modifier::empty(),
                        style(MUTED, 7.0),
                    );
                },
            );
            Text(
                format!("{}", 3 + index * 7 % 40),
                Modifier::empty(),
                style(MUTED, 9.0),
            );
        },
    );
}

// The screen, titled `title`.
#[composable]
pub fn LongScroll(title: &'static str) {
    let scroll = remember(|| ScrollState::new(0.0)).with(|state| *state);
    frame_time(move |t| {
        scroll.scroll_to((t % SCROLL_PERIOD_MS) as f32 * SCROLL_PIXELS_PER_MS);
    });
    Column(
        Modifier::empty()
            .fill_max_size()
            .background(BACKGROUND)
            .padding(8.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(6.0)),
        move || {
            Text(title, Modifier::empty(), style(Color::WHITE, 14.0));
            Column(
                Modifier::empty()
                    .fill_max_width()
                    .height(VIEWPORT_HEIGHT)
                    .vertical_scroll(scroll, false),
                ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(2.0)),
                move || {
                    for index in 0..ROWS {
                        ScrolledRow(index);
                    }
                },
            );
        },
    );
}
