//! The Android activity that hosts the session, and the entry of an immersive Android app.
#![expect(
    unsafe_code,
    reason = "the platform info holds raw pointers to the Java VM and the activity"
)]

use std::time::Duration;

use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};
use cranpose::{AppLauncher, LauncherFonts};
use openxr as xr;

use crate::openxr::{runner::run_immersive, runtime::Runtime, session::SessionStatus};

// The most events that one pass of the loop takes from the activity's queue
const MAX_INPUT_EVENTS_PER_LOOP: usize = 16;

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
