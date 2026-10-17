use cranpose::spatial::{rememberRoomState, Room, RoomState, Wall};
use cranpose_core::rememberMutableStateOf;
use cranpose_ui::{
    composable, text::TextUnit, Box, BoxSpec, Brush, Button, ButtonSpec, Color, Column, ColumnSpec,
    Modifier, Rect, Row, RowSpec, Text, TextStyle,
};

use super::{mineswapper2::mineswapper2_tab, text_showcase::TextShowcaseTab};

const TILE: f32 = 160.0;

fn text_style(size: f32, color: Color) -> TextStyle {
    let mut style = TextStyle::default();
    style.span_style.font_size = TextUnit::Sp(size);
    style.span_style.color = Some(color);
    style
}

#[composable]
pub fn RoomTab() {
    let room = rememberRoomState();
    let controls = room.clone();
    Room(
        room,
        Modifier::empty()
            .fill_max_size()
            .background(Color(0.04, 0.04, 0.06, 1.0)),
        move |wall| match wall {
            Wall::North => Controls(controls.clone()),
            Wall::East => Panel(Color(0.10, 0.11, 0.15, 1.0), mineswapper2_tab),
            Wall::West => Panel(Color(0.12, 0.10, 0.12, 1.0), TextShowcaseTab),
            Wall::South => BackWall(),
            Wall::Floor => Floor(),
            Wall::Ceiling => Ceiling(),
        },
    );
}

#[composable(no_skip)]
fn Controls(room: RoomState) {
    let presses = rememberMutableStateOf(|| 0u32);
    let entered = room.is_entered();
    Column(
        Modifier::empty()
            .fill_max_size()
            .background(Color(0.09, 0.10, 0.14, 1.0))
            .padding(40.0),
        ColumnSpec::default(),
        move || {
            Text(
                "A room of screens",
                Modifier::empty().padding(6.0),
                text_style(30.0, Color(0.95, 0.95, 1.0, 1.0)),
            );
            Text(
                "This is an ordinary screen. Step into the room and it becomes the far wall; \
                 the other demo tabs hang on the walls around you and stay live.",
                Modifier::empty().padding(6.0),
                text_style(16.0, Color(0.75, 0.77, 0.86, 1.0)),
            );
            Text(
                "W A S D or the up and down arrows walk. Q and E, the left and right arrows \
                 or a drag turn. Escape takes the keys back from a focused control.",
                Modifier::empty().padding(6.0),
                text_style(14.0, Color(0.60, 0.63, 0.74, 1.0)),
            );
            let room = room.clone();
            Row(
                Modifier::empty().padding(6.0),
                RowSpec::default(),
                move || {
                    let room = room.clone();
                    Button(
                        Modifier::empty()
                            .rounded_corners(10.0)
                            .background(Color(0.25, 0.45, 0.95, 1.0))
                            .padding(14.0),
                        ButtonSpec::default(),
                        move || {
                            if room.is_entered() {
                                room.leave();
                            } else {
                                room.enter();
                            }
                        },
                        move || {
                            Text(
                                if entered {
                                    "Back to the screen"
                                } else {
                                    "Step into the room"
                                },
                                Modifier::empty(),
                                text_style(16.0, Color::WHITE),
                            );
                        },
                    );
                    Button(
                        Modifier::empty()
                            .padding(6.0)
                            .rounded_corners(10.0)
                            .background(Color(0.20, 0.22, 0.30, 1.0))
                            .padding(14.0),
                        ButtonSpec::default(),
                        move || presses.set(presses.get() + 1),
                        move || {
                            Text(
                                format!("Pressed {} times", presses.get()),
                                Modifier::empty(),
                                text_style(16.0, Color::WHITE),
                            );
                        },
                    );
                },
            );
        },
    );
}

#[composable]
fn Panel(color: Color, content: fn()) {
    Box(
        Modifier::empty()
            .fill_max_size()
            .background(color)
            .padding(32.0),
        BoxSpec::default(),
        content,
    );
}

#[composable]
fn BackWall() {
    Column(
        Modifier::empty()
            .fill_max_size()
            .background(Color(0.13, 0.12, 0.10, 1.0))
            .padding(48.0),
        ColumnSpec::default(),
        || {
            Text(
                "Behind you",
                Modifier::empty().padding(6.0),
                text_style(30.0, Color(0.96, 0.92, 0.84, 1.0)),
            );
            Text(
                "The screen you came from is the far wall. Turn around with Q or E.",
                Modifier::empty().padding(6.0),
                text_style(16.0, Color(0.78, 0.74, 0.66, 1.0)),
            );
        },
    );
}

#[composable]
fn Floor() {
    Box(
        Modifier::empty().fill_max_size().draw_behind(|scope| {
            let size = scope.size();
            scope.draw_rect(Brush::solid(Color(0.16, 0.13, 0.11, 1.0)));
            let columns = (size.width / TILE).ceil() as u32;
            let rows = (size.height / TILE).ceil() as u32;
            for row in 0..rows {
                for column in (row % 2..columns).step_by(2) {
                    scope.draw_rect_at(
                        Rect {
                            x: column as f32 * TILE,
                            y: row as f32 * TILE,
                            width: TILE,
                            height: TILE,
                        },
                        Brush::solid(Color(0.22, 0.18, 0.15, 1.0)),
                    );
                }
            }
        }),
        BoxSpec::default(),
        || {},
    );
}

#[composable]
fn Ceiling() {
    Box(
        Modifier::empty().fill_max_size().draw_behind(|scope| {
            let size = scope.size();
            scope.draw_rect(Brush::solid(Color(0.20, 0.20, 0.22, 1.0)));
            let lamp = Rect {
                x: size.width * 0.5 - 120.0,
                y: 0.0,
                width: 240.0,
                height: 80.0,
            };
            for share in [0.25, 0.55, 0.85] {
                scope.draw_rect_at(
                    Rect {
                        y: size.height * share - lamp.height * 0.5,
                        ..lamp
                    },
                    Brush::solid(Color(0.95, 0.93, 0.85, 1.0)),
                );
            }
        }),
        BoxSpec::default(),
        || {},
    );
}
