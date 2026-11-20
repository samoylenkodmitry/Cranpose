//! The desktop process that hosts the session, and the entry of an app that uses a headset where
//! the machine has one.

use std::{thread, time::Duration};

use cranpose::{AppLauncher, GuestView, LauncherFonts};
use openxr as xr;

use crate::{
    RunError,
    entitlement_watch::Answer,
    openxr::{entitlement_run::EntitlementRun, runner::run_immersive, runtime::Runtime},
    session_status::SessionStatus,
    spatial_request::SpatialRequest,
    tracking_permission::PermissionAnswer,
    tracking_request::TrackingRequest,
};

// How long the process waits between looks at the runtime while the headset shows something else.
const IDLE_WAIT: Duration = Duration::from_millis(100);

// The desktop process. A desktop session has no activity that could end.
pub(crate) struct Host;

// A desktop runtime shows no keyboard of the system over the app, so no value of this type exists.
pub(crate) enum SystemKeyboard {}

impl SystemKeyboard {
    pub(crate) fn install(&self, _view: &mut GuestView) {
        match *self {}
    }

    pub(crate) fn pump(&self, _view: &mut GuestView) {
        match *self {}
    }
}

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

    // A desktop asks the user for nothing: its runtime decides.
    pub(crate) fn permission_answer(&self, _alternatives: &[&str]) -> PermissionAnswer {
        PermissionAnswer::Granted
    }

    // A desktop shows no prompt.
    pub(crate) fn request_permissions(&self, _permissions: &[&str]) {}

    // Readies the process for the runtime that offers `available`: nothing, and it can create
    // its instance.
    pub(crate) fn await_activity(&self, _available: &xr::ExtensionSet) -> bool {
        true
    }

    // A desktop runtime has no keyboard of the system for the app's text fields: the app draws its
    // own.
    pub(crate) fn system_keyboard(&self) -> Option<SystemKeyboard> {
        None
    }

    // Ends the process, as the app asked. Its run returns when the session ended, so there is
    // nothing more to end.
    pub(crate) fn finish(&self) -> bool {
        true
    }

    // A desktop has no store that checks the app: no check runs.
    pub(crate) fn begin_entitlement(&self) -> bool {
        false
    }

    pub(crate) fn entitlement_answer(&self) -> Answer {
        Answer::Checking
    }

    pub(crate) fn entitlement_detail(&self) -> String {
        String::new()
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
    let request = TrackingRequest::of(launcher.capabilities());
    let spatial_request = SpatialRequest::of(launcher.capabilities());
    match Runtime::connect(&host, request, spatial_request) {
        Ok(runtime) => {
            let settings = launcher.into_settings();
            let entitlement = EntitlementRun::start(&host);
            run_immersive(&host, runtime, &settings, entitlement, content)
                .map_err(RunError::Headset)
        }
        Err(reason) => {
            log::info!("cranpose xr: {reason}; the flat window opens");
            launcher.try_run(content).map_err(RunError::Window)
        }
    }
}
