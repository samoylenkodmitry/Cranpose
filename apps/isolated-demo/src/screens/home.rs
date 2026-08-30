#![allow(non_snake_case)]

use cranpose::prelude::*;
use cranpose_core::rememberMutableStateOf;

use crate::theme::{body_text_style, heading_text_style, Palette};

#[composable]
pub(crate) fn HomeScreen(palette: Palette) {
    let counter = rememberMutableStateOf(|| 0i32);
    let celebrating = rememberMutableStateOf(|| false);

    let card_background = if celebrating.value() {
        palette.primary
    } else {
        palette.surface
    };
    let card_text = if celebrating.value() {
        palette.on_primary
    } else {
        palette.text
    };

    Column(
        Modifier::empty().fill_max_size().padding(24.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(16.0)),
        move || {
            Text(
                "Counter and layout",
                Modifier::empty(),
                heading_text_style(palette.text),
            );
            Text(
                "A plain button drives a plain state value. Start reading the \
                 template here, then follow Tasks and Settings for input, \
                 lists, and app-level theming.",
                Modifier::empty(),
                body_text_style(palette.muted_text),
            );

            Row(
                Modifier::empty(),
                RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(12.0)),
                move || {
                    Button(
                        Modifier::empty()
                            .padding(12.0)
                            .background(palette.primary)
                            .rounded_corners(8.0),
                        ButtonSpec::default(),
                        move || counter.set(counter.value() + 1),
                        move || {
                            Text(
                                format!("Count: {}", counter.value()),
                                Modifier::empty(),
                                body_text_style(palette.on_primary),
                            );
                        },
                    );

                    Button(
                        Modifier::empty()
                            .padding(12.0)
                            .background(palette.surface)
                            .rounded_corners(8.0),
                        ButtonSpec::default(),
                        move || celebrating.set(!celebrating.value()),
                        move || {
                            Text(
                                if celebrating.value() {
                                    "Celebrating"
                                } else {
                                    "Celebrate"
                                },
                                Modifier::empty(),
                                body_text_style(palette.text),
                            );
                        },
                    );
                },
            );

            Box(
                Modifier::empty()
                    .fill_max_width()
                    .height(120.0)
                    .background(card_background)
                    .rounded_corners(12.0),
                BoxSpec::default().content_alignment(Alignment::CENTER),
                move || {
                    Text(
                        "Toggle the button above to swap this card's colors",
                        Modifier::empty(),
                        body_text_style(card_text),
                    );
                },
            );
        },
    );
}
