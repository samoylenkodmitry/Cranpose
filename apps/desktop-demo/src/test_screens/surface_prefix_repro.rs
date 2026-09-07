use cranpose::{
    composable, remember, Alignment, Box, BoxSpec, Brush, Color, Column, ColumnSpec,
    HorizontalAlignment, LinearArrangement, Modifier, Rect, Row, RowSpec, ScrollState, Size, Text,
    TextStyle, VerticalAlignment,
};
use cranpose_ui_graphics::GradientBlurDirection;

const PAPER: Color = Color(0.97, 0.97, 0.99, 1.0);
const BLUE: Color = Color(0.0, 0.5, 1.0, 1.0);

fn navigation_label() {
    Text(
        "Surface navigation",
        Modifier::empty(),
        TextStyle::default(),
    );
}

#[allow(non_snake_case)]
#[composable]
pub fn SurfacePrefixReproScreen() {
    let scroll = remember(|| ScrollState::new(0.0)).with(|state| *state);
    Box(
        Modifier::empty().fill_max_size().draw_behind(|scope| {
            let size = scope.size();
            scope.draw_rect_at(
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: size.width.ceil() + 1.0,
                    height: size.height.ceil() + 1.0,
                },
                Brush::solid(PAPER),
            );
        }),
        BoxSpec::default(),
        move || {
            Column(
                Modifier::empty()
                    .fill_max_size()
                    .vertical_scroll(scroll, false)
                    .padding_each(16.0, 90.0, 16.0, 110.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(12.0)),
                move || {
                    Text("Surface content", Modifier::empty(), TextStyle::default());
                    for index in 0..24 {
                        Row(
                            Modifier::empty()
                                .fill_max_width()
                                .height(90.0)
                                .shadow(3.0)
                                .background(Color::WHITE)
                                .rounded_corners(14.0)
                                .padding(16.0),
                            RowSpec::new()
                                .horizontal_arrangement(LinearArrangement::SpacedBy(16.0))
                                .vertical_alignment(VerticalAlignment::CenterVertically),
                            move || {
                                Box(
                                    Modifier::empty()
                                        .size(Size::new(36.0, 36.0))
                                        .background(BLUE),
                                    BoxSpec::default(),
                                    || {},
                                );
                                Text(
                                    format!("Visible row {index}"),
                                    Modifier::empty(),
                                    TextStyle::default(),
                                );
                            },
                        );
                    }
                    Text("Surface end", Modifier::empty(), TextStyle::default());
                },
            );
            Box(
                Modifier::empty()
                    .fill_max_width()
                    .height(65.0)
                    .backdrop_gradient_blur(
                        16.0.into(),
                        0.0.into(),
                        GradientBlurDirection::TopToBottom,
                    ),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty().fill_max_size().padding(8.0),
                BoxSpec::default().content_alignment(Alignment::new(
                    HorizontalAlignment::CenterHorizontally,
                    VerticalAlignment::Bottom,
                )),
                || {
                    Box(
                        Modifier::empty()
                            .fill_max_width()
                            .height(70.0)
                            .backdrop_blur(16.0.into())
                            .background(Color(1.0, 1.0, 1.0, 0.8))
                            .padding(16.0),
                        BoxSpec::default(),
                        navigation_label,
                    );
                },
            );
        },
    );
}
