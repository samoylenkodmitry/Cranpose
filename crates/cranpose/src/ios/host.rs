use std::sync::Arc;

use cranpose_services::{
    HostController, PlatformDirectories, SystemBarIcons, set_application_id, set_host_controller,
    system_bar_icons,
};
use objc2::MainThreadMarker;
use objc2_foundation::NSBundle;
use objc2_ui_kit::UIApplication;
use winit::{
    platform::ios::{StatusBarStyle, WindowExtIOS},
    window::Window,
};

struct IosHost;

impl HostController for IosHost {
    fn set_keep_screen_on(&self, enabled: bool) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        UIApplication::sharedApplication(mtm).setIdleTimerDisabled(enabled);
    }

    fn set_system_bar_icons(&self, icons: SystemBarIcons) {
        crate::thread_state::with(|state| {
            if let Some(window) = state.ios.winit_window.borrow().as_ref() {
                show_system_bar_icons(window.as_ref(), icons);
            }
        });
    }

    fn platform_directories(&self) -> Option<PlatformDirectories> {
        Some(crate::apple::mobile::sandbox_directories())
    }

    fn exit(&self) {}

    fn background(&self) {}

    fn durable_save_deadline(&self) -> std::time::Duration {
        std::time::Duration::from_secs(5)
    }
}

fn show_system_bar_icons(window: &dyn Window, icons: SystemBarIcons) {
    window.set_preferred_status_bar_style(match icons {
        SystemBarIcons::FollowSystem => StatusBarStyle::Default,
        SystemBarIcons::Dark => StatusBarStyle::DarkContent,
        SystemBarIcons::Light => StatusBarStyle::LightContent,
    });
}

// Keeps the app's window, which shows the system bar icons asked for so far.
pub(crate) fn hold_window(window: Arc<dyn Window>) {
    show_system_bar_icons(window.as_ref(), system_bar_icons());
    crate::thread_state::with(|state| *state.ios.winit_window.borrow_mut() = Some(window));
}

fn bundle_identifier() -> Option<String> {
    let bundle = NSBundle::mainBundle();
    bundle
        .bundleIdentifier()
        .map(|identifier| identifier.to_string())
}

pub(crate) fn register() {
    if let Some(identifier) = bundle_identifier()
        && let Err(error) = set_application_id(&identifier)
    {
        log::warn!("cranpose: the iOS bundle identifier is not a usable application id: {error}");
    }
    set_host_controller(Arc::new(IosHost));
    crate::pipeline_cache_file::publish();
}
