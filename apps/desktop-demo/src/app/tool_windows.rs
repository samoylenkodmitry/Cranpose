//! Tool windows that stack and snap, the way a classic media player's do.
//!
//! Three panes of different heights. Drag a pane's title to tear it out of
//! its window into a window of its own that follows the pointer, and carry
//! it to just above or just below another window to snap it on: the two
//! become one window, sized for both. Drag a pane's body to move the window
//! that holds it, panes and all. Every pair and every triple is reachable.
#![allow(non_snake_case)]

use cranpose::{
    Dock, DockAxis, DockHost, DockKey, DockModifierExt, DockPolicy, SizedPane, WindowModifierExt,
};
use cranpose_core::key;
use cranpose_ui::{composable, Box, BoxSpec, Color, Column, ColumnSpec, Modifier, Size, Text};

use super::chrome_tabs::{label_style, CHROME, INK};

/// One tool pane: its identity, its label, its size and its tint.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Tool {
    key: DockKey,
    title: &'static str,
    size: Size,
    tint: Color,
}

fn tools() -> [Tool; 3] {
    [
        Tool {
            key: DockKey::from_static("player"),
            title: "Player",
            size: Size {
                width: PANE_WIDTH,
                height: 116.0,
            },
            tint: Color(0.36, 0.42, 0.52, 1.0),
        },
        Tool {
            key: DockKey::from_static("equalizer"),
            title: "Equalizer",
            size: Size {
                width: PANE_WIDTH,
                height: 116.0,
            },
            tint: Color(0.38, 0.50, 0.44, 1.0),
        },
        Tool {
            key: DockKey::from_static("playlist"),
            title: "Playlist",
            size: Size {
                width: PANE_WIDTH,
                height: 232.0,
            },
            tint: Color(0.56, 0.50, 0.38, 1.0),
        },
    ]
}

/// Three snapping tool windows.
#[composable]
pub fn tool_windows_app() {
    Dock(
        "tool-windows",
        DockPolicy::stack(
            "Cranpose Tool Windows",
            tools()[0].size,
            DockAxis::Vertical,
            SNAP_REACH,
        ),
        |host| ToolStack(host.clone()),
        || {
            for tool in tools() {
                SizedPane(tool.key, tool.size, move || ToolPane(tool));
            }
        },
    );
}

#[composable]
fn ToolStack(host: DockHost) {
    let panes = host.panes().to_vec();
    Column(
        Modifier::empty().background(CHROME),
        ColumnSpec::default(),
        move || {
            for pane in &panes {
                let pane = *pane;
                let host = host.clone();
                key(pane.raw(), move || host.content(pane));
            }
        },
    );
}

#[composable]
fn ToolPane(tool: Tool) {
    Column(
        Modifier::empty().size(tool.size).background(tool.tint),
        ColumnSpec::default(),
        move || {
            Text(
                tool.title,
                Modifier::empty()
                    .fill_max_width()
                    .height(TITLE_HEIGHT)
                    .background(CHROME)
                    .padding(3.0)
                    .dock_handle(tool.key),
                label_style(11.0, INK),
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
                        "drag the title to tear or snap this pane; drag here to move the window",
                        Modifier::empty(),
                        label_style(12.0, INK),
                    );
                },
            );
        },
    );
}

const PANE_WIDTH: f32 = 275.0;
const TITLE_HEIGHT: f32 = 20.0;
const SNAP_REACH: f32 = 12.0;
