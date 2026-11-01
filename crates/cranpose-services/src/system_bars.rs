//! The color of the icons the platform draws in its status and navigation bars
//! over an app that paints its own background under them.

use std::sync::{Mutex, PoisonError, atomic::Ordering};

use cranpose_core::DisposableEffect;
use portable_atomic::AtomicU64;

use crate::host_controller;

/// The color of the icons the platform draws in its status and navigation bars
/// over the app.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SystemBarIcons {
    /// What the platform shows for the system's light or dark theme.
    #[default]
    FollowSystem,
    /// Dark icons, readable over a light background.
    Dark,
    /// Light icons, readable over a dark background.
    Light,
}

// Every active `SystemBarAppearance` with the icons it asks for, oldest first.
static REQUESTS: Mutex<Vec<(u64, SystemBarIcons)>> = Mutex::new(Vec::new());
static NEXT_REQUEST: AtomicU64 = AtomicU64::new(0);

/// The icons the most recent active [`SystemBarAppearance`] asks for, or
/// [`SystemBarIcons::FollowSystem`] when none is in composition.
pub fn system_bar_icons() -> SystemBarIcons {
    REQUESTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .last()
        .map_or(SystemBarIcons::FollowSystem, |&(_, icons)| icons)
}

pub(crate) fn show_current_system_bar_icons() {
    if let Some(host) = host_controller() {
        host.set_system_bar_icons(system_bar_icons());
    }
}

fn request(icons: SystemBarIcons) -> u64 {
    let id = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
    REQUESTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push((id, icons));
    show_current_system_bar_icons();
    id
}

fn withdraw(id: u64) {
    REQUESTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .retain(|&(request, _)| request != id);
    show_current_system_bar_icons();
}

/// Shows `icons` in the status and navigation bars while this call stays in
/// composition, for an app that paints its own background under the bars:
/// Android's light and dark system bar appearance, and the iOS status bar
/// style. The most recent active call wins; when it leaves composition the one
/// before it applies again, and with none left the bars follow the system
/// theme. Platforms without system bars over the app ignore it.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn SystemBarAppearance(icons: SystemBarIcons) {
    DisposableEffect(icons, move |scope| {
        let id = request(icons);
        scope.on_dispose(move || withdraw(id))
    });
}

#[cfg(test)]
#[path = "tests/system_bars_tests.rs"]
mod tests;
