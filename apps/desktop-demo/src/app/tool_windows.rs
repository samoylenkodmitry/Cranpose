//! Tool windows that stack and snap, the way a classic media player's do.
//!
//! Three panes of different heights. Drag a pane's title to tear it out of
//! its window into a window of its own that follows the pointer, and carry
//! it to just above or just below another window to snap it on: the two
//! become one window, sized for both. Drag a pane's body to move the window
//! that holds it, panes and all. Every pair and every triple is reachable.
//! Each pane is `movable` content, so the window that shows it composes the
//! same subtree wherever it goes; the grip sits inside that content and names
//! only the pane.
#![allow(non_snake_case)]

use cranpose::WindowModifierExt;
use cranpose_core::{key, movable};
use cranpose_ui::{composable, Box, BoxSpec, Color, Column, ColumnSpec, Modifier, Size, Text};

use super::{
    chrome_tabs::{label_style, CHROME, INK},
    torn_windows::{Axis, Rules, TornWindowsHost, WindowView, Windows},
};

/// One tool pane: its identity, its label, its size and its tint.
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

/// Three snapping tool windows.
#[composable]
pub fn tool_windows_app() {
    let rules = Rules::stack(
        "Cranpose Tool Windows",
        tools()[0].size,
        Axis::Vertical,
        SNAP_REACH,
    );
    let panes = tools().iter().map(|tool| (tool.key, tool.size)).collect();
    TornWindowsHost("tool-windows", rules, panes, |view| ToolStack(view.clone()));
}

#[composable]
fn ToolStack(view: WindowView) {
    let panes = view.panes().to_vec();
    let windows = view.windows().clone();
    Column(
        Modifier::empty().background(CHROME),
        ColumnSpec::default(),
        move || {
            for pane in &panes {
                let pane = *pane;
                let windows = windows.clone();
                key(pane, move || {
                    if let Some(tool) = tools().into_iter().find(|tool| tool.key == pane) {
                        movable(("tool", pane), move || ToolPane(windows, tool));
                    }
                });
            }
        },
    );
}

#[composable]
fn ToolPane(windows: Windows, tool: Tool) {
    Column(
        Modifier::empty().size(tool.size).background(tool.tint),
        ColumnSpec::default(),
        move || {
            Text(
                tool.title,
                windows
                    .grip(Modifier::empty(), tool.key)
                    .fill_max_width()
                    .height(TITLE_HEIGHT)
                    .background(CHROME)
                    .padding(3.0),
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
