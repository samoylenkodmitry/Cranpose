//! Layout-bound screens: the container width follows the frame clock, so every
//! frame measures and places the whole tree again while nothing below the
//! screen recomposes. The `layered` variants give every node a static rotated
//! graphics layer.

use cranpose::prelude::*;

use super::{rememberFrameSeconds, text_style};
use crate::data::PALETTE;

const LEVEL_BACKGROUND: [Color; 2] = [
    Color::from_rgb_u8(0xE2, 0xE8, 0xF0),
    Color::from_rgb_u8(0xCB, 0xD5, 0xE1),
];

fn width_fraction(seconds: f32) -> f32 {
    0.7 + 0.3 * (0.5 + 0.5 * (seconds * 2.0).sin())
}

fn tilt(layered: bool, degrees: f32) -> Modifier {
    if layered {
        Modifier::empty().graphics_layer_value(GraphicsLayer {
            rotation_z: degrees,
            ..Default::default()
        })
    } else {
        Modifier::empty()
    }
}

/// A flat grid of `rows × columns` cells whose labels wrap as the width moves.
#[composable]
pub fn GridScreen(layered: bool, rows: usize, columns: usize) {
    let seconds = rememberFrameSeconds();
    Column(
        Modifier::empty()
            .fill_max_width_fraction(width_fraction(seconds.get()))
            .weight(1.0),
        ColumnSpec::default(),
        move || {
            for row in 0..rows {
                GridRow(row, columns, layered);
            }
        },
    );
}

#[composable]
fn GridRow(row: usize, columns: usize, layered: bool) {
    Row(
        Modifier::empty().fill_max_width().weight(1.0),
        RowSpec::default(),
        move || {
            for column in 0..columns {
                GridCell(row * columns + column, layered);
            }
        },
    );
}

#[composable]
fn GridCell(index: usize, layered: bool) {
    Box(
        Modifier::empty()
            .weight(1.0)
            .fill_max_height()
            .padding(1.0)
            .then(tilt(layered, ((index % 7) as f32 - 3.0) * 2.0))
            .background(PALETTE[index % PALETTE.len()])
            .rounded_corners(3.0),
        BoxSpec::new().content_alignment(Alignment::CENTER),
        move || {
            Text(
                format!("cell {index}"),
                Modifier::empty(),
                text_style(9.0, Color::WHITE, false),
            );
        },
    );
}

/// `depth` nested levels, each a row of `chips` labels above the next level:
/// a thread of replies nested inside one another.
#[composable]
pub fn DeepScreen(layered: bool, depth: usize, chips: usize) {
    let seconds = rememberFrameSeconds();
    Box(
        Modifier::empty()
            .fill_max_width_fraction(width_fraction(seconds.get()))
            .weight(1.0),
        BoxSpec::default(),
        move || Level(depth, chips, layered),
    );
}

#[composable]
fn Level(remaining: usize, chips: usize, layered: bool) {
    Column(
        Modifier::empty()
            .fill_max_width()
            .then(tilt(
                layered,
                if remaining.is_multiple_of(2) {
                    0.6
                } else {
                    -0.6
                },
            ))
            .background(LEVEL_BACKGROUND[remaining % 2])
            .rounded_corners(4.0)
            .padding_each(3.0, 1.0, 1.0, 1.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(1.0)),
        move || {
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(2.0)),
                move || {
                    for chip in 0..chips {
                        Text(
                            format!("L{remaining}.{chip}"),
                            Modifier::empty()
                                .weight(1.0)
                                .background(PALETTE[(remaining + chip) % PALETTE.len()])
                                .rounded_corners(2.0),
                            text_style(8.0, Color::WHITE, false),
                        );
                    }
                },
            );
            if remaining > 0 {
                Level(remaining - 1, chips, layered);
            }
        },
    );
}
