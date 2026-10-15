use cranpose_ui::{
    Box, BoxSpec, LayoutDirection, Modifier, ProvideLayoutDirection, Size,
    widgets::{FlowColumn, FlowSpec},
};

use crate::{directional_layout::tagged, text_measured_layout_integration::layout_composition};

/// Five 30 by 20 boxes in a 100 high column, 10 apart down a column and 5
/// apart between columns: three fit the first column, two the second.
fn column_of_boxes(direction: LayoutDirection) -> Vec<(f32, f32)> {
    let mut composition = cranpose_ui::run_test_composition(move || {
        ProvideLayoutDirection(direction, || {
            FlowColumn(
                Modifier::empty().size_points(200.0, 100.0),
                FlowSpec::new()
                    .main_axis_spacing(10.0)
                    .cross_axis_spacing(5.0),
                || {
                    for index in 0..5 {
                        Box(
                            Modifier::empty()
                                .size_points(30.0, 20.0)
                                .content_description(format!("item {index}")),
                            BoxSpec::default(),
                            || {},
                        );
                    }
                },
            );
        });
    });
    let layout = layout_composition(&mut composition, Size::new(200.0, 100.0));
    (0..5)
        .map(|index| {
            let rect = tagged(layout.root(), &format!("item {index}")).rect;
            (rect.x, rect.y)
        })
        .collect()
}

#[test]
fn a_flow_column_fills_a_column_downward_and_wraps_into_the_next() {
    assert_eq!(
        column_of_boxes(LayoutDirection::Ltr),
        [
            (0.0, 0.0),
            (0.0, 30.0),
            (0.0, 60.0),
            (35.0, 0.0),
            (35.0, 30.0)
        ],
        "a fourth box would end at 110, past the 100 high column"
    );
}

#[test]
fn a_right_to_left_flow_column_starts_its_first_column_at_the_right() {
    assert_eq!(
        column_of_boxes(LayoutDirection::Rtl),
        [
            (170.0, 0.0),
            (170.0, 30.0),
            (170.0, 60.0),
            (135.0, 0.0),
            (135.0, 30.0)
        ]
    );
}
