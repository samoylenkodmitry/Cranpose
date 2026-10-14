use std::rc::Rc;

use cranpose_core::{CompositionLocal, CompositionLocalProvider, compositionLocalOfWithPolicy};
use cranpose_macros::composable;

/// Reason why [`UriHandler::open_uri`] failed.
#[derive(thiserror::Error, Debug)]
pub enum UriHandlerError {
    /// The platform failed to open the URL. The text describes the cause.
    #[error("Failed to open URL: {0}")]
    OpenFailed(String),
    /// There is no browser window object to open the URL in.
    #[error("No window object available")]
    NoWindow,
    /// The browser blocked the new window for the URL.
    #[error("Popup blocked for URL: {0}")]
    PopupBlocked(String),
    /// The platform has no way to open external links.
    #[error("Opening external links is not supported on this platform: {0}")]
    UnsupportedPlatform(String),
    /// The operation needs a `cranpose-services` feature that is turned off.
    #[error("{operation} requires cranpose-services feature `{feature}`")]
    UnsupportedFeature {
        /// The operation that the build cannot run.
        operation: &'static str,
        /// Name of the `cranpose-services` feature that the operation needs.
        feature: &'static str,
    },
}

/// Opens a URI, such as a web link, in the application the platform picks for it.
pub trait UriHandler {
    /// Opens `uri` outside the app, for example in the system browser. Returns an error when the
    /// platform cannot open it.
    fn open_uri(&self, uri: &str) -> Result<(), UriHandlerError>;
}

/// Shared handle to a [`UriHandler`].
pub type UriHandlerRef = Rc<dyn UriHandler>;

/// Installs a platform URI handler, replacing any previously installed one.
///
/// Registered handlers take precedence over the built-in per-platform opener,
/// so a backend with main-thread UIKit access can open links the app sandbox
/// forbids launching through a subprocess. The iOS backend registers a
/// `UIApplication`-based opener this way (see `cranpose::ios`), because
/// spawning `open`/`xdg-open` is not available inside the iOS sandbox.
pub fn set_platform_uri_handler(handler: UriHandlerRef) {
    crate::thread_state::with(|state| *state.uri_handler.borrow_mut() = Some(handler));
}

/// Removes any registered platform URI handler (tests and teardown).
pub fn clear_platform_uri_handler() {
    crate::thread_state::with(|state| *state.uri_handler.borrow_mut() = None);
}

fn registered_platform_uri_handler() -> Option<UriHandlerRef> {
    crate::thread_state::with(|state| state.uri_handler.borrow().clone())
}

struct PlatformUriHandler;

impl UriHandler for PlatformUriHandler {
    fn open_uri(&self, uri: &str) -> Result<(), UriHandlerError> {
        if let Some(handler) = registered_platform_uri_handler() {
            return handler.open_uri(uri);
        }

        #[cfg(all(
            not(target_arch = "wasm32"),
            not(target_os = "android"),
            feature = "uri-native"
        ))]
        {
            open::that(uri).map_err(|err| UriHandlerError::OpenFailed(format!("{err:?}")))?;
            Ok(())
        }

        #[cfg(all(target_arch = "wasm32", feature = "uri-web"))]
        {
            let window = web_sys::window().ok_or(UriHandlerError::NoWindow)?;
            let opened = window
                .open_with_url_and_target(uri, "_blank")
                .map_err(|err| UriHandlerError::OpenFailed(format!("{err:?}")))?;
            if opened.is_none() {
                Err(UriHandlerError::PopupBlocked(uri.to_string()))
            } else {
                Ok(())
            }
        }

        #[cfg(all(target_os = "android", feature = "uri-android"))]
        {
            webbrowser::open(uri).map_err(|err| UriHandlerError::OpenFailed(err.to_string()))?;
            Ok(())
        }

        #[cfg(all(
            not(target_arch = "wasm32"),
            not(target_os = "android"),
            not(feature = "uri-native")
        ))]
        {
            let _ = uri;
            Err(UriHandlerError::UnsupportedFeature {
                operation: "native URI opening",
                feature: "uri-native",
            })
        }

        #[cfg(all(target_arch = "wasm32", not(feature = "uri-web")))]
        {
            let _ = uri;
            Err(UriHandlerError::UnsupportedFeature {
                operation: "web URI opening",
                feature: "uri-web",
            })
        }

        #[cfg(all(target_os = "android", not(feature = "uri-android")))]
        {
            let _ = uri;
            Err(UriHandlerError::UnsupportedFeature {
                operation: "Android URI opening",
                feature: "uri-android",
            })
        }
    }
}

/// Returns the handler that opens a URI through a registered platform handler or else the
/// built-in opener of the current platform.
pub fn default_uri_handler() -> UriHandlerRef {
    Rc::new(PlatformUriHandler)
}

/// The URI handler a composable reads. It is the platform handler unless a composition local
/// provider above it supplies another one.
pub fn local_uri_handler() -> CompositionLocal<UriHandlerRef> {
    crate::composition_locals::cached_local(
        |locals| &locals.uri_handler,
        || compositionLocalOfWithPolicy(default_uri_handler, Rc::ptr_eq),
    )
}

/// Provides the platform URI handler to `content` through [`local_uri_handler`].
#[composable]
pub fn ProvideUriHandler(content: impl FnOnce()) {
    let uri_handler = cranpose_core::remember(default_uri_handler).with(|state| state.clone());
    let uri_local = local_uri_handler();

    CompositionLocalProvider([uri_local.provides(uri_handler)], move || {
        content();
    });
}

#[cfg(test)]
#[path = "tests/uri_handler_tests.rs"]
mod tests;
