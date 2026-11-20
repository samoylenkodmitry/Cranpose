// A panel with a text field. A text field that takes focus opens the keyboard: the system's input
// method where the runtime shows it over the app, as Android XR does, and a keyboard of Cranpose
// that stands below the panel elsewhere. A flat window has the keyboard of the machine.

use cranpose::prelude::*;
use cranpose_xr::{SpatialPanel, SubspaceModifier};
use player_demo::sized;

// The panel's size in dp, which are millimeters.
pub(crate) const SIZE: [f32; 2] = [460.0, 100.0];
// The place of the panel in its subspace: over the empty lower part of the counter panel and 4 cm
// in front of it, above where the system's keyboard shows on Android XR.
const OFFSET: [f32; 3] = [0.0, -310.0, 40.0];

// The place and size of the panel. The scripted hand reads the same modifier to find the field.
pub(crate) fn modifier() -> SubspaceModifier {
    SubspaceModifier::new()
        .size(SIZE[0], SIZE[1])
        .offset(OFFSET[0], OFFSET[1], OFFSET[2])
}

// The field fills the panel inside its padding, so the middle of the panel is in the field.
#[composable]
pub(crate) fn TypePanel() {
    let text = remember(|| TextFieldState::new("")).with(|text| *text);
    SpatialPanel(modifier(), move || {
        Box(
            Modifier::empty()
                .fill_max_size()
                .background(Color(0.1, 0.13, 0.17, 0.96))
                .rounded_corners(20.0)
                .padding(24.0),
            BoxSpec::default().content_alignment(Alignment::CENTER_START),
            move || {
                if text.text().is_empty() {
                    Text(
                        "Type here",
                        Modifier::empty(),
                        TextStyle {
                            span_style: SpanStyle {
                                color: Some(Color(0.55, 0.6, 0.68, 1.0)),
                                ..sized(28.0, None).span_style
                            },
                            ..TextStyle::default()
                        },
                    );
                }
                BasicTextField(text, Modifier::empty().fill_max_size(), sized(28.0, None));
            },
        );
    });
}
