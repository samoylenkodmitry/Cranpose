#![allow(unsafe_code)]

use std::rc::Rc;

use cranpose_services::{UriHandler, UriHandlerError, set_platform_uri_handler};
use objc2::{MainThreadMarker, rc::Retained, runtime::AnyObject};
use objc2_foundation::{NSDictionary, NSString, NSURL};
use objc2_ui_kit::UIApplication;

pub(crate) fn register() {
    set_platform_uri_handler(Rc::new(IosUriHandler));
}

struct IosUriHandler;

impl UriHandler for IosUriHandler {
    fn open_uri(&self, uri: &str) -> Result<(), UriHandlerError> {
        let Some(mtm) = MainThreadMarker::new() else {
            return Err(UriHandlerError::OpenFailed(
                "opening a URL must run on the main thread".into(),
            ));
        };

        let string = NSString::from_str(uri);
        let url = NSURL::URLWithString(&string)
            .ok_or_else(|| UriHandlerError::OpenFailed(format!("invalid URL: {uri}")))?;

        let app = UIApplication::sharedApplication(mtm);
        let options: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::new();
        unsafe { app.openURL_options_completionHandler(&url, &options, None) };
        Ok(())
    }
}
