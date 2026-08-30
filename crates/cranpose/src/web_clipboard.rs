use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;
use cranpose_ui::clipboard_session::PlatformClipboard;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::{JsFuture, spawn_local};

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
        let promise = clipboard.write_text(text);
        spawn_local(async move {
            if let Err(error) = JsFuture::from(promise).await {
                log::warn!("browser clipboard write failed: {error:?}");
            }
        });
    }

    fn read_text(&self) -> Option<String> {
        None
    }

    fn can_request_paste(&self) -> bool {
        Self::dom_clipboard().is_some()
    }

    fn request_paste(&self) -> bool {
        let Some(clipboard) = Self::dom_clipboard() else {
            return false;
        };
        let promise = clipboard.read_text();
        let app = self.app.clone();
        let request_frame = Rc::clone(&self.request_frame);
        spawn_local(async move {
            let text = match JsFuture::from(promise).await {
                Ok(value) => value.as_string().unwrap_or_default(),
                Err(error) => {
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
