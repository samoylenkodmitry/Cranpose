// A Cranpose app for headsets. In a Meta Quest its panels stand in the room, and a controller's
// trigger presses the buttons: one counts, the other shows or hides the room around the panels.
// A strip above them is bent around the viewer. On the desktop the panels draw flat.

use std::{cell::RefCell, rc::Rc};

use cranpose::{CropRect, DecodedFrames, prelude::*};
use cranpose_ui::text::{FontWeight, TextUnit};
use cranpose_xr::{
    FieldOfView, Pose, SpatialCurvedRow, SpatialExternalSurface, SpatialPanel, StereoMode,
    StreamedScene, StreamedView, StreamedViews, Subspace, SubspaceModifier,
    local_headset_properties, local_spatial_capabilities, local_spatial_environment,
};

mod clip_player;
mod headset_readout;
mod pattern_writer;

use clip_player::ClipPlayer;
use headset_readout::{HeadsetReadout, Readout};
use pattern_writer::{PatternWriter, Split};

cranpose::app_capabilities!();

// The app: a panel with a counter, a smaller panel to the right and a 16:9 video surface to the
// left, both turned toward the viewer, and a strip above them that is bent around the viewer.
// The hardware decoder plays a clip into the video surface, and the same clip's frames go to a
// streamed scene below the panels. A thread writes a moving test pattern into a small
// side-by-side surface above the right panel and into a small top-bottom surface below it. Each
// shows a different color to each eye.
#[composable]
pub fn XrDemo() {
    let presses = rememberMutableStateOf(|| 0_u32);
    let readout = rememberMutableStateOf(Readout::default);
    Subspace(move || {
        HeadsetReadout(presses, readout);
        ClipSurface(
            StereoMode::Mono,
            SubspaceModifier::new()
                .size(640.0, 360.0)
                .offset(-760.0, 0.0, 50.0)
                .rotate(0.0, 25.0, 0.0),
        );
        PatternSurface(
            StereoMode::SideBySide,
            SubspaceModifier::new()
                .size(320.0, 180.0)
                .offset(600.0, 260.0, 100.0)
                .rotate(0.0, -30.0, 0.0),
            Split::LeftRight,
        );
        PatternSurface(
            StereoMode::TopBottom,
            SubspaceModifier::new()
                .size(320.0, 180.0)
                .offset(600.0, -260.0, 100.0)
                .rotate(0.0, -30.0, 0.0),
            Split::TopBottom,
        );
        CounterPanel(presses);
        CurvedStrip();
        PressesPanel(presses, readout);
    });
}

// The panel with the counter and the button for the room. A handle under it carries it.
#[composable]
fn CounterPanel(presses: MutableState<u32>) {
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
}

// A strip above the panels that is bent around the viewer.
#[composable]
fn CurvedStrip() {
    SpatialCurvedRow(SubspaceModifier::new(), 1000.0, || {
        SpatialPanel(
            SubspaceModifier::new()
                .size(1200.0, 220.0)
                .offset(0.0, 480.0, 0.0),
            || {
                Column(
                    Modifier::empty()
                        .fill_max_size()
                        .background(Color(0.1, 0.14, 0.12, 0.94))
                        .rounded_corners(24.0)
                        .padding(32.0),
                    ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(12.0)),
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
}

// A smaller panel to the right, turned toward the viewer, that counts the presses too and shows
// what the frame effect reads of the headset.
#[composable]
fn PressesPanel(presses: MutableState<u32>, readout: MutableState<Readout>) {
    SpatialPanel(
        SubspaceModifier::new()
            .size(320.0, 260.0)
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
                    Column(
                        Modifier::empty(),
                        ColumnSpec::default()
                            .vertical_arrangement(LinearArrangement::spaced_by(6.0)),
                        move || {
                            Text(
                                format!("{} presses", presses.get()),
                                Modifier::empty(),
                                sized(28.0, Some(FontWeight::BOLD)),
                            );
                            ReadoutLines(readout);
                        },
                    );
                },
            );
        },
    );
}

// The lines of the headset's readout. Outside a headset the frame effect never runs, and they
// stay at their start.
#[composable]
fn ReadoutLines(readout: MutableState<Readout>) {
    let Readout {
        head_cm,
        trigger_percent,
        frames_per_second,
    } = readout.get();
    let head = head_cm.map_or_else(|| "not tracked".to_owned(), |cm| format!("{cm} cm"));
    let (system, eye) = local_headset_properties().current().map_or_else(
        || (String::new(), String::new()),
        |headset| {
            let [width, height] = headset.eye_image_sizes[0].recommended;
            (headset.system_name, format!("Eye image {width}x{height}"))
        },
    );
    for line in [
        system,
        eye,
        format!("Head y {head}"),
        format!("Right trigger {trigger_percent}%"),
        format!("Frame effect {frames_per_second} Hz"),
    ] {
        Text(line, Modifier::empty(), sized(18.0, None));
    }
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

// A surface that a thread fills with the test pattern from its creation to its end.
#[composable]
fn PatternSurface(stereo_mode: StereoMode, modifier: SubspaceModifier, split: Split) {
    let writer = remember(|| Rc::new(RefCell::new(None::<PatternWriter>))).with(Clone::clone);
    SpatialExternalSurface(stereo_mode, modifier, move |scope| {
        let started = Rc::clone(&writer);
        scope.on_surface_created(move |surface| {
            started
                .borrow_mut()
                .replace(PatternWriter::start(surface, split));
        });
        let stopped = Rc::clone(&writer);
        scope.on_surface_destroyed(move |_| {
            stopped.borrow_mut().take();
        });
    });
}

// A surface that the hardware decoder plays the clip into. The decoder's second output, its
// frames, goes to a streamed scene: the left half of the clip is the left eye's picture, and the
// right half the right eye's, rendered from fixed poses.
#[composable]
fn ClipSurface(stereo_mode: StereoMode, modifier: SubspaceModifier) {
    let frames = rememberMutableStateOf(|| None::<DecodedFrames>);
    let player = remember(|| Rc::new(RefCell::new(None::<ClipPlayer>))).with(Clone::clone);
    SpatialExternalSurface(stereo_mode, modifier, move |scope| {
        let started = Rc::clone(&player);
        scope.on_surface_created(move |surface| match ClipPlayer::start(surface) {
            Ok((clip, decoded)) => {
                started.borrow_mut().replace(clip);
                frames.set(Some(decoded));
            }
            Err(error) => log::error!("xr demo: no clip: {error}"),
        });
        let stopped = Rc::clone(&player);
        scope.on_surface_destroyed(move |_| {
            stopped.borrow_mut().take();
            frames.set(None);
        });
    });
    if let Some(frames) = frames.get() {
        StreamedScene(frames, clip_views);
    }
}

// The views of a clip frame: both eyes look 30 degrees down from level, below the panels. The
// eyes are 64 millimeters apart. The field of view of each has the shape of its half of the clip,
// which is 320 by 360 pixels.
fn clip_views(_frame: u64) -> StreamedViews {
    const DOWN: f32 = 30.0 * std::f32::consts::PI / 180.0;
    let (sin, cos) = (DOWN / 2.0).sin_cos();
    let eye = |x: f32, left: u32| StreamedView {
        pose: Pose {
            position: [x, 0.0, 0.0],
            orientation: [-sin, 0.0, 0.0, cos],
        },
        fov: FieldOfView::symmetric(0.4358, 0.4899),
        rect: CropRect {
            x: left,
            y: 0,
            width: 320,
            height: 360,
        },
    };
    StreamedViews {
        left: eye(-0.032, 0),
        right: eye(0.032, 320),
    }
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
