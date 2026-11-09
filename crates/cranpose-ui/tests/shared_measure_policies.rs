// Rows with equal arrangements share one measure policy, and more distinct
// arrangements than the shared policies hold take policies of their own:
// each row still spaces its children by its own arrangement.

use cranpose_ui::{
    Box, BoxSpec, Column, ColumnSpec, LinearArrangement, Modifier, Row, RowSpec, Size,
};

use crate::{directional_layout::tagged, text_measured_layout_integration::layout_composition};

const ROWS: usize = 24;
const SPACINGS: usize = 12;

fn spacing(row: usize) -> f32 {
    (row % SPACINGS) as f32
}

#[test]
fn rows_space_their_children_by_their_own_arrangements() {
    let mut composition = cranpose_ui::run_test_composition(|| {
        Column(Modifier::empty(), ColumnSpec::default(), || {
            for row in 0..ROWS {
                Row(
                    Modifier::empty(),
                    RowSpec::new()
                        .horizontal_arrangement(LinearArrangement::spaced_by(spacing(row))),
                    move || {
                        Box(
                            Modifier::empty().size_points(10.0, 4.0),
                            BoxSpec::default(),
                            || {},
                        );
                        Box(
                            Modifier::empty()
                                .size_points(10.0, 4.0)
                                .content_description(format!("second of {row}")),
                            BoxSpec::default(),
                            || {},
                        );
                    },
                );
            }
        });
    });
    let layout = layout_composition(&mut composition, Size::new(400.0, 400.0));
    for row in 0..ROWS {
        let second = tagged(layout.root(), &format!("second of {row}"));
        assert_eq!(second.rect.x, 10.0 + spacing(row), "row {row}");
        assert_eq!(second.rect.y, 4.0 * row as f32, "row {row}");
    }
}
