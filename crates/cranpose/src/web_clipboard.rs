//! The browser's clipboard, bridged to the in-tree selection menu.
//!
//! The menu's Copy / Cut / Paste live in the widget tree and reach the system
//! clipboard through
//! [`PlatformClipboard`](cranpose_ui::clipboard_session::PlatformClipboard).
//! Without a bridge installed they fall back to an in-process clipboard, so a
//! copy on the web page could only ever be pasted back into the same page.
//!
//! The two directions are not symmetric, because the Async Clipboard API is
//! not:
//!
//! * **Writing** is fire-and-forget. `navigator.clipboard.writeText` returns a
//!   promise nobody has to wait on, and it succeeds while the page holds a
//!   transient user activation — which a menu tap is.
//! * **Reading** cannot be answered on the call stack that asks. `readText`
//!   returns a promise and, outside Chromium, prompts the user. So this bridge
//!   reports nothing readable and instead takes the *paste itself*
//!   ([`request_paste`](cranpose_ui::clipboard_session::PlatformClipboard::request_paste)),
//!   completing it through the shell's ordinary paste path once the promise
//!   resolves. `readText` is still called synchronously, inside the tap, so the
//!   activation is unambiguous; only the await is deferred.
//!
//! On a page with no clipboard at all — an insecure origin has no
//! `navigator.clipboard` — this bridge declines both directions, and the
//! session's in-process clipboard keeps copy and paste working inside the page.

use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;
use cranpose_ui::clipboard_session::PlatformClipboard;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::{JsFuture, spawn_local};

/// Installs the browser clipboard for `app`'s context.
///
/// Holds the shell weakly: the clipboard lives in the app context, which the
/// shell owns, so a strong handle would be a cycle that never drops.
pub(crate) fn install(app: &Rc<RefCell<AppShell<WgpuRenderer>>>, request_frame: Rc<dyn Fn()>) {
    let clipboard = Rc::new(WebClipboard {
        app: Rc::downgrade(app),
        request_frame,
    });
    app.borrow().app_context().enter(move || {
        cranpose_ui::clipboard_session::set_platform_clipboard(clipboard);
    });
}

struct WebClipboard {
    app: Weak<RefCell<AppShell<WgpuRenderer>>>,
    request_frame: Rc<dyn Fn()>,
}

impl WebClipboard {
    /// The DOM clipboard, or `None` on a page that has none — an insecure
    /// origin (plain `http://` to anything but localhost) has no
    /// `navigator.clipboard` at all.
    fn dom_clipboard() -> Option<web_sys::Clipboard> {
        let clipboard = web_sys::window()?.navigator().clipboard();
        (!JsValue::from(clipboard.clone()).is_undefined()).then_some(clipboard)
    }
}

impl PlatformClipboard for WebClipboard {
    fn write_text(&self, text: &str) {
        let Some(clipboard) = Self::dom_clipboard() else {
            log::warn!("browser clipboard unavailable (insecure origin?); copy stayed in-page");
            return;
        };
        // Resolving the promise is the browser's business: there is nothing to
        // report back to a menu that has already closed, and dropping it would
        // cancel the write.
        let promise = clipboard.write_text(text);
        spawn_local(async move {
            if let Err(error) = JsFuture::from(promise).await {
                log::warn!("browser clipboard write failed: {error:?}");
            }
        });
    }

    fn read_text(&self) -> Option<String> {
        // Deliberately not a `readText()`: that is a promise, and this call
        // wants an answer now. Nothing about the system clipboard is knowable
        // synchronously here, so the session's in-process copy answers reads
        // inside the page and `request_paste` answers everything else.
        None
    }

    fn can_request_paste(&self) -> bool {
        Self::dom_clipboard().is_some()
    }

    fn request_paste(&self) -> bool {
        let Some(clipboard) = Self::dom_clipboard() else {
            return false;
        };
        // Called inside the tap that asked to paste, so the page's transient
        // user activation still covers it; only the await is deferred.
        let promise = clipboard.read_text();
        let app = self.app.clone();
        let request_frame = Rc::clone(&self.request_frame);
        spawn_local(async move {
            let text = match JsFuture::from(promise).await {
                Ok(value) => value.as_string().unwrap_or_default(),
                Err(error) => {
                    // A denied permission prompt lands here; the user declined
                    // the paste, which is not the app's problem to report.
                    log::info!("browser clipboard read declined: {error:?}");
                    return;
                }
            };
            if text.is_empty() {
                return;
            }
            let Some(shell) = app.upgrade() else {
                return;
            };
            // The tap that asked for this is long finished, so the shell is no
            // longer borrowed -- but a frame could be in flight, and a paste is
            // worth dropping rather than panicking over.
            let pasted = match shell.try_borrow_mut() {
                Ok(mut shell) => {
                    shell.on_paste(&text);
                    true
                }
                Err(_) => false,
            };
            if pasted {
                request_frame();
            }
        });
        true
    }
}
