use anyhow::Result;
use cranpose_core::{compositionLocalOf, CompositionLocal};
use std::cell::RefCell;
use std::rc::Rc;

pub(crate) trait UriHandler {
    fn open_uri(&self, uri: &str) -> Result<()>;
}

pub(crate) type UriHandlerRef = Rc<dyn UriHandler>;

struct PlatformUriHandler;

impl UriHandler for PlatformUriHandler {
    fn open_uri(&self, uri: &str) -> Result<()> {
        #[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
        {
            use anyhow::Context;

            open::that(uri).with_context(|| format!("Failed to open URL: {}", uri))?;
            Ok(())
        }

        #[cfg(target_arch = "wasm32")]
        {
            let window =
                web_sys::window().ok_or_else(|| anyhow::anyhow!("No window object available"))?;
            let opened = window
                .open_with_url_and_target(uri, "_blank")
                .map_err(|err| anyhow::anyhow!("window.open failed: {:?}", err))?;
            if opened.is_none() {
                Err(anyhow::anyhow!("Popup blocked for URL: {}", uri))
            } else {
                Ok(())
            }
        }

        #[cfg(target_os = "android")]
        {
            Err(anyhow::anyhow!(
                "Opening external links is not supported on Android yet (url: {})",
                uri
            ))
        }
    }
}

pub(crate) fn default_uri_handler() -> UriHandlerRef {
    Rc::new(PlatformUriHandler)
}

pub(crate) fn local_uri_handler() -> CompositionLocal<UriHandlerRef> {
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
