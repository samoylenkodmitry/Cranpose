#![allow(non_snake_case)]

use cranpose::{
    rememberWindowStateAt, LocalWindowState, WindowConfig, WindowModifierExt, WindowState,
};
use cranpose_core::{key, movable, rememberMutableStateOf, MutableState};
use cranpose_ui::{
    composable, Box, BoxSpec, Color, Column, ColumnSpec, Modifier, Point, PointerEvent,
    PointerEventKind, PointerInputScope, Row, RowSpec, Size, Text, VerticalAlignment,
};

use super::{
    chrome_tabs::{label_style, CHROME, INK},
    demo_trace::trace,
    window_snap::{rememberSnapSet, SnapSet},
};

#[derive(Clone, Copy, PartialEq, Debug)]
struct Tool {
    key: u64,
    title: &'static str,
    size: Size,
    tint: Color,
}

fn tools() -> [Tool; 3] {
    [
        Tool {
            key: 1,
            title: "Player",
            size: Size {
                width: PANE_WIDTH,
                height: 116.0,
            },
            tint: Color(0.36, 0.42, 0.52, 1.0),
        },
        Tool {
            key: 2,
            title: "Equalizer",
            size: Size {
                width: PANE_WIDTH,
                height: 116.0,
            },
            tint: Color(0.38, 0.50, 0.44, 1.0),
        },
        Tool {
            key: 3,
            title: "Playlist",
            size: Size {
                width: PANE_WIDTH,
                height: 232.0,
            },
            tint: Color(0.56, 0.50, 0.38, 1.0),
        },
    ]
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct ToolPlaces {
    torn: Vec<(u64, Point)>,
}

impl ToolPlaces {
    pub fn tear(&mut self, tool: u64, origin: Point) {
        self.dock(tool);
        self.torn.push((tool, origin));
    }

    pub fn dock(&mut self, tool: u64) {
        self.torn.retain(|(held, _)| *held != tool);
    }

    pub fn torn_origin(&self, tool: u64) -> Option<Point> {
        self.torn
            .iter()
            .find(|(held, _)| *held == tool)
            .map(|(_, origin)| *origin)
    }

    pub fn torn_tools(&self) -> Vec<u64> {
        self.torn.iter().map(|(tool, _)| *tool).collect()
    }
}

#[composable]
pub fn tool_windows_app() {
    let places = rememberMutableStateOf(ToolPlaces::default);
    let snapshot = places.get();
    trace!(
        "windows={}",
        snapshot
            .torn_tools()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",")
    );
    let snap = rememberSnapSet();
    let mut windows = Vec::new();
    for tool in tools() {
        let origin = snapshot
            .torn_origin(tool.key)
            .unwrap_or(Point::new(0.0, 0.0));
        windows.push((
            tool.key,
            rememberWindowStateAt(origin.x, origin.y, tool.size.width, tool.size.height),
        ));
    }
    let inline = snapshot.clone();
    Column(
        Modifier::empty().background(CHROME),
        ColumnSpec::default(),
        move || {
            for tool in tools() {
                if inline.torn_origin(tool.key).is_none() {
                    key(tool.key, move || {
                        movable(("tool", tool.key), move || ToolPane(tool, places));
                    });
                }
            }
        },
    );
    for (tool, (_, state)) in tools().into_iter().zip(windows.iter().copied()) {
        match snapshot.torn_origin(tool.key) {
            Some(origin) => {
                let windows = windows.clone();
                key(tool.key, move || {
                    TornTool(tool, origin, places, state, snap, windows.clone());
                });
            }
            None => snap.forget(tool.key),
        }
    }
}

