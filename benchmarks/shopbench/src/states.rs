// The loading and error states of `spec/design/screens/States.dc.html`.

use cranpose::{
    Alignment, Box, BoxSpec, Column, ColumnSpec, HorizontalAlignment, LinearArrangement, Modifier,
    RoundedCornerShape, Text, composable, text::FontWeight, widgets::CircularProgressIndicator,
};

use crate::theme::{TEXT, TEXT_MUTED, centered, paragraph, style};

#[composable]
pub(crate) fn ErrorState(on_retry: impl Fn() + Clone + 'static) {
    Column(
        Modifier::empty().fill_max_size().padding_horizontal(32.0),
        ColumnSpec::new()
            .vertical_arrangement(LinearArrangement::Center)
            .horizontal_alignment(HorizontalAlignment::CenterHorizontally),
        move || {
            Text(
                "Couldn't load products",
                Modifier::empty(),
                style(17.0, FontWeight::BOLD, TEXT),
            );
            Text(
                "Check your connection and try again.",
                Modifier::empty().padding_each(0.0, 12.0, 0.0, 0.0),
                centered(paragraph(15.0, 22.0, FontWeight::NORMAL, TEXT_MUTED)),
            );
            Box(
                Modifier::empty()
                    .padding_each(0.0, 20.0, 0.0, 0.0)
                    .height(48.0)
                    .border(1.0, TEXT, RoundedCornerShape::uniform(12.0))
                    .test_tag("error-retry")
                    .clickable({
                        let on_retry = on_retry.clone();
                        move |_| on_retry()
                    })
                    .padding_horizontal(24.0),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                || {
                    Text(
                        "Retry",
                        Modifier::empty(),
                        style(16.0, FontWeight::SEMI_BOLD, TEXT),
                    );
                },
            );
        },
    );
}

#[composable]
pub(crate) fn Loading() {
    Box(
        Modifier::empty().fill_max_size(),
        BoxSpec::new().content_alignment(Alignment::CENTER),
        || {
            CircularProgressIndicator(Modifier::empty().size_points(28.0, 28.0), TEXT_MUTED, 3.0);
        },
    );
}

// The small spinner at the end of a list while its next page loads.
#[composable]
pub(crate) fn LoadingMore() {
    Box(
        Modifier::empty().fill_max_width().padding_vertical(16.0),
        BoxSpec::new().content_alignment(Alignment::CENTER),
        || {
            CircularProgressIndicator(Modifier::empty().size_points(20.0, 20.0), TEXT_MUTED, 2.0);
        },
    );
}
