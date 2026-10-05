use cranpose_core::remember;
use cranpose_foundation::text::TextFieldState;
use cranpose_ui::{
    composable, local_ime_insets, Alignment, BasicTextField, Box, BoxSpec, Color, Column,
    ColumnSpec, Modifier, Text, TextStyle,
};

/// Physical keyboard fixture with a bottom marker driven only by inset padding.
#[composable]
pub fn InsetsRobotScreen() {
    let field = remember(|| TextFieldState::new("Type here")).with(|state| *state);
    Box(
        Modifier::empty()
            .fill_max_size()
            .background(Color::WHITE)
            .safe_area_padding(),
        BoxSpec::default(),
        move || {
            Column(
                Modifier::empty().fill_max_width().padding(20.0),
                ColumnSpec::default(),
                move || {
                    InsetsStatus();
                    BasicTextField(
                        field,
                        Modifier::empty()
                            .fill_max_width()
                            .height(64.0)
                            .content_description("Insets editor"),
                        TextStyle::default(),
                    );
                },
            );
            Box(
                Modifier::empty().fill_max_size().ime_padding(),
                BoxSpec::default().content_alignment(Alignment::BOTTOM_START),
                || {
                    Text(
                        "Bottom marker",
                        Modifier::empty()
                            .height(48.0)
                            .background(Color::rgb(0.0, 0.7, 0.0))
                            .content_description("Insets bottom marker"),
                        TextStyle::default(),
                    );
                },
            );
        },
    );
}

#[composable]
fn InsetsStatus() {
    let label = if local_ime_insets().current().bottom > 0.0 {
        "Insets open"
    } else {
        "Insets closed"
    };
    Text(
        label,
        Modifier::empty().content_description(label),
        TextStyle::default(),
    );
}
