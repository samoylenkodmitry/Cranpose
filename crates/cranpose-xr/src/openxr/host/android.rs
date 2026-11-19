//! The Android activity that hosts the session, and the entry of an immersive Android app.
#![expect(
    unsafe_code,
    reason = "the platform info holds raw pointers to the Java VM and the activity"
)]

use std::time::{Duration, Instant};

use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};
use cranpose::{AppLauncher, LauncherFonts};
use openxr as xr;

use crate::openxr::{runner::run_immersive, runtime::Runtime, session::SessionStatus};

// The most events that one pass of the loop takes from the activity's queue
const MAX_INPUT_EVENTS_PER_LOOP: usize = 16;

// How long one look at the activity's events lasts while the app waits for the activity, and how
// long the wait goes on before the log says so.
const RESUME_POLL: Duration = Duration::from_millis(20);
const RESUME_NOTICE: Duration = Duration::from_secs(5);

// The start of the extension names that only Android XR's runtime offers.
const ANDROID_XR_EXTENSION: &[u8] = b"XR_ANDROID_";

// The activity that the session belongs to.
pub(crate) struct Host {
    app: AndroidApp,
}

impl Host {
    // What the loader and the instance need of the platform. The runtime finds the app's window
    // through the activity it is given. The process's NDK context can hold the application
    // context instead, and with it the runtime shows none of the app's frames.
    pub(crate) fn platform(&self) -> xr::AndroidPlatformInfo {
        // SAFETY: the Java VM and the global reference to the activity live as long as the
        // AndroidApp, which outlives the instance made with this info.
        unsafe { xr::AndroidPlatformInfo::new(self.app.vm_as_ptr(), self.app.activity_as_ptr()) }
    }

    // Adds the extension that creates an instance for an activity, and the one that gives a
    // decoder or a camera a surface that the compositor shows with no copy.
    pub(crate) fn require(&self, available: &xr::ExtensionSet, extensions: &mut xr::ExtensionSet) {
        extensions.khr_android_create_instance = true;
        extensions.khr_android_surface_swapchain = available.khr_android_surface_swapchain;
    }

    // Readies the activity for the runtime that offers `available`, and returns whether the app
    // can create its instance: `false` when the activity ended first. Android XR's runtime waits
    // for a resumed activity with a window inside `xrCreateInstance`. It reads the commands of
    // the C glue (`android_native_app_glue`) on the calling thread's looper, where this app's
    // glue, android-activity, keeps its own. The runtime never reads them and spins. So the
    // app takes the activity's events until the activity is resumed and has a window, and the
    // runtime finds it ready. Other runtimes do not wait for the activity, and the app starts
    // as it always did.
    pub(crate) fn await_activity(&self, available: &xr::ExtensionSet) -> bool {
        let android_xr = available
            .names()
            .iter()
            .any(|name| name.starts_with(ANDROID_XR_EXTENSION));
        !android_xr || self.wait_until_resumed()
    }

    // Takes the activity's events until it is resumed and has a window, and returns whether it
    // got there: `false` when the activity ended first.
    fn wait_until_resumed(&self) -> bool {
        let start = Instant::now();
        let mut noticed = false;
        let mut resumed = false;
        loop {
            let mut destroyed = false;
            self.app
                .poll_events(Some(RESUME_POLL), |event| match event {
                    PollEvent::Main(MainEvent::Resume { .. }) => resumed = true,
                    PollEvent::Main(MainEvent::Pause) => resumed = false,
                    PollEvent::Main(MainEvent::Destroy) => destroyed = true,
                    _ => {}
                });
            self.leave_input_unhandled();
            if destroyed {
                return false;
            }
            if resumed && self.app.native_window().is_some() {
                return true;
            }
            if !noticed && start.elapsed() > RESUME_NOTICE {
                noticed = true;
                log::warn!("cranpose xr: the activity is not resumed with a window yet");
            }
        }
    }

    // Handles the activity's events and returns whether the activity ended. While the headset
    // shows something else, it waits for them.
    pub(crate) fn ended(&self, status: SessionStatus) -> bool {
        let timeout = if status.runs() {
            Duration::ZERO
        } else {
            Duration::from_millis(100)
        };
        let mut destroyed = false;
        self.app.poll_events(Some(timeout), |event| {
            if let PollEvent::Main(MainEvent::Destroy) = event {
                destroyed = true;
            }
        });
        self.leave_input_unhandled();
        destroyed
    }

    // Ends the activity, as the app asked. Returns whether the call landed.
    pub(crate) fn finish(&self) -> bool {
        cranpose::finish_activity(&self.app)
    }

    // Takes the activity's queued key and touch events and leaves them unhandled. The controllers
    // reach the app through OpenXR. An event that nobody takes from the queue stops the activity
    // from answering Android, which shows an "app is not responding" dialog five seconds later.
    fn leave_input_unhandled(&self) {
        let Ok(mut events) = self.app.input_events_iter() else {
            return;
        };
        for _ in 0..MAX_INPUT_EVENTS_PER_LOOP {
            if !events.next(|_| InputStatus::Unhandled) {
                break;
            }
        }
    }
}

/// Runs `content` as an immersive app on the headset: its subspaces' panels stand in the room
/// and the controllers point at them. Returns when the headset ends the session or the activity
/// ends.
pub fn run<Fonts: LauncherFonts>(
    launcher: AppLauncher<Fonts>,
    app: AndroidApp,
    content: impl FnMut() + 'static,
) {
    let settings = launcher.start_android_host(&app);
    let host = Host { app };
    let result = Runtime::connect(&host)
        .and_then(|runtime| run_immersive(&host, runtime, &settings, content));
    if let Err(error) = result {
        log::error!("cranpose xr: {error}");
    }
}
