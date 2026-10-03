use cranpose_ui::{
    text::TextAlign, widgets::BoxWithConstraints, BoxWithConstraintsScope, ScrollState,
};

use super::*;

#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::app) struct Table {
    pub alignments: Vec<pulldown_cmark::Alignment>,
    pub rows: Vec<Vec<Rc<AnnotatedString>>>,
}

#[composable]
pub(super) fn MarkdownTable(table: Rc<Table>, appearance: MarkdownAppearance) {
    let table = cranpose_core::rememberUpdatedState(table);
    let scroll = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
    BoxWithConstraints(Modifier::empty().fill_max_width(), move |scope| {
        let columns = table.with(|table| table.alignments.len()).max(1);
        let column_width = (scope.constraints().max_width / columns as f32).max(150.0);
        Column(
            Modifier::empty()
                .fill_max_width()
                .clip_to_bounds()
                .content_description("Table")
                .horizontal_scroll(scroll, false),
            ColumnSpec::default(),
            move || {
                let rows = table.with(|table| table.rows.len());
                for row in 0..rows {
                    Row(
                        Modifier::empty()
                            .width(column_width * columns as f32)
                            .background(if row == 0 {
                                Color(0.12, 0.21, 0.26, 1.0)
                            } else {
                                Color(0.06, 0.10, 0.14, 1.0)
                            }),
                        RowSpec::default(),
                        move || {
                            for column in 0..columns {
                                Box(
                                    Modifier::empty().width(column_width).padding(10.0),
                                    BoxSpec::default(),
                                    move || {
                                        table.with(|table| {
                                            let Some(cell) = table.rows[row].get(column) else {
                                                return;
                                            };
                                            let alignment = match table.alignments[column] {
                                                pulldown_cmark::Alignment::Center => {
                                                    TextAlign::Center
                                                }
                                                pulldown_cmark::Alignment::Right => TextAlign::End,
                                                _ => TextAlign::Start,
                                            };
                                            render_text_block_aligned(
                                                cell.clone(),
                                                appearance,
                                                alignment,
                                            );
                                        });
                                    },
                                );
                            }
                        },
                    );
                    Spacer(
                        Modifier::empty()
                            .width(column_width * columns as f32)
                            .height(1.0)
                            .background(Color(0.21, 0.33, 0.39, 1.0)),
                    );
                }
            },
        );
    });
}
