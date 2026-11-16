use cranpose_ui::{
    composable, BoxSpec, Brush, Button, ButtonSpec, Color, Column, ColumnSpec, CornerRadii,
    LinearArrangement, Modifier, Row, RowSpec, Size, Spacer, Text, TextStyle, VerticalAlignment,
};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum ShowcaseType {
    SimpleCard,
    PositionedBoxes,
    ItemList,
    ComplexChain,
    DynamicModifiers,
    LongList,
}

impl ShowcaseType {
    fn label(self) -> &'static str {
        match self {
            Self::SimpleCard => "Simple Card",
            Self::PositionedBoxes => "Positioned Boxes",
            Self::ItemList => "Item List (5)",
            Self::ComplexChain => "Complex Chain",
            Self::DynamicModifiers => "Dynamic Modifiers",
            Self::LongList => "Long List (50)",
        }
    }
}

#[composable]
pub(super) fn modifier_showcase_tab() {
    let selected_showcase = cranpose_core::rememberMutableStateOf(|| ShowcaseType::SimpleCard);

    Row(
        Modifier::empty().fill_max_width().padding(8.0),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
            .vertical_alignment(VerticalAlignment::Top),
        move || {
            Column(
                Modifier::empty()
                    .width(180.0)
                    .background(Color(0.08, 0.10, 0.18, 1.0))
                    .rounded_corners(20.0)
                    .padding(16.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                move || {
                    Text(
                        "Select Showcase",
                        Modifier::empty()
                            .background(Color(1.0, 1.0, 1.0, 0.08))
                            .rounded_corners(12.0)
                            .padding(8.0),
                        TextStyle::default(),
                    );

                    Spacer(Modifier::empty().size_points(0.0, 8.0));

                    let showcase_types = [
                        ShowcaseType::SimpleCard,
                        ShowcaseType::PositionedBoxes,
                        ShowcaseType::ItemList,
                        ShowcaseType::ComplexChain,
                        ShowcaseType::DynamicModifiers,
                        ShowcaseType::LongList,
                    ];

                    for showcase_type in showcase_types {
                        let is_selected = selected_showcase.get() == showcase_type;
                        Button(
                            Modifier::empty()
                                .fill_max_width()
                                .rounded_corners(10.0)
                                .draw_behind(move |scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(if is_selected {
                                            Color(0.25, 0.45, 0.85, 1.0)
                                        } else {
                                            Color(0.15, 0.18, 0.25, 0.8)
                                        }),
                                        CornerRadii::uniform(10.0),
                                    );
                                })
                                .padding(10.0),
                            ButtonSpec::default(),
                            {
                                move || {
                                    if selected_showcase.get() != showcase_type {
                                        selected_showcase.set(showcase_type);
                                    }
                                }
                            },
                            {
                                let label = showcase_type.label();
                                move || {
                                    Text(
                                        label,
                                        Modifier::empty().padding(4.0),
                                        TextStyle::default(),
                                    );
                                }
                            },
                        );
                    }
                },
            );

            Column(
                Modifier::empty()
                    .fill_max_width()
                    .padding(24.0)
                    .background(Color(0.06, 0.08, 0.16, 0.9))
                    .rounded_corners(20.0)
                    .padding(16.0),
                ColumnSpec::default(),
                move || {
                    let showcase_to_render = selected_showcase.get();
                    cranpose_core::with_key(&showcase_to_render, || match showcase_to_render {
                        ShowcaseType::SimpleCard => simple_card_showcase(),
                        ShowcaseType::PositionedBoxes => positioned_boxes_showcase(),
                        ShowcaseType::ItemList => item_list_showcase(),
                        ShowcaseType::ComplexChain => complex_chain_showcase(),
                        ShowcaseType::DynamicModifiers => dynamic_modifiers_showcase(),
                        ShowcaseType::LongList => long_list_showcase(),
                    });
                },
            );
        },
    );
}

