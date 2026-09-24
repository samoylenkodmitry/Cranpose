use std::{cell::RefCell, rc::Rc};

use cranpose_core::{CompositionLocal, CompositionLocalProvider, compositionLocalOfWithPolicy};
use cranpose_macros::composable;

#[derive(thiserror::Error, Debug)]
pub enum UriHandlerError {
    #[error("Failed to open URL: {0}")]
    OpenFailed(String),
    #[error("No window object available")]
    NoWindow,
    #[error("Popup blocked for URL: {0}")]
    PopupBlocked(String),
    #[error("Opening external links is not supported on this platform: {0}")]
    UnsupportedPlatform(String),
    #[error("{operation} requires cranpose-services feature `{feature}`")]
    UnsupportedFeature {
        operation: &'static str,
        feature: &'static str,
    },
}

pub trait UriHandler {
    fn open_uri(&self, uri: &str) -> Result<(), UriHandlerError>;
}

pub type UriHandlerRef = Rc<dyn UriHandler>;

thread_local! {
    static PLATFORM_URI_HANDLER: RefCell<Option<UriHandlerRef>> = const { RefCell::new(None) };
}

/// Installs a platform URI handler, replacing any previously installed one.
///
/// Registered handlers take precedence over the built-in per-platform opener,
/// so a backend with main-thread UIKit access can open links the app sandbox
/// forbids launching through a subprocess. The iOS backend registers a
/// `UIApplication`-based opener this way (see `cranpose::ios`), because
/// spawning `open`/`xdg-open` is not available inside the iOS sandbox.
pub fn set_platform_uri_handler(handler: UriHandlerRef) {
    PLATFORM_URI_HANDLER.with(|cell| *cell.borrow_mut() = Some(handler));
}

/// Removes any registered platform URI handler (tests and teardown).
pub fn clear_platform_uri_handler() {
    PLATFORM_URI_HANDLER.with(|cell| *cell.borrow_mut() = None);
}

fn registered_platform_uri_handler() -> Option<UriHandlerRef> {
    PLATFORM_URI_HANDLER.with(|cell| cell.borrow().clone())
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

pub fn default_uri_handler() -> UriHandlerRef {
    Rc::new(PlatformUriHandler)
}

pub fn local_uri_handler() -> CompositionLocal<UriHandlerRef> {
    thread_local! {
        static LOCAL_URI_HANDLER: RefCell<Option<CompositionLocal<UriHandlerRef>>> = const { RefCell::new(None) };
    }

    LOCAL_URI_HANDLER.with(|cell| {
        let mut local = cell.borrow_mut();
        local
            .get_or_insert_with(|| compositionLocalOfWithPolicy(default_uri_handler, Rc::ptr_eq))
            .clone()
    })
}

#[composable]
pub fn ProvideUriHandler(content: impl FnOnce()) {
    let uri_handler = cranpose_core::remember(default_uri_handler).with(|state| state.clone());
    let uri_local = local_uri_handler();

    CompositionLocalProvider(vec![uri_local.provides(uri_handler)], move || {
        content();
    });
}

#[cfg(test)]
#[path = "tests/uri_handler_tests.rs"]
mod tests;
