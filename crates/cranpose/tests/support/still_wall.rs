// A screen of many small items that hold still while one line changes every
// frame: a wall of tiles, each a rounded box with a label, under a clock
// whose text changes each frame. A renderer that pays for a change of one
// node in proportion to the whole screen, or that holds much per node,
// shows it here, so each benchmark that shows the screen keeps that cost in
// view: the framebuffer bench and the watch sprites bench with
// `SCREEN=wall`, and the ES3C35P's screens in turn.

use cranpose::prelude::*;

#[path = "frame_time.rs"]
mod frame_time;
#[path = "text_style.rs"]
mod text_style;

use frame_time::{Time, frame_time};
use text_style::text_style as style;

const COLUMNS: usize = 8;
const ROWS: usize = 12;
const TILE_HEIGHT: f32 = 28.0;
const LABELS: [&str; 6] = ["Ab", "Cd", "Ef", "Gh", "Ij", "Kl"];

const BACKGROUND: Color = Color::from_rgb_u8(15, 18, 32);
const FILLS: [Color; 4] = [
    Color::from_rgb_u8(67, 97, 238),
    Color::from_rgb_u8(76, 201, 240),
    Color::from_rgb_u8(247, 37, 133),
    Color::from_rgb_u8(78, 205, 196),
];

#[composable]
fn Tile(index: usize) {
    Box(
        Modifier::empty()
            .weight(1.0)
            .height(TILE_HEIGHT)
            .background(FILLS[index % FILLS.len()])
            .rounded_corners(4.0),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || {
            Text(
                LABELS[index % LABELS.len()],
                Modifier::empty(),
                style(Color::WHITE, 9.0),
            );
        },
    );
}

// The clock: minutes, seconds and milliseconds since the first frame, new
// text every frame.
#[composable]
fn Clock(time: Time) {
    let t = time.get();
    Text(
        format!("{:02}:{:02}.{:03}", t / 60_000, t / 1_000 % 60, t % 1_000),
        Modifier::empty(),
        style(Color::WHITE, 12.0),
    );
}

// The screen, titled `title`.
#[composable]
pub fn StillWall(title: &'static str) {
    let time = frame_time(|_| {});
    Column(
        Modifier::empty()
            .fill_max_size()
            .background(BACKGROUND)
            .padding(8.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(4.0)),
        move || {
            Text(title, Modifier::empty(), style(Color::WHITE, 14.0));
            Clock(time);
            for row in 0..ROWS {
                Row(
                    Modifier::empty().fill_max_width(),
                    RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(4.0)),
                    move || {
                        for column in 0..COLUMNS {
                            Tile(row * COLUMNS + column);
                        }
                    },
                );
            }
        },
    );
}