#[composable]
pub fn simple_card_showcase() {
    Column(Modifier::empty(), ColumnSpec::default(), || {
        Text(
            "=== Simple Card Pattern ===",
            Modifier::empty()
                .background(Color(1.0, 1.0, 1.0, 0.1))
                .rounded_corners(14.0)
                .padding(12.0),
            TextStyle::default(),
        );

        Spacer(Modifier::empty().size_points(0.0, 16.0));

        cranpose_ui::Box(
            Modifier::empty()
                .background(Color(0.4, 0.6, 0.9, 0.8))
                .rounded_corners(18.0)
                .padding(3.0),
            BoxSpec::default(),
            || {
                cranpose_ui::Box(
                    Modifier::empty()
                        .background(Color(0.15, 0.18, 0.25, 0.95))
                        .rounded_corners(16.0)
                        .padding(16.0)
                        .size_points(300.0, 200.0),
                    BoxSpec::default(),
                    || {
                        Column(
                            Modifier::empty().padding(8.0),
                            ColumnSpec::default(),
                            || {
                                Text(
                                    "Card Title",
                                    Modifier::empty()
                                        .background(Color(0.3, 0.5, 0.8, 0.6))
                                        .rounded_corners(8.0)
                                        .padding(8.0),
                                    TextStyle::default(),
                                );

                                Spacer(Modifier::empty().size_points(0.0, 8.0));

                                Text(
                                    "Card content goes here with padding",
                                    Modifier::empty().padding(4.0),
                                    TextStyle::default(),
                                );

                                Spacer(Modifier::empty().size_points(0.0, 12.0));

                                Row(Modifier::empty(), RowSpec::default(), || {
                                    Text(
                                        "Action 1",
                                        Modifier::empty()
                                            .background(Color(0.2, 0.7, 0.4, 0.7))
                                            .rounded_corners(6.0)
                                            .padding(8.0),
                                        TextStyle::default(),
                                    );

                                    Spacer(Modifier::empty().size_points(8.0, 0.0));

                                    Text(
                                        "Action 2",
                                        Modifier::empty()
                                            .background(Color(0.8, 0.3, 0.3, 0.7))
                                            .rounded_corners(6.0)
                                            .padding(8.0),
                                        TextStyle::default(),
                                    );
                                });
                            },
                        );
                    },
                );
            },
        );
    });
}

#[composable]
pub fn positioned_boxes_showcase() {
    Column(Modifier::empty(), ColumnSpec::default(), || {
        Text(
            "=== Positioned Boxes ===",
            Modifier::empty()
                .background(Color(1.0, 1.0, 1.0, 0.1))
                .rounded_corners(14.0)
                .padding(12.0),
            TextStyle::default(),
        );

        Spacer(Modifier::empty().size_points(0.0, 16.0));

        cranpose_ui::Box(
            Modifier::empty()
                .size_points(360.0, 280.0)
                .background(Color(0.05, 0.05, 0.15, 0.5))
                .rounded_corners(8.0),
            BoxSpec::default(),
            || {
                cranpose_ui::Box(
                    Modifier::empty()
                        .size_points(100.0, 100.0)
                        .offset(20.0, 20.0)
                        .background(Color(0.6, 0.2, 0.7, 0.85))
                        .rounded_corners(12.0)
                        .padding(8.0),
                    BoxSpec::default(),
                    || {
                        Text(
                            "Box A",
                            Modifier::empty().padding(6.0),
                            TextStyle::default(),
                        );
                    },
                );

                cranpose_ui::Box(
                    Modifier::empty()
                        .size_points(100.0, 100.0)
                        .offset(220.0, 160.0)
                        .background(Color(0.2, 0.7, 0.4, 0.85))
                        .rounded_corners(12.0)
                        .padding(8.0),
                    BoxSpec::default(),
                    || {
                        Text(
                            "Box B",
                            Modifier::empty().padding(6.0),
                            TextStyle::default(),
                        );
                    },
                );

                cranpose_ui::Box(
                    Modifier::empty()
                        .size_points(80.0, 60.0)
                        .offset(140.0, 30.0)
                        .background(Color(0.9, 0.5, 0.2, 0.85))
                        .rounded_corners(10.0)
                        .padding(6.0),
                    BoxSpec::default(),
                    || {
                        Text("C", Modifier::empty().padding(4.0), TextStyle::default());
                    },
                );

                cranpose_ui::Box(
                    Modifier::empty()
                        .size_points(120.0, 80.0)
                        .offset(40.0, 140.0)
                        .background(Color(0.2, 0.5, 0.9, 0.85))
                        .rounded_corners(14.0)
                        .padding(8.0),
                    BoxSpec::default(),
                    || {
                        Text(
                            "Box D",
                            Modifier::empty().padding(6.0),
                            TextStyle::default(),
                        );
                    },
                );
            },
        );
    });
}