#[composable]
fn TornTool(
    tool: Tool,
    origin: Point,
    places: MutableState<ToolPlaces>,
    state: WindowState,
    snap: SnapSet,
    windows: Vec<(u64, WindowState)>,
) {
    let first_frame = cranpose_core::remember(|| std::cell::Cell::new(false))
        .with(|placed| !placed.replace(true));
    if first_frame {
        state.set_position(Some(origin));
    }
    if let Some(at) = state.position() {
        let size = state.size();
        trace!(
            "window id={} origin=({:.1},{:.1}) size=({:.1},{:.1}) panes=1 parked=false",
            tool.key,
            at.x,
            at.y,
            size.width,
            size.height
        );
    }
    Box(
        Modifier::empty().window(
            WindowConfig::borderless_for_state(tool.title, state)
                .with_transparent(true)
                .on_moved(move |x, y| {
                    snap.window_moved(tool.key, Point::new(x, y), &windows);
                }),
        ),
        BoxSpec::default(),
        move || movable(("tool", tool.key), move || ToolPane(tool, places)),
    );
}

#[composable]
fn ToolPane(tool: Tool, places: MutableState<ToolPlaces>) {
    let torn = LocalWindowState::current().is_some_and(WindowState::presented);
    Column(
        Modifier::empty().size(tool.size).background(tool.tint),
        ColumnSpec::default(),
        move || {
            Row(
                tear_grip(
                    Modifier::empty()
                        .fill_max_width()
                        .height(TITLE_HEIGHT)
                        .background(CHROME)
                        .window_drag_area(),
                    tool,
                    places,
                    torn,
                ),
                RowSpec {
                    vertical_alignment: VerticalAlignment::CenterVertically,
                    ..RowSpec::default()
                },
                move || {
                    Text(
                        tool.title,
                        Modifier::empty().weight(1.0).padding(3.0),
                        label_style(11.0, INK),
                    );
                    if torn {
                        Text(
                            "dock",
                            Modifier::empty().padding(3.0).clickable(move |_| {
                                places.update(|held| held.dock(tool.key));
                                trace!("docked tool={}", tool.key);
                            }),
                            label_style(11.0, INK),
                        );
                    }
                },
            );
            Box(
                Modifier::empty()
                    .fill_max_width()
                    .height(tool.size.height - TITLE_HEIGHT)
                    .padding(12.0)
                    .window_drag_area(),
                BoxSpec::default(),
                || {
                    Text(
                        "drag the title to tear this pane into a window that follows the pointer; torn panes snap to each other",
                        Modifier::empty(),
                        label_style(12.0, INK),
                    );
                },
            );
        },
    );
}

fn tear_grip(
    modifier: Modifier,
    tool: Tool,
    places: MutableState<ToolPlaces>,
    torn: bool,
) -> Modifier {
    if torn {
        return modifier;
    }
    modifier.pointer_input(tool.key, move |scope: PointerInputScope| async move {
        scope
            .await_pointer_event_scope(|await_scope| async move {
                let mut press: Option<Point> = None;
                loop {
                    let event = await_scope.await_pointer_event().await;
                    match event.kind {
                        PointerEventKind::Down => press = Some(event.global_position),
                        PointerEventKind::Move => {
                            if let Some(origin) =
                                press.and_then(|pressed| tear_origin(pressed, &event))
                            {
                                press = None;
                                event.consume();
                                places.update(|held| held.tear(tool.key, origin));
                                trace!(
                                    "torn tool={} at=({:.1},{:.1})",
                                    tool.key,
                                    origin.x,
                                    origin.y
                                );
                            }
                        }
                        PointerEventKind::Up | PointerEventKind::Cancel => press = None,
                        _ => {}
                    }
                }
            })
            .await;
    })
}

fn tear_origin(pressed: Point, event: &PointerEvent) -> Option<Point> {
    let dx = event.global_position.x - pressed.x;
    let dy = event.global_position.y - pressed.y;
    if (dx * dx + dy * dy).sqrt() <= TEAR_DISTANCE {
        return None;
    }
    let screen = event.screen_position?;
    Some(Point::new(
        screen.x - event.position.x,
        screen.y - event.position.y,
    ))
}

const PANE_WIDTH: f32 = 275.0;
const TITLE_HEIGHT: f32 = 20.0;
const TEAR_DISTANCE: f32 = 12.0;

#[cfg(test)]
#[path = "tests/tool_windows_tests.rs"]
mod tests;
