use std::{cell::RefCell, rc::Rc};

use cranpose_ui::{
    composable, Axis, Color, Column, ColumnSpec, LinearArrangement, Modifier, Row, RowSpec, Text,
    TextStyle,
};

use super::cached_recursive_static_text;

#[composable]
pub(super) fn recursive_layout_node(modifier: Modifier, depth: usize, axis: Axis, index: usize) {
    let accent = [
        Color(0.25, 0.32, 0.58, 0.75),
        Color(0.30, 0.20, 0.45, 0.75),
        Color(0.20, 0.40, 0.32, 0.75),
        Color(0.45, 0.28, 0.24, 0.75),
    ][index % 4];

    Column(
        modifier
            .background(accent)
            .rounded_corners(18.0)
            .padding(12.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            Text(
                cached_depth_text(depth),
                Modifier::empty()
                    .background(Color(0.0, 0.0, 0.0, 0.25))
                    .rounded_corners(10.0)
                    .padding(6.0),
                TextStyle::default(),
            );

            if depth <= 1 {
                Text(
                    cached_recursive_static_text("Leaf node"),
                    Modifier::empty()
                        .background(Color(1.0, 1.0, 1.0, 0.12))
                        .rounded_corners(10.0)
                        .padding(6.0),
                    TextStyle::default(),
                );
            } else if axis.is_horizontal() {
                Row(
                    Modifier::empty().fill_max_width(),
                    RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
                    move || {
                        for child_idx in 0..2 {
                            recursive_layout_node(
                                Modifier::empty().rowWeight(1.0, true),
                                depth - 1,
                                Axis::Vertical,
                                index * 2 + child_idx + 1,
                            );
                        }
                    },
                );
            } else {
                Column(
                    Modifier::empty().fill_max_width(),
                    ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                    move || {
                        for child_idx in 0..2 {
                            recursive_layout_node(
                                Modifier::empty().columnWeight(1.0, true),
                                depth - 1,
                                Axis::Horizontal,
                                index * 2 + child_idx + 1,
                            );
                        }
                    },
                );
            }
        },
    );
}

fn cached_depth_text(depth: usize) -> Rc<cranpose_ui::text::AnnotatedString> {
    thread_local! {
        static TEXTS: RefCell<Vec<Rc<cranpose_ui::text::AnnotatedString>>> = const { RefCell::new(Vec::new()) };
    }

    TEXTS.with(|texts| {
        let mut texts = texts.borrow_mut();
        while texts.len() <= depth {
            let next_depth = texts.len();
            texts.push(Rc::new(cranpose_ui::text::AnnotatedString::from(format!(
                "Depth {next_depth}"
            ))));
        }
        Rc::clone(&texts[depth])
    })
}
