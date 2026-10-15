use cranpose_ui::{
    Alignment, Box, BoxSpec, Column, ColumnSpec, Modifier, Row, RowSpec, Size, VerticalAlignment,
};

use crate::{directional_layout::tagged, text_measured_layout_integration::layout_composition};

fn lay_out(content: impl Fn() + 'static) -> cranpose_ui::LayoutTree {
    let mut composition = cranpose_ui::run_test_composition(content);
    layout_composition(&mut composition, Size::new(200.0, 200.0))
}

#[test]
fn the_outermost_weight_of_a_row_child_wins() {
    let layout = lay_out(|| {
        Row(Modifier::empty().width(200.0), RowSpec::default(), || {
            Box(
                Modifier::empty()
                    .weight(1.0)
                    .weight(3.0)
                    .height(10.0)
                    .content_description("first"),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty().weight(1.0).height(10.0),
                BoxSpec::default(),
                || {},
            );
        });
    });
    assert_eq!(tagged(layout.root(), "first").rect.width, 100.0);
}

#[test]
fn the_outermost_alignment_of_a_child_wins() {
    let layout = lay_out(|| {
        Column(Modifier::empty(), ColumnSpec::default(), || {
            Box(
                Modifier::empty().size_points(100.0, 100.0),
                BoxSpec::default(),
                || {
                    Box(
                        Modifier::empty()
                            .align(Alignment::BOTTOM_END)
                            .align(Alignment::TOP_START)
                            .size_points(10.0, 10.0)
                            .content_description("in box"),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
            Row(Modifier::empty().height(50.0), RowSpec::default(), || {
                Box(
                    Modifier::empty()
                        .alignInRow(VerticalAlignment::Bottom)
                        .alignInRow(VerticalAlignment::Top)
                        .size_points(10.0, 10.0)
                        .content_description("in row"),
                    BoxSpec::default(),
                    || {},
                );
            });
        });
    });
    let in_box = tagged(layout.root(), "in box").rect;
    assert_eq!((in_box.x, in_box.y), (90.0, 90.0));
    assert_eq!(
        tagged(layout.root(), "in row").rect.y,
        140.0,
        "the row starts at 100 and is 50 high"
    );
}
