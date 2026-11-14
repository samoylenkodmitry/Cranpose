// The navigation bar of a pushed screen: a back button and the title,
// centered.

use cranpose::{
    Alignment, Box, BoxSpec, Modifier, Row, RowSpec, TextWithOptions, VerticalAlignment,
    composable, text::FontWeight,
};

use crate::{
    icons::{IconKind, StrokeIcon},
    theme::{BAR_HEIGHT, TEXT, lines, style},
};

#[composable]
pub(crate) fn NavHeader(title: String, on_back: impl Fn() + 'static) {
    Row(
        Modifier::empty()
            .fill_max_width()
            .height(BAR_HEIGHT)
            .padding_horizontal(8.0),
        RowSpec::new().vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            BackButton(on_back.clone());
            Box(
                Modifier::empty().weight(1.0),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                {
                    let title = title.clone();
                    move || {
                        TextWithOptions(
                            title.clone(),
                            Modifier::empty(),
                            style(17.0, FontWeight::SEMI_BOLD, TEXT),
                            lines(1),
                        );
                    }
                },
            );
            Box(
                Modifier::empty().size_points(BAR_HEIGHT, BAR_HEIGHT),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

#[composable]
pub(crate) fn BackButton(on_back: impl Fn() + 'static) {
    Box(
        Modifier::empty()
            .size_points(BAR_HEIGHT, BAR_HEIGHT)
            .content_description("Back")
            .test_tag("back")
            .clickable(move |_| on_back()),
        BoxSpec::new().content_alignment(Alignment::CENTER),
        || StrokeIcon(IconKind::Back, 24.0, 1.75, TEXT),
    );
}
