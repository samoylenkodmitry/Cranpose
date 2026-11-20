// A Cranpose app for headsets. In a Meta Quest its panels stand in the room, and a controller's
// trigger or a hand's pinch presses the buttons: one counts, the other shows or hides the room
// around the panels.
// A lit torus stands beside them in 3D, and a handle under it lets a controller carry it. A glTF
// crate with a texture turns beside the torus.
// A strip above them is bent around the viewer. A ball above the left controller shows or hides
// the strip when the right controller's ray presses it. The counter panel and the video surface
// have a handle at their bottom right corner that resizes them, and the counter panel one under it
// that carries it. A text field in a small panel over the counter panel opens a keyboard when it
// takes focus: the system's on Android XR, and the one that Cranpose draws below the panel elsewhere.
// On the desktop the panels draw flat, and a panel plays two clips through the platform's decoder
// and a player surface.

use std::{
    cell::{OnceCell, RefCell},
    rc::Rc,
    time::Duration,
};

use cranpose::{CropRect, DecodedFrames, prelude::*};
use cranpose_ui::text::FontWeight;
use cranpose_xr::{
    DEFAULT_PANEL_SIZE, FieldOfView, FollowMode, FollowTarget, FollowingSubspace, Hand,
    OnNotEntitled, Pose, ResizeLimits, ResizePolicy, SpatialCurvedRow, SpatialExternalSurface,
    SpatialPanel, StereoMode, StreamStats, StreamedScene, StreamedView, StreamedViews, Subspace,
    SubspaceModifier, local_entitlement_check, local_spatial_capabilities,
    local_spatial_environment,
};
#[cfg(not(target_os = "android"))]
use player_demo::Clips;
use player_demo::sized;

mod clip_player;
mod controller_models;
mod crate_model;
mod decoder_limits;
mod foveated_player;
mod headset_readout;
mod hum;
mod pattern_writer;
mod presses_panel;
mod ray_target;
mod room_panel;
#[cfg(all(feature = "android", target_os = "android"))]
mod scripted_hand;
mod sound_probe;
mod torus_volume;
mod tracking_panel;
mod type_panel;

use clip_player::{ClipPlayer, target_display_time};
use controller_models::{ControllerModelsButton, ModelsOnly, StartingChoice, models_only};
use crate_model::CrateModel;
use foveated_player::FoveatedPlayer;
use headset_readout::{HeadsetClock, HeadsetReadout, Readout, StreamLogs};
use hum::rememberHum;
use pattern_writer::PatternWriter;
use presses_panel::PressesPanel;
use ray_target::rememberRayTarget;
use room_panel::RoomDemo;
use sound_probe::{SoundProbe, probe_build};
use torus_volume::TorusVolume;
use tracking_panel::{TrackedHands, TrackingSection, TrackingView};
use type_panel::TypePanel;

cranpose::app_capabilities!();

// The ball's diameter in dp, which are millimeters.
const BALL: f32 = 40.0;

