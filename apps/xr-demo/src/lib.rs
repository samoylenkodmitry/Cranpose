// A Cranpose app for headsets. In a Meta Quest its two panels stand in the room, and a
// controller's trigger presses the button. On the desktop the panels draw flat.

use cranpose::prelude::*;
use cranpose_ui::text::{FontWeight, TextUnit};
use cranpose_xr::{SpatialPanel, Subspace, SubspaceModifier};

cranpose::app_capabilities!();

// The app: a panel with a counter and a smaller panel to the right, turned toward the viewer.
#[composable]
pub fn XrDemo() {
    let presses = rememberMutableStateOf(|| 0_u32);
    Subspace(move || {
        SpatialPanel(
            SubspaceModifier::new().size(800.0, 500.0).movable(),
            move || {
                Column(
                    Modifier::empty()
                        .fill_max_size()
                        .background(Color(0.1, 0.11, 0.14, 0.94))
                        .rounded_corners(24.0)
                        .padding(40.0),
                    ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(24.0)),
                    move || {
                        Text(
                            "Cranpose in a headset",
                            Modifier::empty(),
                            sized(36.0, Some(FontWeight::BOLD)),
                        );
                        Text(
                            "This panel is a layer of the headset's compositor, which draws its text once at the display's resolution.",
                            Modifier::empty(),
                            sized(22.0, None),
                        );
                        Button(
                            Modifier::empty()
                                .background(Color(0.25, 0.45, 0.95, 1.0))
                                .rounded_corners(16.0)
                                .padding_symmetric(32.0, 18.0),
                            ButtonSpec::default(),
                            move || presses.set(presses.get() + 1),
                            move || {
                                Text(
                                    format!("Pressed {} times", presses.get()),
                                    Modifier::empty(),
                                    sized(24.0, Some(FontWeight::BOLD)),
                                );
                            },
                        );
                    },
                );
            },
        );
        SpatialPanel(
            SubspaceModifier::new()
                .size(320.0, 200.0)
                .offset(600.0, 0.0, 100.0)
                .rotate(0.0, -30.0, 0.0),
            move || {
                Box(
                    Modifier::empty()
                        .fill_max_size()
                        .background(Color(0.16, 0.13, 0.1, 0.94))
                        .rounded_corners(20.0)
                        .padding(24.0),
                    BoxSpec::default(),
                    move || {
                        Text(
                            format!("{} presses", presses.get()),
                            Modifier::empty(),
                            sized(28.0, Some(FontWeight::BOLD)),
                        );
                    },
                );
            },
        );
    });
}

fn sized(size: f32, weight: Option<FontWeight>) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(Color(0.95, 0.96, 0.98, 1.0)),
            font_size: TextUnit::Sp(size),
            font_weight: weight,
            ..SpanStyle::default()
        },
        ..TextStyle::default()
    }
}

#[cfg(all(feature = "android", target_os = "android"))]
fn launcher() -> AppLauncher {
    AppLauncher::new().with_capabilities(&CAPABILITIES)
}

cranpose_xr::android_main! {
    launcher: launcher(),
    content: XrDemo,
}
