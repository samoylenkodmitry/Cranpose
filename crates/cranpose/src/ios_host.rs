//! iOS implementation of framework host controls.

#![allow(unsafe_code)]

use std::sync::Arc;

use cranpose_services::{
    HostController, PlatformDirectories, set_application_id, set_host_controller,
};
use objc2::MainThreadMarker;
use objc2_foundation::{NSBundle, NSHomeDirectory};
use objc2_ui_kit::UIApplication;

struct IosHost;

impl HostController for IosHost {
    fn set_keep_screen_on(&self, enabled: bool) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        UIApplication::sharedApplication(mtm).setIdleTimerDisabled(enabled);
    }

    fn platform_directories(&self) -> Option<PlatformDirectories> {
        let home = std::path::PathBuf::from(format!("{}", NSHomeDirectory()));
        Some(PlatformDirectories {
            data: home.join("Library").join("Application Support"),
            config: home.join("Library").join("Preferences"),
            cache: home.join("Library").join("Caches"),
            documents: Some(home.join("Documents")),
            temporary: std::env::temp_dir(),
            shared: None,
        })
    }

    fn exit(&self) {}

    fn background(&self) {}

    fn durable_save_deadline(&self) -> std::time::Duration {
        // iOS grants roughly five seconds when the app moves to the background
        // before it may be suspended; longer work continues under the
        // background-task lease.
        std::time::Duration::from_secs(5)
    }
}

/// The packaged bundle identifier, used to scope framework-owned storage.
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
}