// The app: a panel with a counter, a smaller panel to the right and a 16:9 video surface to the
// left, both turned toward the viewer, and a strip above them that is bent around the viewer.
// The hardware decoder plays a clip into the video surface, and the same clip's frames go to a
// streamed scene below the panels. A thread writes a moving test pattern into a small
// side-by-side surface above the right panel and into a small top-bottom surface below it. Each
// shows a different color to each eye.
#[composable]
pub fn XrDemo() {
    if depth_test_build() {
        DepthTest();
        return;
    }
    if matches!(ROOM_BUILD, Some("only" | "script")) {
        RoomDemo(room_panel::scripted(ROOM_BUILD));
        return;
    }
    if models_only() {
        ModelsOnly();
        return;
    }
    StartingChoice();
    let presses = rememberMutableStateOf(|| 0_u32);
    let strip_shown = rememberMutableStateOf(|| true);
    let readout = rememberMutableStateOf(Readout::default);
    let clock = remember(HeadsetClock::default).with(Clone::clone);
    let logs = remember(StreamLogs::default).with(Clone::clone);
    remember(decoder_limits::log_in_background);
    let tracking = TrackedHands();
    let hum = rememberHum();
    let target = rememberRayTarget();
    EntitlementChoice();
    Subspace(move || {
        HeadsetReadout(presses, readout, clock.clone(), logs.clone());
        // A flat window shows the panels side by side, so the clips come first
        #[cfg(not(target_os = "android"))]
        FlatStreamedClip(logs.clip.clone());
        #[cfg(not(target_os = "android"))]
        FlatClips();
        // The surface's corner handle stands behind its right edge, so the surface stands far
        // enough to the left that the counter panel does not cover the handle
        ClipSurface(
            StereoMode::Mono,
            SubspaceModifier::new()
                .size(640.0, 360.0)
                .offset(-860.0, 0.0, 50.0)
                .rotate(0.0, 25.0, 0.0),
            clock.clone(),
            logs.clip.clone(),
        );
        ClipLabel();
        FoveatedScene(clock.clone(), logs.foveated.clone());
        PatternSurface(
            StereoMode::SideBySide,
            SubspaceModifier::new()
                .size(320.0, 180.0)
                .offset(600.0, 260.0, 100.0)
                .rotate(0.0, -30.0, 0.0),
        );
        // A volume takes two of the compositor's layers, one for the 3D content and one for its
        // handle. A Quest takes 16. With both rays, the ball and the text panel the demo has more,
        // and the open keyboard is one more, so the frame leaves out the panels that stand
        // farthest from the head. The torus hums: the sound comes from the torus and follows it
        // when a controller carries it. A build that plays the hum from the head's side leaves
        // the torus silent.
        TorusVolume(
            SubspaceModifier::new()
                .offset(600.0, -330.0, 100.0)
                .rotate(0.0, -30.0, 0.0),
            true,
            target,
            hum.filter(|_| !probe_build()).map(hum::source),
        );
        if let Some(sound) = hum.filter(|_| probe_build()) {
            SoundProbe(sound);
        }
        // The crate's volume has no handle, so it adds no layer to the frame: the torus's volume
        // already brings the projection layer, which all volumes share
        CrateModel(
            SubspaceModifier::new()
                .offset(950.0, -330.0, 100.0)
                .rotate(0.0, -30.0, 0.0),
            true,
            target,
        );
        CounterPanel(presses, tracking);
        TypePanel();
        if strip_shown.get() {
            CurvedStrip();
        }
        PressesPanel(presses, readout, target);
    });
    ControllerBall(strip_shown);
    if ROOM_BUILD == Some("with") {
        RoomDemo(false);
    }
}

// A build with the `meta-entitlement` feature quits when the store's check fails. A build made with
// `XR_DEMO_ENTITLEMENT=continue` goes on instead. Its readout shows what the check found.
const ENTITLEMENT_BUILD: Option<&str> = option_env!("XR_DEMO_ENTITLEMENT");

#[composable]
fn EntitlementChoice() {
    if ENTITLEMENT_BUILD == Some("continue")
        && let Some(check) = local_entitlement_check().current()
    {
        // The choice is made once, before the check can answer
        remember(move || check.set_on_not_entitled(OnNotEntitled::Continue));
    }
}

// The depth test builds are `XR_DEMO_VOLUME=through`, which shows the panel and the torus, and
// `XR_DEMO_VOLUME=alone`, which shows the torus only. A screenshot of the second build tells how
// many pixels the whole torus covers. `XR_DEMO_VOLUME=model` shows the glTF crate only, standing
// still and turned to show three faces, so that a screenshot tells how many pixels its texture
// covers in each eye.
const VOLUME_BUILD: Option<&str> = option_env!("XR_DEMO_VOLUME");

// The room builds ask for the anchors and the planes, which the user's permission gates, so the
// default build leaves them out. `XR_DEMO_ROOM=with` adds the room panel to the demo, `only` shows it
// alone and `script` shows it alone and places it with no user. See `room_panel`.
const ROOM_BUILD: Option<&str> = option_env!("XR_DEMO_ROOM");

fn depth_test_build() -> bool {
    matches!(VOLUME_BUILD, Some("through" | "alone" | "model"))
}

// A test of the compositor's depth test: a panel and a torus that pierces it, a meter ahead of the
// head where the demo started. The ring's near half lies in front of the panel's plane and its far
// half behind it. A compositor that tests depth shows the near half over the panel and hides the far
// half. One that does not shows the panel over the whole ring. The build shows nothing else, so that a
// screenshot holds only the panel and the ring.
#[composable]
fn DepthTest() {
    if VOLUME_BUILD == Some("model") {
        FollowingSubspace(FollowTarget::View(FollowMode::Snap), || {
            CrateModel(
                SubspaceModifier::new().rotate(20.0, 30.0, 0.0),
                false,
                rememberRayTarget(),
            );
        });
        return;
    }
    FollowingSubspace(FollowTarget::View(FollowMode::Snap), || {
        if VOLUME_BUILD != Some("alone") {
            SpatialPanel(SubspaceModifier::new().size(600.0, 600.0), || {
                Box(
                    Modifier::empty()
                        .fill_max_size()
                        .background(Color(0.85, 0.2, 0.5, 1.0))
                        .rounded_corners(24.0)
                        .padding(24.0),
                    BoxSpec::default(),
                    || {
                        Text(
                            "This panel stands at z = 0",
                            Modifier::empty(),
                            sized(28.0, Some(FontWeight::BOLD)),
                        );
                    },
                );
            });
        }
        TorusVolume(
            SubspaceModifier::new().rotate(0.0, 45.0, 0.0),
            false,
            rememberRayTarget(),
            None,
        );
    });
}