#[composable]
pub fn item_list_showcase() {
    Column(Modifier::empty(), ColumnSpec::default(), || {
        Text(
            "=== Item List (5 items) ===",
            Modifier::empty()
                .background(Color(1.0, 1.0, 1.0, 0.1))
                .rounded_corners(14.0)
                .padding(12.0),
            TextStyle::default(),
        );

        Spacer(Modifier::empty().size_points(0.0, 16.0));

        Column(
            Modifier::empty().padding(16.0),
            ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
            || {
                for i in 0..5 {
                    let (bg_color, border_color) = if i % 2 == 0 {
                        (Color(0.12, 0.16, 0.28, 0.8), Color(0.3, 0.5, 0.8, 0.9))
                    } else {
                        (Color(0.18, 0.12, 0.28, 0.8), Color(0.5, 0.3, 0.8, 0.9))
                    };

                    cranpose_ui::Box(
                        Modifier::empty()
                            .background(border_color)
                            .rounded_corners(12.0)
                            .padding(2.0),
                        BoxSpec::default(),
                        move || {
                            Row(
                                Modifier::empty()
                                    .background(bg_color)
                                    .rounded_corners(10.0)
                                    .padding(8.0)
                                    .size_points(400.0, 50.0),
                                RowSpec::default(),
                                move || {
                                    let text = match i {
                                        0 => "Item #0",
                                        1 => "Item #1",
                                        2 => "Item #2",
                                        3 => "Item #3",
                                        4 => "Item #4",
                                        _ => "Item",
                                    };
                                    Text(
                                        text,
                                        Modifier::empty().padding_horizontal(12.0),
                                        TextStyle::default(),
                                    );

                                    Spacer(Modifier::empty().size_points(0.0, 0.0));

                                    let status_color = if i % 3 == 0 {
                                        Color(0.2, 0.8, 0.3, 0.9)
                                    } else if i % 3 == 1 {
                                        Color(0.9, 0.7, 0.2, 0.9)
                                    } else {
                                        Color(0.8, 0.3, 0.2, 0.9)
                                    };

                                    cranpose_ui::Box(
                                        Modifier::empty()
                                            .size_points(12.0, 12.0)
                                            .background(status_color)
                                            .rounded_corners(6.0),
                                        BoxSpec::default(),
                                        || {},
                                    );
                                },
                            );
                        },
                    );
                }
            },
        );
    });
}

#[composable]
pub fn complex_chain_showcase() {
    Column(Modifier::empty(), ColumnSpec::default(), || {
        Text(
            "=== Complex Modifier Chain ===",
            Modifier::empty()
                .background(Color(1.0, 1.0, 1.0, 0.1))
                .rounded_corners(14.0)
                .padding(12.0),
            TextStyle::default(),
        );

        Spacer(Modifier::empty().size_points(0.0, 16.0));

        Text(
            "Nested: Red → Green → Blue layers",
            Modifier::empty().padding(8.0),
            TextStyle::default(),
        );

        Spacer(Modifier::empty().size_points(0.0, 12.0));

        cranpose_ui::Box(
            Modifier::empty()
                .background(Color(0.8, 0.2, 0.2, 0.9))
                .rounded_corners(16.0)
                .padding(8.0),
            BoxSpec::default(),
            || {
                cranpose_ui::Box(
                    Modifier::empty()
                        .background(Color(0.2, 0.7, 0.3, 0.9))
                        .rounded_corners(12.0)
                        .padding(6.0),
                    BoxSpec::default(),
                    || {
                        cranpose_ui::Box(
                            Modifier::empty()
                                .background(Color(0.3, 0.5, 0.9, 0.9))
                                .rounded_corners(8.0)
                                .padding(12.0),
                            BoxSpec::default(),
                            || {
                                Text("Nested!", Modifier::empty(), TextStyle::default());
                            },
                        );
                    },
                );
            },
        );

        Spacer(Modifier::empty().size_points(0.0, 16.0));

        Text(
            "Chain: offset + size + multiple backgrounds",
            Modifier::empty().padding(8.0),
            TextStyle::default(),
        );

        Spacer(Modifier::empty().size_points(0.0, 12.0));

        cranpose_ui::Box(
            Modifier::empty()
                .offset(20.0, 0.0)
                .size_points(180.0, 80.0)
                .background(Color(0.9, 0.6, 0.2, 0.9))
                .rounded_corners(10.0)
                .padding(6.0),
            BoxSpec::default(),
            || {
                cranpose_ui::Box(
                    Modifier::empty()
                        .background(Color(0.5, 0.3, 0.7, 0.9))
                        .rounded_corners(6.0)
                        .padding(8.0),
                    BoxSpec::default(),
                    || {
                        Text("Offset + Sized", Modifier::empty(), TextStyle::default());
                    },
                );
            },
        );
    });
}

