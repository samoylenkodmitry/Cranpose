// A Cranpose app for headsets. In a Meta Quest its two panels stand in the room, and a
// controller's trigger presses the button. On the desktop the panels draw flat.

use std::{cell::RefCell, rc::Rc};

use cranpose::prelude::*;
use cranpose_ui::text::{FontWeight, TextUnit};
use cranpose_xr::{
    ExternalSurface, SpatialExternalSurface, SpatialPanel, StereoMode, Subspace, SubspaceModifier,
};

mod clip_player;
mod pattern_writer;

use clip_player::ClipPlayer;
use pattern_writer::{PatternWriter, Split};

cranpose::app_capabilities!();

// The app: a panel with a counter, a smaller panel to the right and a 16:9 video surface to the
// left, both turned toward the viewer. The hardware decoder plays a clip into the video surface.
// A thread writes a moving test pattern into a small side-by-side surface above the right panel
// and into a small top-bottom surface below it. Each shows a different color to each eye.
#[composable]
pub fn XrDemo() {
    let presses = rememberMutableStateOf(|| 0_u32);
    Subspace(move || {
        PlayedSurface(
            StereoMode::Mono,
            SubspaceModifier::new()
                .size(640.0, 360.0)
                .offset(-760.0, 0.0, 50.0)
                .rotate(0.0, 25.0, 0.0),
            Source::Clip,
        );
        PlayedSurface(
            StereoMode::SideBySide,
            SubspaceModifier::new()
                .size(320.0, 180.0)
                .offset(600.0, 260.0, 100.0)
                .rotate(0.0, -30.0, 0.0),
            Source::Pattern(Split::LeftRight),
        );
        PlayedSurface(
            StereoMode::TopBottom,
            SubspaceModifier::new()
                .size(320.0, 180.0)
                .offset(600.0, -260.0, 100.0)
                .rotate(0.0, -30.0, 0.0),
            Source::Pattern(Split::TopBottom),
        );
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

// What fills a surface.
#[derive(Clone, Copy, PartialEq)]
enum Source {
    // The hardware decoder plays the clip.
    Clip,
    // A thread writes the test pattern.
    Pattern(Split),
}

// The thread that fills a surface. It stops when dropped.
struct Writer {
    _clip: Option<ClipPlayer>,
    _pattern: Option<PatternWriter>,
}

impl Writer {
    fn start(surface: ExternalSurface, source: Source) -> Self {
        match source {
            Source::Clip => Self {
                _clip: Some(ClipPlayer::start(surface)),
                _pattern: None,
            },
            Source::Pattern(split) => Self {
                _clip: None,
                _pattern: Some(PatternWriter::start(surface, split)),
            },
        }
    }
}

// A surface that a thread fills from its creation to its end.
#[composable]
fn PlayedSurface(stereo_mode: StereoMode, modifier: SubspaceModifier, source: Source) {
    let writer = remember(|| Rc::new(RefCell::new(None::<Writer>))).with(Clone::clone);
    SpatialExternalSurface(stereo_mode, modifier, move |scope| {
        let started = Rc::clone(&writer);
        scope.on_surface_created(move |surface| {
            started.borrow_mut().replace(Writer::start(surface, source));
        });
        let stopped = Rc::clone(&writer);
        scope.on_surface_destroyed(move |_| {
            stopped.borrow_mut().take();
        });
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
