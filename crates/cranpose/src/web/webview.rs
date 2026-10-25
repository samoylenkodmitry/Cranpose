use std::rc::Rc;

use cranpose_ui::Rect;
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::{Document, HtmlCanvasElement, HtmlElement, HtmlIFrameElement, Window};

use crate::{
    native_view::NativeViewHost,
    webview_host::{BrowserView, WebViews},
};

pub(crate) struct BrowserFrame {
    frame: HtmlIFrameElement,
    _loaded: Closure<dyn FnMut()>,
}

impl BrowserView for BrowserFrame {
    fn navigate(&mut self, url: &str) -> Result<(), String> {
        self.frame.set_src(url);
        Ok(())
    }

    fn place(&mut self, bounds: Rect) -> Result<(), String> {
        self.frame.style().set_css_text(&format!(
            "position:absolute;border:0;pointer-events:auto;left:{}px;top:{}px;width:{}px;height:{}px;",
            bounds.x, bounds.y, bounds.width.max(0.0), bounds.height.max(0.0),
        ));
        Ok(())
    }
}

impl Drop for BrowserFrame {
    fn drop(&mut self) {
        self.frame.set_onload(None);
        self.frame.remove();
    }
}

#[derive(Default)]
pub(crate) struct BrowserViews {
    views: WebViews<BrowserFrame>,
    container: Option<BrowserContainer>,
}

impl BrowserViews {
    pub(crate) fn dispatch(&self, host: &NativeViewHost) {
        self.views.dispatch(host);
    }

    pub(crate) fn sync<R: cranpose_render_common::Renderer>(
        &mut self,
        host: &NativeViewHost,
        shell: &mut cranpose_app_shell::AppShell<R>,
        canvas: &HtmlCanvasElement,
        wake: &Rc<dyn Fn()>,
    ) where
        R::Error: std::fmt::Debug,
    {
        if host.is_empty() && self.views.is_empty() {
            return;
        }
        self.views
            .sync(host, shell.layout_tree(), |id, url, bounds, events| {
                let document = canvas.owner_document().ok_or("canvas has no document")?;
                let container = browser_container(&mut self.container, &document, wake)?;
                let frame: HtmlIFrameElement = document
                    .create_element("iframe")
                    .map_err(|error| format!("{error:?}"))?
                    .dyn_into()
                    .map_err(|_| "could not create iframe")?;
                frame.set_title("Embedded website");
                let events = events.clone();
                let callback_frame = frame.clone();
                let wake = Rc::clone(wake);
                let loaded = Closure::wrap(Box::new(move || {
                    let _ = events.send((id, format!("loaded:{}", callback_frame.src())));
                    wake();
                }) as Box<dyn FnMut()>);
                frame.set_onload(Some(loaded.as_ref().unchecked_ref()));
                container
                    .element
                    .append_child(&frame)
                    .map_err(|error| format!("{error:?}"))?;
                let mut view = BrowserFrame {
                    frame,
                    _loaded: loaded,
                };
                view.place(bounds)?;
                view.navigate(url)?;
                Ok(view)
            });
        if self.views.is_empty() {
            self.container = None;
        } else if let Some(container) = &mut self.container {
            container.place(canvas);
        }
    }
}

fn browser_container<'a>(
    slot: &'a mut Option<BrowserContainer>,
    document: &Document,
    wake: &Rc<dyn Fn()>,
) -> Result<&'a BrowserContainer, String> {
    if slot.is_none() {
        *slot = Some(BrowserContainer::new(document, wake)?);
    }
    slot.as_ref()
        .ok_or_else(|| "browser container was not created".to_owned())
}

struct BrowserContainer {
    element: HtmlElement,
    window: Window,
    moved: Closure<dyn FnMut()>,
    bounds: Option<(f64, f64, f64, f64)>,
}

impl BrowserContainer {
    fn new(document: &Document, wake: &Rc<dyn Fn()>) -> Result<Self, String> {
        let element: HtmlElement = document
            .create_element("div")
            .map_err(|error| format!("{error:?}"))?
            .dyn_into()
            .map_err(|_| "could not create browser container")?;
        let window = document.default_view().ok_or("document has no window")?;
        let wake = Rc::clone(wake);
        let moved = Closure::wrap(Box::new(move || wake()) as Box<dyn FnMut()>);
        let container = Self {
            element,
            window,
            moved,
            bounds: None,
        };
        for event in ["scroll", "resize"] {
            container
                .window
                .add_event_listener_with_callback_and_bool(
                    event,
                    container.moved.as_ref().unchecked_ref(),
                    true,
                )
                .map_err(|error| format!("{error:?}"))?;
        }
        document
            .body()
            .ok_or("document has no body")?
            .append_child(&container.element)
            .map_err(|error| format!("{error:?}"))?;
        Ok(container)
    }

    fn place(&mut self, canvas: &HtmlCanvasElement) {
        let rect = canvas.get_bounding_client_rect();
        let bounds = (rect.x(), rect.y(), rect.width(), rect.height());
        if self.bounds != Some(bounds) {
            self.element.style().set_css_text(&format!(
                "position:fixed;overflow:hidden;pointer-events:none;left:{}px;top:{}px;width:{}px;height:{}px;",
                bounds.0, bounds.1, bounds.2, bounds.3,
            ));
            self.bounds = Some(bounds);
        }
    }
}

impl Drop for BrowserContainer {
    fn drop(&mut self) {
        for event in ["scroll", "resize"] {
            let _ = self.window.remove_event_listener_with_callback_and_bool(
                event,
                self.moved.as_ref().unchecked_ref(),
                true,
            );
        }
        self.element.remove();
    }
}
