//! The gauntlet in zgui: the screen `compose-app/.../Gauntlet.kt` draws, in
//! zgui's own idiom: elements built once whose styles, text and canvases
//! follow signals, under a style sheet. The benchmark's README describes it;
//! `desktop.py` runs it: `PERF_TIER=16 PERF_FONTS=../fonts cargo run --release`.

mod chat;
mod gauntlet;
mod patchwork;
mod picture;
mod sheet;
mod wall;

use std::{cell::RefCell, rc::Rc, sync::Arc};

use perf_data::*;
use zgui::{
    prelude::*,
    view::{AnyView, FrameHandle, Timers},
};

/// Roboto from the files every desktop app loads, and the chat's monospaced
/// face for the patchwork; no other face.
fn fonts(scenario: Scenario) -> Fonts {
    let fonts = Fonts::shipped_only();
    let mut files = vec![
        ("Roboto-Regular.ttf", "Roboto"),
        ("Roboto-Bold.ttf", "Roboto"),
    ];
    if scenario == Scenario::Patchwork {
        files.push(("DejaVuSansMono.ttf", "DejaVu Sans Mono"));
    }
    for (file, family) in files {
        match font_bytes(file) {
            Ok(bytes) => {
                if let Err(error) = fonts.register(Arc::new(bytes), Some(family)) {
                    log::warn!("{file} is no font: {error:?}");
                }
            }
            Err(error) => log::warn!("no {file}: {error}"),
        }
    }
    fonts
}

/// What runs once per animation frame.
struct Frames {
    timers: Timers,
    handle: RefCell<Option<FrameHandle>>,
    step: Box<dyn Fn() -> bool>,
}

/// Calls `step` once per animation frame until it answers `false`. Must run
/// inside a window's scope.
pub(crate) fn run_frames(step: impl Fn() -> bool + 'static) {
    let frames = Rc::new(Frames {
        timers: Timers::current().expect("a window's scope"),
        handle: RefCell::new(None),
        step: Box::new(step),
    });
    arm(frames);
}

fn arm(frames: Rc<Frames>) {
    let next = frames.clone();
    let handle = frames.timers.request_frame(move |_| {
        if (next.step)() {
            arm(next);
        } else {
            next.handle.borrow_mut().take();
        }
    });
    *frames.handle.borrow_mut() = Some(handle);
}

fn main() -> Result<(), zgui::Error> {
    perf_data::log_to_stdout();
    let launch = perf_data::launch();
    let (width, height) = DESKTOP_WINDOW;
    let (sheet, screen): (String, fn() -> AnyView) = match launch.scenario {
        Scenario::Gauntlet => (
            sheet::gauntlet(desktop_gauntlet().tier.scale),
            gauntlet::screen,
        ),
        Scenario::Wall => (sheet::wall(), wall::screen),
        Scenario::Patchwork => (sheet::patchwork(), patchwork::screen),
    };
    app()
        .with_title(launch.scenario.title())
        .with_size(width as f32, height as f32)
        .with_resizable(false)
        .with_fonts(fonts(launch.scenario))
        .with_stylesheet(sheet)
        .run(screen)
}
