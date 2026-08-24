//! iOS URI opener built on `UIApplication.openURL`.
//!
//! Registered as the platform URI handler (see
//! [`cranpose_services::set_platform_uri_handler`]) by the iOS backend, which
//! runs on the UIKit main thread. The generic `open`/`xdg-open` path in
//! `cranpose-services` cannot work inside the iOS sandbox (spawning a helper
//! process is not permitted), so links are opened through `UIApplication`
//! instead.
#![allow(unsafe_code)]

use std::rc::Rc;

use cranpose_services::{set_platform_uri_handler, UriHandler, UriHandlerError};
use objc2::{rc::Retained, runtime::AnyObject, MainThreadMarker};
use objc2_foundation::{NSDictionary, NSString, NSURL};
use objc2_ui_kit::UIApplication;

/// Installs the iOS opener as the platform URI handler.
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
        // `URLWithString:` returns nil for an unparsable URL, handled below.
        let url = NSURL::URLWithString(&string)
            .ok_or_else(|| UriHandlerError::OpenFailed(format!("invalid URL: {uri}")))?;

        let app = UIApplication::sharedApplication(mtm);
        let options: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::new();
        // SAFETY: opening a URL with empty options and no completion handler on
        // the main thread; the system routes it to the owning app (Safari for
        // http/https).
        unsafe { app.openURL_options_completionHandler(&url, &options, None) };
        Ok(())
    }
}
