use std::{path::PathBuf, sync::Arc};

use super::*;
use crate::{HostController, PlatformDirectories, clear_host_controller, set_host_controller};

#[derive(Default)]
struct RecordingHost(Mutex<Vec<SystemBarIcons>>);

impl RecordingHost {
    fn shown(&self) -> Vec<SystemBarIcons> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl HostController for RecordingHost {
    fn set_keep_screen_on(&self, _enabled: bool) {}
    fn set_system_bar_icons(&self, icons: SystemBarIcons) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(icons);
    }
    fn platform_directories(&self) -> Option<PlatformDirectories> {
        Some(PlatformDirectories {
            data: PathBuf::from("data"),
            config: PathBuf::from("config"),
            cache: PathBuf::from("cache"),
            documents: None,
            temporary: PathBuf::from("temporary"),
            shared: None,
        })
    }
    fn exit(&self) {}
    fn background(&self) {}
}

#[test]
fn the_latest_request_wins_and_the_one_before_returns_when_it_goes() {
    let _guard = crate::host::tests::test_lock();
    let host = Arc::new(RecordingHost::default());
    set_host_controller(Arc::clone(&host) as _);

    let dark = request(SystemBarIcons::Dark);
    let light = request(SystemBarIcons::Light);
    assert_eq!(system_bar_icons(), SystemBarIcons::Light);
    withdraw(light);
    withdraw(dark);

    assert_eq!(system_bar_icons(), SystemBarIcons::FollowSystem);
    assert_eq!(
        host.shown(),
        [
            SystemBarIcons::FollowSystem,
            SystemBarIcons::Dark,
            SystemBarIcons::Light,
            SystemBarIcons::Dark,
            SystemBarIcons::FollowSystem,
        ]
    );
    clear_host_controller();
}

#[test]
fn withdrawing_an_older_request_keeps_the_latest() {
    let _guard = crate::host::tests::test_lock();
    let dark = request(SystemBarIcons::Dark);
    let light = request(SystemBarIcons::Light);

    withdraw(dark);
    assert_eq!(system_bar_icons(), SystemBarIcons::Light);
    withdraw(light);
    assert_eq!(system_bar_icons(), SystemBarIcons::FollowSystem);
}

#[test]
fn a_new_host_is_shown_the_icons_already_asked_for() {
    let _guard = crate::host::tests::test_lock();
    let dark = request(SystemBarIcons::Dark);
    let host = Arc::new(RecordingHost::default());

    set_host_controller(Arc::clone(&host) as _);

    assert_eq!(host.shown(), [SystemBarIcons::Dark]);
    withdraw(dark);
    clear_host_controller();
}