// A ball of 4 cm that floats 10 cm above the left controller, upright wherever the hand tilts.
// The right controller's ray presses it to show or hide the curved strip. The left ray passes
// through it. It stays while the panels are hidden, so it brings them back. A window has no
// controller and no ball.
#[composable]
fn ControllerBall(strip_shown: MutableState<bool>) {
    if !local_spatial_capabilities()
        .current()
        .is_spatial_ui_enabled()
    {
        return;
    }
    FollowingSubspace(
        FollowTarget::Controller(Hand::Left, FollowMode::Tight),
        move || {
            SpatialPanel(
                SubspaceModifier::new()
                    .size(BALL, BALL)
                    .offset(0.0, 100.0, 0.0),
                move || {
                    let color = if strip_shown.get() {
                        Color(0.2, 0.6, 0.45, 1.0)
                    } else {
                        Color(0.45, 0.45, 0.5, 1.0)
                    };
                    Box(
                        Modifier::empty()
                            .fill_max_size()
                            .background(color)
                            .rounded_corners(BALL / 2.0)
                            .clickable(move |_| strip_shown.set(!strip_shown.get())),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        },
    );
}

// A label 2 cm in front of the video surface, at its upper left. The compositor draws it over the
// video, because the layers go back to front by the distance of their planes from the head.
#[composable]
fn ClipLabel() {
    SpatialPanel(
        SubspaceModifier::new()
            .size(260.0, 70.0)
            .offset(-1023.7, 130.0, 148.4)
            .rotate(0.0, 25.0, 0.0),
        || {
            Box(
                Modifier::empty()
                    .fill_max_size()
                    .background(Color(0.9, 0.5, 0.1, 1.0))
                    .rounded_corners(16.0)
                    .padding(16.0),
                BoxSpec::default(),
                || {
                    Text(
                        "In front of the video",
                        Modifier::empty(),
                        sized(24.0, None),
                    );
                },
            );
        },
    );
}

// A panel that plays two clips flat in a window: H.264 fitted into a box of its shape, and HEVC
// cropped to fill its box. A headset plays the clip into a surface instead.
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
            Clips,
        );
    });
}

// The frames of the clip in a streamed scene, which a window shows as the left eye's picture, the
// left half of the clip, fitted to its box. A headset shows the scene of `ClipSurface` instead.
#[cfg(not(target_os = "android"))]
#[composable]
fn FlatStreamedClip(stats: StreamStats) {
    let started = remember(|| {
        ClipPlayer::start_frames()
            .map(|(player, frames)| (Rc::new(player), frames))
            .map_err(|error| log::error!("xr demo: no flat clip: {error}"))
            .ok()
    })
    .with(Clone::clone);
    if let Some((_player, frames)) = started {
        Box(
            Modifier::empty().size_points(320.0, 360.0),
            BoxSpec::default(),
            move || {
                StreamedScene(
                    frames.clone(),
                    |index| clip_views(index, Duration::ZERO),
                    stats.clone(),
                );
            },
        );
    }
}

