// A screen whose every frame moves deep rows that hold still: a list that
// scrolls by itself, each row a card with a shadow around nested containers,
// two lines of text, a chip and a drawn path, none of which changes. A
// renderer pays here for each node that moves, as it moves, so each
// benchmark that shows the screen keeps that cost in view: the framebuffer
// bench and the watch sprites bench with `SCREEN=moves`, and the
// ES3C35P's screens in turn.

use cranpose::prelude::*;

use super::{frame_time::frame_time, text_style::text_style as style};

const ROWS: usize = 40;
const ROW_HEIGHT: f32 = 44.0;
// The list moves 60 pixels a second, a pixel a frame at 60 Hz, and starts
// over after a third of its rows, so it never reaches its end.
const SCROLL_PIXELS_PER_MS: f32 = 0.06;
const SCROLL_PERIOD_MS: u64 = 10_000;
const NAMES: [&str; 8] = [
    "Photos", "Audio", "Notes", "Maps", "Inbox", "Books", "Video", "Docs",
];
const DETAILS: [&str; 4] = ["12 items", "Shared", "2 days ago", "Synced"];
const TAGS: [&str; 4] = ["new", "work", "home", "pinned"];

const BACKGROUND: Color = Color::from_rgb_u8(15, 18, 32);
const CARD: Color = Color::from_rgb_u8(27, 31, 51);
const WELL: Color = Color::from_rgb_u8(42, 48, 80);
const MUTED: Color = Color::from_rgb_u8(140, 147, 176);
const FILLS: [Color; 4] = [
    Color::from_rgb_u8(67, 97, 238),
    Color::from_rgb_u8(76, 201, 240),
    Color::from_rgb_u8(247, 37, 133),
    Color::from_rgb_u8(78, 205, 196),
];

// A mark in a well: a filled triangle under a zigzag line, the shapes of
// row `index`.
fn mark(index: usize) -> Modifier {
    let fill = FILLS[index % FILLS.len()];
    let rise = (index % 5) as f32;
    Modifier::empty()
        .size(Size::new(24.0, 24.0))
        .background(WELL)
        .rounded_corners(6.0)
        .draw_behind(move |scope| {
            let size = scope.size();
            let mut area = Path::new();
            area.move_to(Point::new(3.0, size.height - 3.0));
            area.line_to(Point::new(size.width / 2.0, 4.0 + rise));
            area.line_to(Point::new(size.width - 3.0, size.height - 3.0));
            area.close();
            scope.draw_path(&area, Brush::solid(fill.with_alpha(0.5)), DrawStyle::Fill);
            let mut line = Path::new();
            for step in 0..5 {
                let point = Point::new(
                    3.0 + step as f32 * (size.width - 6.0) / 4.0,
                    if step % 2 == 0 { 14.0 } else { 9.0 + rise },
                );
                if step == 0 {
                    line.move_to(point);
                } else {
                    line.line_to(point);
                }
            }
            scope.draw_path(
                &line,
                Brush::solid(Color::WHITE),
                DrawStyle::Stroke(Stroke::new(1.5)),
            );
        })
}

// A card with a shadow, holding a mark, a name over a detail and a tag
// chip, and a count: containers nest as cards and fields do.
#[composable]
fn MovingRow(index: usize) {
    let kind = index % NAMES.len();
    let shape = RoundedCornerShape::uniform(8.0);
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(ROW_HEIGHT)
            .padding_vertical(3.0),
        BoxSpec::default(),
        move || {
            Row(
                Modifier::empty()
                    .fill_max_size()
                    .shadow_with(
                        2.0,
                        LayerShape::Rounded(shape),
                        false,
                        Color::BLACK,
                        Color::BLACK,
                    )
                    .background(CARD)
                    .rounded_corners(8.0)
                    .padding_horizontal(6.0),
                RowSpec::default()
                    .vertical_alignment(VerticalAlignment::CenterVertically)
                    .horizontal_arrangement(LinearArrangement::spaced_by(6.0)),
                move || {
                    Box(mark(index), BoxSpec::default(), || {});
                    Column(
                        Modifier::empty().weight(1.0),
                        ColumnSpec::default(),
                        move || {
                            Text(NAMES[kind], Modifier::empty(), style(Color::WHITE, 10.0));
                            Row(
                                Modifier::empty(),
                                RowSpec::default()
                                    .horizontal_arrangement(LinearArrangement::spaced_by(4.0)),
                                move || {
                                    Text(
                                        DETAILS[index % DETAILS.len()],
                                        Modifier::empty(),
                                        style(MUTED, 8.0),
                                    );
                                    Text(
                                        TAGS[index % TAGS.len()],
                                        Modifier::empty()
                                            .background(WELL)
                                            .rounded_corners(4.0)
                                            .padding_horizontal(3.0),
                                        style(FILLS[index % FILLS.len()], 8.0),
                                    );
                                },
                            );
                        },
                    );
                    Text(
                        format!("{}", 3 + index * 7 % 40),
                        Modifier::empty(),
                        style(MUTED, 10.0),
                    );
                },
            );
        },
    );
}

// The screen, titled `title`.
#[composable]
pub fn DeepMoves(title: &'static str) {
    let list = rememberLazyListState();
    frame_time(move |t| {
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
            LazyColumn(
                Modifier::empty()
                    .fill_max_width()
                    .weight(1.0)
                    .clip_to_bounds(),
                list,
                LazyColumnSpec::default(),
                move |scope| {
                    scope.items(LazyItems::new(ROWS), MovingRow);
                },
            );
        },
    );
}
