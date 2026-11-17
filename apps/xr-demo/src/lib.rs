// A Cranpose app for headsets. In a Meta Quest its panels stand in the room, and a controller's
// trigger presses the buttons: one counts, the other shows or hides the room around the panels.
// A strip above them is bent around the viewer. On the desktop the panels draw flat.

use cranpose::prelude::*;
use cranpose_ui::text::{FontWeight, TextUnit};
use cranpose_xr::{
    SpatialCurvedRow, SpatialPanel, Subspace, SubspaceModifier, local_spatial_capabilities,
    local_spatial_environment,
};

cranpose::app_capabilities!();

// The app: a panel with a counter, a smaller panel to the right turned toward the viewer, and a
// curved strip above them.
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
                        PassthroughButton();
                    },
                );
            },
        );
        SpatialCurvedRow(SubspaceModifier::new(), 1000.0, || {
            SpatialPanel(
                SubspaceModifier::new()
                    .size(1200.0, 220.0)
                    .offset(0.0, 400.0, 0.0),
                || {
                    Column(
                        Modifier::empty()
                            .fill_max_size()
                            .background(Color(0.1, 0.14, 0.12, 0.94))
                            .rounded_corners(24.0)
                            .padding(32.0),
                        ColumnSpec::default()
                            .vertical_arrangement(LinearArrangement::spaced_by(12.0)),
                        || {
                            Text(
                                "A curved panel",
                                Modifier::empty(),
                                sized(34.0, Some(FontWeight::BOLD)),
                            );
                            Text(
                                "A cylinder layer of the headset's compositor, bent around you at a radius of one meter. Its text stays as sharp as a flat panel's.",
                                Modifier::empty(),
                                sized(22.0, None),
                            );
                        },
                    );
                },
            );
        });
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

// A button that shows or hides the room around the panels, or a note where the headset cannot.
// The room shows from the start.
#[composable]
fn PassthroughButton() {
    let Some(environment) = local_spatial_environment().current() else {
        return;
    };
    if !local_spatial_capabilities()
        .current()
        .is_passthrough_control_enabled()
    {
        Text(
            "This headset shows no room behind the panels.",
            Modifier::empty(),
            sized(20.0, None),
        );
        return;
    }
    DisposableEffect(true, move |_| {
        environment.set_preferred_passthrough_opacity(Some(1.0));
        DisposableEffectResult::new(|| {})
    });
    let shown = environment.current_passthrough_opacity() > 0.0;
    Button(
        Modifier::empty()
            .background(Color(0.2, 0.55, 0.4, 1.0))
            .rounded_corners(16.0)
            .padding_symmetric(32.0, 18.0),
        ButtonSpec::default(),
        move || {
            environment.set_preferred_passthrough_opacity(Some(if shown { 0.0 } else { 1.0 }));
        },
        move || {
            Text(
                if shown {
                    "Hide the room"
                } else {
                    "Show the room"
                },
                Modifier::empty(),
                sized(24.0, Some(FontWeight::BOLD)),
            );
        },
    );
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
