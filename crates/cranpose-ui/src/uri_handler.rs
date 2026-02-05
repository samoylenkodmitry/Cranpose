use crate::composable;
use cranpose_core::compositionLocalOf;
use cranpose_core::CompositionLocal;
use cranpose_core::CompositionLocalProvider;
use std::cell::RefCell;
use std::rc::Rc;

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
}

pub trait UriHandler {
    fn open_uri(&self, uri: &str) -> Result<(), UriHandlerError>;
}

pub type UriHandlerRef = Rc<dyn UriHandler>;

struct PlatformUriHandler;

impl UriHandler for PlatformUriHandler {
    fn open_uri(&self, uri: &str) -> Result<(), UriHandlerError> {
        #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
        {
            open::that(uri).map_err(|err| UriHandlerError::OpenFailed(format!("{:?}", err)))?;
            Ok(())
        }

        #[cfg(target_arch = "wasm32")]
        {
            let window = web_sys::window().ok_or(UriHandlerError::NoWindow)?;
            let opened = window
                .open_with_url_and_target(uri, "_blank")
                .map_err(|err| UriHandlerError::OpenFailed(format!("{:?}", err)))?;
            if opened.is_none() {
                Err(UriHandlerError::PopupBlocked(uri.to_string()))
            } else {
                Ok(())
            }
        }

        #[cfg(target_os = "android")]
        {
            Err(UriHandlerError::UnsupportedPlatform(uri.to_string()))
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
        if local.is_none() {
            *local = Some(compositionLocalOf(default_uri_handler));
        }
        local
            .as_ref()
            .expect("Uri handler composition local must be initialized")
            .clone()
    })
}

#[allow(non_snake_case)]
#[composable]
pub fn ProvideUriHandler(content: impl FnOnce()) {
    let uri_handler = cranpose_core::remember(default_uri_handler).with(|state| state.clone());
    let uri_local = local_uri_handler();

    CompositionLocalProvider(vec![uri_local.provides(uri_handler)], move || {
        content();
    });
}
