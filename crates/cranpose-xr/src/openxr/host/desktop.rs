//! The desktop process that hosts the session, and the entry of an app that uses a headset where
//! the machine has one.

use std::{thread, time::Duration};

use cranpose::{AppLauncher, LauncherFonts};
use openxr as xr;

use crate::{
    RunError,
    openxr::{runner::run_immersive, runtime::Runtime, session::SessionStatus},
};

// How long the process waits between looks at the runtime while the headset shows something else.
const IDLE_WAIT: Duration = Duration::from_millis(100);

// The desktop process. A desktop session has no activity that could end.
pub(crate) struct Host;

impl Host {
    // What the loader and the instance need of the platform: nothing.
    pub(crate) fn platform(&self) {}

    // Adds the extensions that the platform needs: none.
    pub(crate) fn require(
        &self,
        _available: &xr::ExtensionSet,
        _extensions: &mut xr::ExtensionSet,
    ) {
    }

    // Waits while the headset shows something else. The session alone ends a desktop run.
    pub(crate) fn ended(&self, status: SessionStatus) -> bool {
        if !status.runs() {
            // No frame waits for this: the session has none to draw.
            thread::park_timeout(IDLE_WAIT);
        }
        false
    }
}

/// Runs `content` as an immersive app on the headset of the machine's OpenXR runtime when it has
/// one: the panels of its subspaces stand in the room and the controllers point at them. When the
/// machine has no OpenXR loader, no runtime or no headset, it opens the flat window of
/// [`AppLauncher::try_run`] instead, which draws the panels side by side.
///
/// It takes the arguments of [`android_main!`](crate::android_main) on Android, so an app has one launcher and one
/// content for both. It returns when the headset ends the session or the window closes.
///
/// # Errors
///
/// Returns [`RunError::Headset`] when a headset was found and its session failed. Returns
/// [`RunError::Window`] when the flat window failed to open.
pub fn run<Fonts: LauncherFonts>(
    launcher: AppLauncher<Fonts>,
    content: impl FnMut() + 'static,
) -> Result<(), RunError> {
    let host = Host;
    match Runtime::connect(&host) {
        Ok(runtime) => {
            let settings = launcher.into_settings();
            run_immersive(&host, runtime, &settings, content).map_err(RunError::Headset)
        }
        Err(reason) => {
            log::info!("cranpose xr: {reason}; the flat window opens");
            launcher.try_run(content).map_err(RunError::Window)
        }
    }
}
