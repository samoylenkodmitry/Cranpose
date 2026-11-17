// A Cranpose app for headsets. In a Meta Quest its panels stand in the room, and a controller's
// trigger presses the buttons: one counts, the other shows or hides the room around the panels.
// A strip above them is bent around the viewer. On the desktop the panels draw flat, and a panel
// plays two clips through the platform's decoder and a player surface.

use std::{cell::RefCell, rc::Rc, time::Duration};

use cranpose::{CropRect, DecodedFrames, DecoderStats, VideoCodec, prelude::*};
use cranpose_ui::text::{FontWeight, TextUnit};
use cranpose_xr::{
    Fov, Pose, SpatialCurvedRow, SpatialExternalSurface, SpatialPanel, StereoMode, StreamedScene,
    StreamedView, StreamedViews, Subspace, SubspaceModifier, local_spatial_capabilities,
    local_spatial_environment,
};

mod clip_player;
mod pattern_writer;

use clip_player::ClipPlayer;
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
    Subspace(move || {
        // A flat window shows the panels side by side, so the clips come first
        #[cfg(not(target_os = "android"))]
        FlatClips();
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
        PressesPanel(presses);
    });
}

// A panel that plays two clips flat in a window: H.264 fitted into a box of its shape, and HEVC
// cropped into a square. A headset plays the clip into a surface instead.
#[cfg(not(target_os = "android"))]
#[composable]
fn FlatClips() {
    SpatialPanel(SubspaceModifier::new().size(900.0, 480.0), || {
        Column(
            Modifier::empty()
                .fill_max_size()
                .background(Color(0.08, 0.1, 0.12, 0.94))
                .rounded_corners(24.0)
                .padding(24.0),
            ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(12.0)),
            || {
                Text(
                    "Hardware decoder, then PlayerSurface",
                    Modifier::empty(),
                    sized(28.0, Some(FontWeight::BOLD)),
                );
                Row(
                    Modifier::empty(),
                    RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(24.0)),
                    || {
                        FlatClip("H.264 fitted", VideoCodec::H264, ContentScale::Fit, 480.0);
                        FlatClip("HEVC cropped", VideoCodec::Hevc, ContentScale::Crop, 270.0);
                    },
                );
            },
        );
    });
}

// A clip that plays as long as the box is composed, in a player surface `width` points wide and
// 270 high, with a line of the decoder's counts below it.
#[cfg(not(target_os = "android"))]
#[composable]
fn FlatClip(title: &'static str, codec: VideoCodec, content_scale: ContentScale, width: f32) {
    let frames = rememberMutableStateOf(|| None::<DecodedFrames>);
    let stats = rememberMutableStateOf(DecoderStats::default);
    let failure = rememberMutableStateOf(|| None::<String>);
    DisposableEffect(codec, move |_| {
        let player = match ClipPlayer::start_flat(codec) {
            Ok((player, decoded)) => {
                frames.set(Some(decoded));
                Some(player)
            }
            Err(error) => {
                failure.set(Some(error.to_string()));
                None
            }
        };
        DisposableEffectResult::new(move || drop(player))
    });
    LaunchedEffect(frames.value(), async move |_| {
        let Some(decoded) = frames.value() else {
            return;
        };
        loop {
            delay(Duration::from_secs(1)).await;
            stats.set(decoded.stats());
        }
    });
    Column(
        Modifier::empty(),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(8.0)),
        move || {
            Text(
                title,
                Modifier::empty(),
                sized(22.0, Some(FontWeight::BOLD)),
            );
            if let Some(decoded) = frames.value() {
                PlayerSurface(
                    decoded,
                    Modifier::empty()
                        .size_points(width, 270.0)
                        .background(Color(0.0, 0.0, 0.0, 1.0)),
                    content_scale,
                );
            }
            let counts = stats.value();
            Text(
                failure.value().unwrap_or_else(|| {
                    format!(
                        "{} pictures, decode {:.2} ms, {} dropped",
                        counts.decoded,
                        counts.mean_decode.as_secs_f64() * 1000.0,
                        counts.dropped
                    )
                }),
                Modifier::empty(),
                sized(18.0, None),
            );
        },
    );
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

// A smaller panel to the right, turned toward the viewer, that counts the presses too.
#[composable]
fn PressesPanel(presses: MutableState<u32>) {
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
        fov: Fov::symmetric(0.4358, 0.4899),
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
