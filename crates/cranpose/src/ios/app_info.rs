use std::rc::Rc;

use cranpose_services::{AppInfo, set_platform_app_info};
use objc2_foundation::{NSBundle, NSString, ns_string};

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

pub(crate) fn register() {
    set_platform_app_info(Rc::new(IosAppInfo {
        version_name: bundle_string(ns_string!("CFBundleShortVersionString")),
        build_version: bundle_string(ns_string!("CFBundleVersion")),
    }));
}