#[composable]
pub fn dynamic_modifiers_showcase() {
    let frame = cranpose_core::rememberMutableStateOf(|| 0i32);

    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Text(
            "=== Dynamic Modifiers ===",
            Modifier::empty()
                .background(Color(1.0, 1.0, 1.0, 0.1))
                .rounded_corners(14.0)
                .padding(12.0),
            TextStyle::default(),
        );

        Spacer(Modifier::empty().size_points(0.0, 16.0));

        let current_frame = frame.get();
        let x = (current_frame as f32 * 10.0) % 200.0;
        let y = 50.0;

        cranpose_ui::Box(
            Modifier::empty()
                .size_points(250.0, 150.0)
                .background(Color(0.05, 0.05, 0.15, 0.5))
                .rounded_corners(8.0),
            BoxSpec::default(),
            move || {
                cranpose_ui::Box(
                    Modifier::empty()
                        .size(Size {
                            width: 50.0,
                            height: 50.0,
                        })
                        .offset(x, y)
                        .background(Color(0.3, 0.6, 0.9, 0.9))
                        .rounded_corners(10.0)
                        .padding(6.0),
                    BoxSpec::default(),
                    || {
                        Text("Move", Modifier::empty(), TextStyle::default());
                    },
                );
            },
        );

        Spacer(Modifier::empty().size_points(0.0, 16.0));

        Text(
            format!("Frame: {current_frame}, X: {x:.1}"),
            Modifier::empty()
                .background(Color(0.2, 0.2, 0.3, 0.6))
                .rounded_corners(10.0)
                .padding(8.0),
            TextStyle::default(),
        );

        Spacer(Modifier::empty().size_points(0.0, 12.0));

        Button(
            Modifier::empty()
                .rounded_corners(12.0)
                .draw_behind(|scope| {
                    scope.draw_round_rect(
                        Brush::solid(Color(0.25, 0.45, 0.85, 1.0)),
                        CornerRadii::uniform(12.0),
                    );
                })
                .padding(10.0),
            ButtonSpec::default(),
            move || {
                frame.set(frame.get() + 1);
            },
            || {
                Text(
                    "Advance Frame",
                    Modifier::empty().padding(6.0),
                    TextStyle::default(),
                );
            },
        );
    });
}

#[composable]
pub fn long_list_showcase() {
    Column(Modifier::empty(), ColumnSpec::default(), || {
        Text(
            "=== Long List (50 items) ===",
            Modifier::empty()
                .background(Color(1.0, 1.0, 1.0, 0.1))
                .rounded_corners(14.0)
                .padding(12.0),
            TextStyle::default(),
        );

        Spacer(Modifier::empty().size_points(0.0, 16.0));

        Column(
            Modifier::empty().padding(16.0),
            ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
            || {
                for i in 0..50 {
                    Text(
                        format!("Item {i}"),
                        Modifier::empty()
                            .background(Color(0.12 + (i as f32 * 0.005), 0.15, 0.25, 0.7))
                            .rounded_corners(8.0)
                            .padding_symmetric(8.0, 4.0)
                            .size_points(400.0, 40.0)
                            .padding_horizontal(12.0),
                        TextStyle::default(),
                    );
                }
            },
        );
    });
}
