use cranpose_core::{rememberMutableStateOf, with_key};
use cranpose_services::local_accessibility_options;
use cranpose_ui::{
    composable, Button, ButtonSpec, Column, ColumnSpec, LinearArrangement, Modifier, Text,
    TextStyle,
};

pub const PROBE_LABEL: &str = "Every label fits the box it measured";
pub const SHOW_COPY: &str = "Compose a fresh copy";

pub fn font_scale_readout(font_scale: f32) -> String {
    format!("Font scale {font_scale}")
}

#[allow(non_snake_case)]
#[composable]
pub fn FontScaleReproScreen() {
    let copies = rememberMutableStateOf(|| 0u32);
    Column(
        Modifier::empty().padding(24.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(12.0)),
        move || {
            Text(PROBE_LABEL, Modifier::empty(), TextStyle::default());
            Button(
                Modifier::empty(),
                ButtonSpec::default(),
                move || copies.set(copies.get() + 1),
                || {
                    Text(SHOW_COPY, Modifier::empty(), TextStyle::default());
                },
            );
            let count = copies.get();
            if count > 0 {
                with_key(&count, || {
                    let font_scale = local_accessibility_options().current().font_scale;
                    Text(
                        font_scale_readout(font_scale),
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                    Text(PROBE_LABEL, Modifier::empty(), TextStyle::default());
                });
            }
        },
    );
}