// The panel with the counter and the button for the room. A handle under it carries it, and a
// handle at its bottom right corner resizes it. The size is the state that the resize sets.
#[composable]
fn CounterPanel(presses: MutableState<u32>, tracking: TrackingView) {
    let size = rememberMutableStateOf(|| [800.0_f32, 800.0]);
    let [width, height] = size.get();
    SpatialPanel(
        SubspaceModifier::new()
            .size(width, height)
            .movable()
            .resizable_with(
                ResizeLimits::new()
                    .minimum_size(400.0, 300.0)
                    .maximum_size(1600.0, 1200.0),
                ResizePolicy::custom(move |event| size.set(event.size)),
            ),
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
                    ControllerModelsButton();
                    TrackingSection(tracking);
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
fn PatternSurface(stereo_mode: StereoMode, modifier: SubspaceModifier) {
    let writer = remember(|| Rc::new(RefCell::new(None::<PatternWriter>))).with(Clone::clone);
    SpatialExternalSurface(stereo_mode, modifier, move |scope| {
        let started = Rc::clone(&writer);
        scope.on_surface_created(move |surface| {
            started.borrow_mut().replace(PatternWriter::start(surface));
        });
        let stopped = Rc::clone(&writer);
        scope.on_surface_destroyed(move |_| {
            stopped.borrow_mut().take();
        });
    });
}

// A surface that the hardware decoder plays the clip into. A handle at its bottom right corner
// resizes it, and it keeps the clip's proportions, because the picture fills the whole surface.
// The decoder's second output, its frames, goes to a streamed scene: the left half of the clip is
// the left eye's picture, and the right half the right eye's, rendered from fixed poses. Each
// frame carries the time at which it should reach the display.
#[composable]
fn ClipSurface(
    stereo_mode: StereoMode,
    modifier: SubspaceModifier,
    clock: HeadsetClock,
    stats: StreamStats,
) {
    let size = rememberMutableStateOf(|| modifier.size_or(DEFAULT_PANEL_SIZE));
    let [width, height] = size.get();
    let modifier = modifier.size(width, height).resizable_with(
        ResizeLimits::new()
            .minimum_size(320.0, 180.0)
            .maximum_size(1280.0, 720.0)
            .maintain_aspect_ratio(true),
        ResizePolicy::custom(move |event| size.set(event.size)),
    );
    let frames = rememberMutableStateOf(|| None::<(DecodedFrames, Duration)>);
    let player = remember(|| Rc::new(RefCell::new(None::<ClipPlayer>))).with(Clone::clone);
    SpatialExternalSurface(stereo_mode, modifier, move |scope| {
        let started = Rc::clone(&player);
        let clock = clock.clone();
        scope.on_surface_created(move |surface| match ClipPlayer::start(surface) {
            Ok((clip, decoded)) => {
                started.borrow_mut().replace(clip);
                frames.set(Some((decoded, clock.read())));
            }
            Err(error) => log::error!("xr demo: no clip: {error}"),
        });
        let stopped = Rc::clone(&player);
        scope.on_surface_destroyed(move |_| {
            stopped.borrow_mut().take();
            frames.set(None);
        });
    });
    if let Some((frames, base)) = frames.get() {
        StreamedScene(frames, move |index| clip_views(index, base), stats);
    }
}

// A streamed scene of a foveated clip, which the headset unwarps. It lies above the panels. The
// first frame that the scene asks about anchors the stream on the headset's clock: the frame
// effect has set the clock by then, which it has not when the scene is composed.
#[composable]
fn FoveatedScene(clock: HeadsetClock, stats: StreamStats) {
    let started = remember(|| {
        FoveatedPlayer::start()
            .map(|(player, frames)| (Rc::new(player), frames))
            .map_err(|error| log::error!("xr demo: no foveated clip: {error}"))
            .ok()
    })
    .with(Clone::clone);
    let anchor = remember(|| Rc::new(OnceCell::<(u64, Duration)>::new())).with(Clone::clone);
    if let Some((_player, frames)) = started {
        StreamedScene(
            frames,
            move |index| {
                let (first, base) = *anchor.get_or_init(|| (index, clock.read()));
                foveated_player::views(index.saturating_sub(first), base)
            },
            stats,
        );
    }
}

// The views of a clip frame: both eyes look 30 degrees down from level, below the panels. The
// eyes are 64 millimeters apart. The field of view of each has the shape of its half of the clip,
// which is 320 by 360 pixels.
fn clip_views(index: u64, base: Duration) -> StreamedViews {
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
        foveation: None,
    };
    StreamedViews {
        left: eye(-0.032, 0),
        right: eye(0.032, 320),
        target_display_time: Some(target_display_time(base, index)),
    }
}

#[cfg(all(feature = "android", target_os = "android"))]
fn launcher() -> AppLauncher {
    if let Some(hand) = scripted_hand::from_build(
        option_env!("XR_DEMO_SCRIPTED_HAND"),
        option_env!("XR_DEMO_SCRIPTED_TEXT"),
    ) {
        cranpose_xr::script_hand(hand);
    }
    for controller in
        scripted_hand::controllers_from_build(option_env!("XR_DEMO_SCRIPTED_CONTROLLERS"))
    {
        cranpose_xr::script_controller(controller);
    }
    // `XR_DEMO_KEYBOARD=drawn` draws the keyboard on Android XR too, where the system shows its own
    if option_env!("XR_DEMO_KEYBOARD") == Some("drawn") {
        cranpose_xr::use_keyboard(cranpose_xr::Keyboard::Drawn);
    }
    AppLauncher::new().with_capabilities(&CAPABILITIES)
}

cranpose_xr::android_main! {
    launcher: launcher(),
    content: XrDemo,
}
