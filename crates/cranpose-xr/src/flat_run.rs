//! The entry of a desktop that has no headset runtime.

use cranpose::{AppLauncher, LauncherFonts};

use crate::RunError;

/// Runs `content` in the flat window of [`AppLauncher::try_run`], which draws the panels side by
/// side, because this platform has no OpenXR headset runtime. It takes the arguments of the
/// `run` of the platforms that have one, so an app has one entry.
///
/// # Errors
///
/// Returns [`RunError::Window`] when the window failed to open.
pub fn run<Fonts: LauncherFonts>(
    launcher: AppLauncher<Fonts>,
    content: impl FnMut() + 'static,
) -> Result<(), RunError> {
    launcher.try_run(content).map_err(RunError::Window)
}
