#![allow(non_snake_case)]

use cranpose::prelude::*;

use crate::theme::{body_text_style, heading_text_style, Palette};

#[composable]
pub(crate) fn SettingsScreen(palette: Palette, dark_mode: MutableState<bool>) {
    Column(
        Modifier::empty().fill_max_size().padding(24.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(16.0)),
        move || {
            Text(
                "Settings",
                Modifier::empty(),
                heading_text_style(palette.text),
            );
            Text(
                "Cranpose ships no theme system: colors are plain values a \
                 caller passes down. This screen owns the one state value \
                 that picks between Palette::for_mode's two constants; a \
                 real app's own palette and settings belong here.",
                Modifier::empty(),
                body_text_style(palette.muted_text),
            );

            Row(
                Modifier::empty()
                    .fill_max_width()
                    .padding(12.0)
                    .background(palette.surface)
                    .rounded_corners(8.0),
                RowSpec::default()
                    .horizontal_arrangement(LinearArrangement::SpaceBetween)
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                move || {
                    Text(
                        "Dark mode",
                        Modifier::empty(),
                        body_text_style(palette.text),
                    );
                    Button(
                        Modifier::empty()
                            .padding(10.0)
                            .background(palette.primary)
                            .rounded_corners(8.0),
                        ButtonSpec::default(),
                        move || dark_mode.set(!dark_mode.value()),
                        move || {
                            Text(
                                if dark_mode.value() { "On" } else { "Off" },
                                Modifier::empty(),
                                body_text_style(palette.on_primary),
                            );
                        },
                    );
                },
            );
        },
    );
}
