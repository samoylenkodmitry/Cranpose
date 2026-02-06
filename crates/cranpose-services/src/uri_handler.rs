use cranpose_core::compositionLocalOf;
use cranpose_core::CompositionLocal;
use cranpose_core::CompositionLocalProvider;
use cranpose_macros::composable;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_test_composition;
    use cranpose_core::CompositionLocalProvider;
    use std::cell::RefCell;

    struct TestUriHandler;

    impl UriHandler for TestUriHandler {
        fn open_uri(&self, _uri: &str) -> Result<(), UriHandlerError> {
            Ok(())
        }
    }

    #[test]
    fn default_uri_handler_can_be_created() {
        let handler = default_uri_handler();
        assert_eq!(Rc::strong_count(&handler), 1);
    }

    #[test]
    fn local_uri_handler_can_be_overridden() {
        let local = local_uri_handler();
        let default_handler = default_uri_handler();
        let custom_handler: UriHandlerRef = Rc::new(TestUriHandler);
        let captured = Rc::new(RefCell::new(None));

        {
            let captured = Rc::clone(&captured);
            let custom_handler = custom_handler.clone();
            let local_for_provider = local.clone();
            let local_for_read = local.clone();
            run_test_composition(move || {
                let captured = Rc::clone(&captured);
                let custom_handler = custom_handler.clone();
                let local_for_read = local_for_read.clone();
                CompositionLocalProvider(
                    vec![local_for_provider.provides(custom_handler)],
                    move || {
                        let current = local_for_read.current();
                        *captured.borrow_mut() = Some(current);
                    },
                );
            });
        }

        let current = captured
            .borrow()
            .as_ref()
            .expect("handler captured")
            .clone();
        assert!(Rc::ptr_eq(&current, &custom_handler));
        assert!(!Rc::ptr_eq(&current, &default_handler));
    }

    #[test]
    fn provide_uri_handler_sets_current_handler() {
        let local = local_uri_handler();
        let captured = Rc::new(RefCell::new(None));

        {
            let captured = Rc::clone(&captured);
            let local = local.clone();
            run_test_composition(move || {
                let captured = Rc::clone(&captured);
                let local = local.clone();
                ProvideUriHandler(move || {
                    let current = local.current();
                    *captured.borrow_mut() = Some(current);
                });
            });
        }

        assert!(captured.borrow().is_some());
    }
}
