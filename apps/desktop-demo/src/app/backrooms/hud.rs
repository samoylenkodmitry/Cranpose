// What the camcorder shows over the backrooms: REC with a blinking dot, the
// room it is in, the time it has run, a clock nobody set, the corners of
// its frame and the dark rim of its lens. For the first seconds a line at
// the bottom tells how to walk; for a few seconds after each doorway, a
// caption tells what it heard.

use cranpose_core::derivedStateOf;
use cranpose_ui::{
    composable,
    text::{FontFamily, TextUnit},
    Alignment, Box, BoxSpec, Brush, Color, Column, ColumnSpec, DrawScope, Modifier, Point, Rect,
    Row, RowSpec, Size, Text, TextStyle, TileMode, VerticalAlignment,
};

use super::{journey::Journey, notes, tab::FRAMES_PER_SECOND};

const INK: Color = Color(0.93, 0.93, 0.88, 0.9);
const RED: Color = Color(0.9, 0.1, 0.08, 1.0);
const MARGIN: f32 = 26.0;
// How far the text stands in from the edges, inside the frame's corners.
const INSET: f32 = MARGIN + 14.0;
const CORNER: f32 = 34.0;
const LINE: f32 = 2.0;
const HINT_SECONDS: u32 = 12;
const CAPTION_SECONDS: u32 = 5;

fn camcorder(size: f32) -> TextStyle {
    let mut style = TextStyle::default();
    style.span_style.font_size = TextUnit::Sp(size);
    style.span_style.color = Some(INK);
    style.span_style.font_family = Some(FontFamily::Monospace);
    style
}

#[composable]
pub(super) fn Hud(journey: Journey) {
    let frames = journey.frames;
    let whole = derivedStateOf(move || frames.get() / FRAMES_PER_SECOND);
    let blink = derivedStateOf(move || (frames.get() / (FRAMES_PER_SECOND / 2)).is_multiple_of(2));
    let hint = derivedStateOf(move || frames.get() < HINT_SECONDS * FRAMES_PER_SECOND);
    let heard = derivedStateOf(move || {
        let since = frames.get().saturating_sub(journey.entered.get());
        journey.rooms.get() > 1 && since < CAPTION_SECONDS * FRAMES_PER_SECOND
    });
    Box(
        Modifier::empty().fill_max_size().draw_behind(lens),
        BoxSpec::default(),
        move || {
            let on = blink.get();
            Row(
                Modifier::empty().align(Alignment::TOP_START).padding(INSET),
                RowSpec::new().vertical_alignment(VerticalAlignment::CenterVertically),
                move || {
                    Box(
                        Modifier::empty()
                            .size(Size::new(14.0, 14.0))
                            .rounded_corners(7.0)
                            .background(if on { RED } else { Color::TRANSPARENT }),
                        BoxSpec::default(),
                        || {},
                    );
                    Text("REC", Modifier::empty().padding(10.0), camcorder(22.0));
                },
            );
            Text(
                format!("ROOM {}", journey.rooms.get()),
                Modifier::empty().align(Alignment::TOP_END).padding(INSET),
                camcorder(22.0),
            );
            let elapsed = whole.get();
            Text(
                format!(
                    "{}:{:02}:{:02}",
                    elapsed / 3600,
                    elapsed / 60 % 60,
                    elapsed % 60
                ),
                Modifier::empty()
                    .align(Alignment::BOTTOM_START)
                    .padding(INSET),
                camcorder(22.0),
            );
            Column(
                Modifier::empty()
                    .align(Alignment::BOTTOM_END)
                    .padding(INSET),
                ColumnSpec::default(),
                move || {
                    Text("JAN. 1 1970", Modifier::empty(), camcorder(18.0));
                    Text(
                        if on { "12:00 AM" } else { "  :   AM" },
                        Modifier::empty(),
                        camcorder(18.0),
                    );
                },
            );
            if heard.get() {
                Text(
                    notes::caption(journey.rooms.get()),
                    Modifier::empty()
                        .align(Alignment::BOTTOM_CENTER)
                        .padding(MARGIN + 60.0),
                    camcorder(17.0),
                );
            } else if hint.get() {
                Text(
                    "W A S D or a finger on the left half walks. A drag looks around.",
                    Modifier::empty()
                        .align(Alignment::BOTTOM_CENTER)
                        .padding(MARGIN + 60.0),
                    camcorder(15.0),
                );
            }
        },
    );
}

// The dark rim of the lens and the corners of the frame.
fn lens(scope: &mut dyn DrawScope) {
    let size = scope.size();
    let middle = Point::new(size.width * 0.5, size.height * 0.5);
    let reach = (size.width * size.width + size.height * size.height).sqrt() * 0.5;
    scope.draw_rect(Brush::radial_gradient_stops(
        vec![
            (0.0, Color(0.0, 0.0, 0.0, 0.0)),
            (0.55, Color(0.0, 0.0, 0.0, 0.0)),
            (0.85, Color(0.0, 0.0, 0.0, 0.35)),
            (1.0, Color(0.0, 0.0, 0.0, 0.8)),
        ],
        middle,
        reach,
        TileMode::Clamp,
    ));
    let (left, top) = (MARGIN, MARGIN);
    let (right, bottom) = (size.width - MARGIN, size.height - MARGIN);
    for (x, y, toward_x, toward_y) in [
        (left, top, 1.0, 1.0),
        (right, top, -1.0, 1.0),
        (left, bottom, 1.0, -1.0),
        (right, bottom, -1.0, -1.0),
    ] {
        let across = Rect {
            x: if toward_x > 0.0 { x } else { x - CORNER },
            y: if toward_y > 0.0 { y } else { y - LINE },
            width: CORNER,
            height: LINE,
        };
        let down = Rect {
            x: if toward_x > 0.0 { x } else { x - LINE },
            y: if toward_y > 0.0 { y } else { y - CORNER },
            width: LINE,
            height: CORNER,
        };
        scope.draw_rect_at(across, Brush::solid(INK));
        scope.draw_rect_at(down, Brush::solid(INK));
    }
}
