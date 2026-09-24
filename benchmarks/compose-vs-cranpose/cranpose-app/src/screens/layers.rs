use cranpose::prelude::*;

use super::{rememberFrameSeconds, text_style};
use crate::data::PALETTE;

/// `columns × rows` tiles fill the screen; past 6 columns the gaps and type
/// shrink with the tiles. `opaque` keeps every tile at full alpha
/// (`--ez opaque true`), an ablation that separates the cost of translucent
/// layers from transformed ones.
#[composable]
pub fn LayersScreen(opaque: bool, columns: usize, rows: usize) {
    let seconds = rememberFrameSeconds();
    let scale = (6.0 / columns as f32).min(1.0);
    Column(
        Modifier::empty().fill_max_width().weight(1.0).padding(4.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(8.0 * scale)),
        move || {
            for row in 0..rows {
                Row(
                    Modifier::empty().fill_max_width().weight(1.0),
                    RowSpec::new()
                        .horizontal_arrangement(LinearArrangement::spaced_by(8.0 * scale)),
                    move || {
                        for column in 0..columns {
                            Tile(row * columns + column, seconds, opaque, scale);
                        }
                    },
                );
            }
        },
    );
}

#[composable]
fn Tile(index: usize, seconds: MutableState<f32>, opaque: bool, scale: f32) {
    let phase = index as f32;
    Box(
        Modifier::empty()
            .weight(1.0)
            .fill_max_height()
            // Read in the layer block only: frames update layer properties,
            // nothing recomposes or redraws.
            .graphics_layer_block(move |layer| {
                let t = seconds.get();
                layer.rotation_z = (t * 90.0 + phase * 13.0) % 360.0;
                let scale = 0.85 + 0.15 * (t * 3.0 + phase * 0.4).sin();
                layer.scale_x = scale;
                layer.scale_y = scale;
                if !opaque {
                    layer.alpha = 0.65 + 0.35 * (0.5 + 0.5 * (t * 2.0 + phase * 0.7).sin());
                }
            })
            .background(PALETTE[index % PALETTE.len()])
            .rounded_corners(12.0 * scale),
        BoxSpec::new().content_alignment(Alignment::CENTER),
        move || {
            Text(
                index.to_string(),
                Modifier::empty(),
                text_style(16.0 * scale, Color::WHITE, true),
            );
        },
    );
}
