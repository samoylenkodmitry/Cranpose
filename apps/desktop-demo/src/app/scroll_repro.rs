use cranpose_foundation::{lazy::LazyListScope, SemanticsConfiguration};
use cranpose_ui::{
    composable,
    widgets::{LazyColumn, LazyColumnSpec},
    Color, Column, ColumnSpec, LinearArrangement, Modifier, Row, RowSpec, Size, Spacer, Text,
    TextStyle, VerticalAlignment,
};

#[composable]
pub fn scroll_repro_tab() {
    let list_state = cranpose_foundation::lazy::remember_lazy_list_state();

    Column(
        Modifier::empty()
            .fill_max_size()
            .padding(16.0)
            .background(Color(0.96, 0.96, 0.94, 1.0)),
        ColumnSpec::default(),
        move || {
            // Mimic HN Header roughly to occupy similar space
            Row(
                Modifier::empty()
                    .fill_max_width()
                    .background(Color(1.0, 0.4, 0.0, 1.0))
                    .padding(8.0)
                    .rounded_corners(4.0),
                RowSpec::new().vertical_alignment(VerticalAlignment::CenterVertically),
                || {
                    Text(
                        "Scroll Repro",
                        Modifier::empty().padding(4.0),
                        TextStyle::default(),
                    );
                },
            );

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            LazyColumn(
                Modifier::empty()
                    .semantics(|config: &mut SemanticsConfiguration| {
                        config.content_description = Some("LazyListViewport".to_string());
                    })
                    .fill_max_width()
                    .weight(1.0),
                list_state,
                LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                move |scope| {
                    scope.items(
                        100,
                        None::<fn(usize) -> u64>,
                        None::<fn(usize) -> u64>,
                        move |index| {
                            repro_item(index + 1);
                        },
                    );
                },
            );
        },
    );
}

#[composable]
fn repro_item(rank: usize) {
    // Matches the story-row layout used for scroll regression coverage.
    Row(
        Modifier::empty()
            .fill_max_width()
            .background(Color(1.0, 1.0, 1.0, 1.0))
            .padding(12.0)
            .rounded_corners(4.0),
        RowSpec::new().vertical_alignment(VerticalAlignment::Top),
        move || {
            // Rank
            Text(
                format!("{}.", rank),
                Modifier::empty().padding(4.0),
                TextStyle {
                    color: Some(Color(0.5, 0.5, 0.5, 1.0)),
                    ..Default::default()
                },
            );

            Spacer(Size {
                width: 8.0,
                height: 0.0,
            });

            Column(Modifier::empty().weight(1.0), ColumnSpec::default(), {
                move || {
                    // Title - Fake Title
                    Text(
                        format!("Fake Title for Item {}", rank),
                        Modifier::empty().padding(2.0),
                        TextStyle {
                            color: Some(Color(0.0, 0.0, 0.0, 0.87)),
                            ..Default::default()
                        },
                    );

                    // Metadata - Fake Metadata
                    Row(
                        Modifier::empty(),
                        RowSpec::new().vertical_alignment(VerticalAlignment::CenterVertically),
                        move || {
                            Text(
                                "100 points by user | 50 comments".to_string(),
                                Modifier::empty().padding(2.0),
                                TextStyle {
                                    color: Some(Color(0.5, 0.5, 0.5, 1.0)),
                                    ..Default::default()
                                },
                            );
                        },
                    );
                }
            });
        },
    );
}
