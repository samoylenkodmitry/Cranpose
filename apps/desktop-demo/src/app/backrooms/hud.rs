// What the camcorder shows over the backrooms: REC with a blinking dot, the
// room it is in, the time it has run, a clock nobody set, the corners of
// its frame and the dark rim of its lens. For the first seconds a line at
// the bottom tells how to walk; for a few seconds after each doorway, a
// caption tells what it heard. Each part reads only the state it shows, so
// a new second composes the timer alone and the blink composes nothing.

use std::time::Duration;

use cranpose_animation::{
    infiniteRepeatable, rememberInfiniteTransition, AnimationSpec, RepeatMode, StartOffset,
};
use cranpose_core::{delay, derivedStateOf, rememberMutableStateOf, LaunchedEffect, State};
use cranpose_ui::{
    composable,
    text::{FontFamily, TextUnit},
    Alignment, Box, BoxSpec, Brush, Color, Column, ColumnSpec, DrawScope, GraphicsLayer, Modifier,
    Point, Rect, Row, RowSpec, Size, Text, TextStyle, TileMode, VerticalAlignment,
};

use super::{journey::Journey, notes};

const INK: Color = Color(0.93, 0.93, 0.88, 0.9);
const RED: Color = Color(0.9, 0.1, 0.08, 1.0);
const MARGIN: f32 = 26.0;
// How far the text stands in from the edges, inside the frame's corners.
const INSET: f32 = MARGIN + 14.0;
const CORNER: f32 = 34.0;
const LINE: f32 = 2.0;
// A blink: shown for half of it, hidden for the other half.
const BLINK_MILLIS: u64 = 1000;
const SECOND: Duration = Duration::from_secs(1);
const HINT: Duration = Duration::from_secs(12);
const CAPTION: Duration = Duration::from_secs(5);

fn camcorder(size: f32) -> TextStyle {
    let mut style = TextStyle::default();
    style.span_style.font_size = TextUnit::Sp(size);
    style.span_style.color = Some(INK);
    style.span_style.font_family = Some(FontFamily::Monospace);
    style
}

#[composable]
pub(super) fn Hud(journey: Journey) {
    let phase = rememberInfiniteTransition("camcorder_blink").animateFloat(
        0.0,
        1.0,
        infiniteRepeatable(
            AnimationSpec::linear(BLINK_MILLIS),
            RepeatMode::Restart,
            StartOffset::default(),
        ),
        "camcorder_blink",
    );
    // The phase changes each frame; the blink, twice a second.
    let on = derivedStateOf(move || phase.value() < 0.5);
    Box(
        Modifier::empty().fill_max_size().draw_behind(lens),
        BoxSpec::default(),
        move || {
            Row(
                Modifier::empty().align(Alignment::TOP_START).padding(INSET),
                RowSpec::new().vertical_alignment(VerticalAlignment::CenterVertically),
                move || {
                    Box(
                        Modifier::empty()
                            .size(Size::new(14.0, 14.0))
                            .graphics_layer(move || blinking(on))
                            .rounded_corners(7.0)
                            .background(RED),
                        BoxSpec::default(),
                        || {},
                    );
                    Text("REC", Modifier::empty().padding(10.0), camcorder(22.0));
                },
            );
            RoomLabel(journey);
            Elapsed();
            Column(
                Modifier::empty()
                    .align(Alignment::BOTTOM_END)
                    .padding(INSET),
                ColumnSpec::default(),
                move || {
                    Text("JAN. 1 1970", Modifier::empty(), camcorder(18.0));
                    Row(Modifier::empty(), RowSpec::default(), move || {
                        Text(
                            "12:00",
                            Modifier::empty().graphics_layer(move || blinking(on)),
                            camcorder(18.0),
                        );
                        Text(" AM", Modifier::empty(), camcorder(18.0));
                    });
                },
            );
            Caption(journey);
        },
    );
}

// A layer shown while `on` holds. The layer reads `on` as it draws.
fn blinking(on: State<bool>) -> GraphicsLayer {
    GraphicsLayer {
        alpha: if on.get() { 1.0 } else { 0.0 },
        ..GraphicsLayer::DEFAULT
    }
}

// The room the camcorder is in, and the warning while the lights are out.
#[composable]
fn RoomLabel(journey: Journey) {
    Column(
        Modifier::empty().align(Alignment::TOP_END).padding(INSET),
        ColumnSpec::default(),
        move || {
            Text(
                format!("ROOM {}", journey.rooms()),
                Modifier::empty(),
                camcorder(22.0),
            );
            if journey.lights_out() {
                let mut warning = camcorder(15.0);
                warning.span_style.color = Some(RED);
                Text("EMERGENCY POWER", Modifier::empty(), warning);
            }
        },
    );
}

// The time the camcorder has run, a second at a time.
#[composable]
fn Elapsed() {
    let seconds = rememberMutableStateOf(|| 0_u32);
    LaunchedEffect((), async move |_| loop {
        delay(SECOND).await;
        seconds.update(|seconds| *seconds += 1);
    });
    let elapsed = seconds.get();
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
}

// What the camcorder heard after the last doorway, for a few seconds; and
// before any doorway, for the first seconds, how to walk.
#[composable]
fn Caption(journey: Journey) {
    let rooms = journey.rooms();
    let heard = rememberMutableStateOf(|| false);
    LaunchedEffect(rooms, async move |_| {
        if rooms > 1 {
            heard.set(true);
            delay(CAPTION).await;
            heard.set(false);
        }
    });
    let hint = rememberMutableStateOf(|| true);
    LaunchedEffect((), async move |_| {
        delay(HINT).await;
        hint.set(false);
    });
    let line = Modifier::empty()
        .align(Alignment::BOTTOM_CENTER)
        .padding(MARGIN + 60.0);
    if heard.get() {
        Text(notes::caption(rooms), line, camcorder(17.0));
    } else if hint.get() {
        Text(
            "W A S D or a finger on the left half walks. A drag looks around.",
            line,
            camcorder(15.0),
        );
    }
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
