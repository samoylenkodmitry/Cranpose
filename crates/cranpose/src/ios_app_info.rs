//! Reads the version identity from the running iOS application bundle.

use cranpose_services::{set_platform_app_info, AppInfo};
use objc2_foundation::{ns_string, NSBundle, NSString};
use std::rc::Rc;

struct IosAppInfo {
    version_name: Option<String>,
    build_version: Option<String>,
}

impl AppInfo for IosAppInfo {
    fn version_name(&self) -> Option<String> {
        self.version_name.clone()
    }

    fn build_version(&self) -> Option<String> {
        self.build_version.clone()
    }
}

fn bundle_string(key: &NSString) -> Option<String> {
    NSBundle::mainBundle()
        .objectForInfoDictionaryKey(key)?
        .downcast::<NSString>()
        .ok()
        .map(|value| value.to_string())
}

/// Installs the immutable packaged identity of the running application.
pub(crate) fn register() {
    set_platform_app_info(Rc::new(IosAppInfo {
        version_name: bundle_string(ns_string!("CFBundleShortVersionString")),
        build_version: bundle_string(ns_string!("CFBundleVersion")),
    }));
}
