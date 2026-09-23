//! Hears a web app's canvas change size in whichever window holds the canvas.
//!
//! A `ResizeObserver` reports during the rendering of the document it was made
//! in, and a `resize` event fires on the window that resized. Made in the page,
//! neither hears a canvas the page has moved into a floating window: the page
//! is hidden behind that window, renders nothing, and never resizes. So both
//! live in the canvas's own window, and move with the canvas.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use wasm_bindgen::{JsCast, JsValue, prelude::Closure};
use web_sys::{HtmlCanvasElement, Window};

pub(crate) struct CanvasWatch {
    canvas: HtmlCanvasElement,
    on_resize: Closure<dyn FnMut(js_sys::Array, JsValue)>,
    on_window_resize: Closure<dyn FnMut()>,
    watching: RefCell<Option<(Window, JsValue)>>,
}

impl CanvasWatch {
    /// Watches `canvas`, keeping `device_size` at the canvas's size in device
    /// pixels where the browser counts them, and calling `changed` whenever the
    /// canvas or its window changes size.
    pub(crate) fn new(
        canvas: HtmlCanvasElement,
        changed: Rc<dyn Fn()>,
        device_size: Rc<Cell<Option<(u32, u32)>>>,
    ) -> Self {
        let on_resize = {
            let changed = changed.clone();
            Closure::wrap(Box::new(move |entries: js_sys::Array, _observer: JsValue| {
                device_size.set(observed_device_size(&entries));
                changed();
            }) as Box<dyn FnMut(js_sys::Array, JsValue)>)
        };
        let on_window_resize = Closure::wrap(Box::new(move || changed()) as Box<dyn FnMut()>);
        Self {
            canvas,
            on_resize,
            on_window_resize,
            watching: RefCell::new(None),
        }
    }

    /// Moves the watch into the window that holds the canvas now, when that is
    /// not the window it watches from. Cheap when nothing moved, so a frame can
    /// ask every time.
    pub(crate) fn follow(&self) {
        let Some(host) = crate::web_frame_host::window() else {
            return;
        };
        let moved = self.watching.borrow().as_ref().is_none_or(|(watched, _)| {
            AsRef::<JsValue>::as_ref(watched) != AsRef::<JsValue>::as_ref(&host)
        });
        if !moved {
            return;
        }
        if let Some((window, observer)) = self.watching.take() {
            // The window may be a floating one already closed; letting go of
            // it is all that is left to do.
            let _ = call_method(&observer, "disconnect", &[]);
            let _ = window.remove_event_listener_with_callback(
                "resize",
                self.on_window_resize.as_ref().unchecked_ref(),
            );
        }
        match self.observe_from(&host) {
            Ok(observer) => {
                let _ = host.add_event_listener_with_callback(
                    "resize",
                    self.on_window_resize.as_ref().unchecked_ref(),
                );
                self.watching.replace(Some((host, observer)));
            }
            Err(error) => log::error!("the canvas cannot be watched for size: {error:?}"),
        }
    }

    /// A `ResizeObserver` made in `host`, watching the canvas's device-pixel
    /// box where the browser has one, which also hears a change of pixel ratio
    /// that leaves the CSS box as it was, and its CSS box otherwise.
    fn observe_from(&self, host: &Window) -> Result<JsValue, JsValue> {
        let constructor: js_sys::Function =
            js_sys::Reflect::get(host, &"ResizeObserver".into())?.dyn_into()?;
        let observer =
            js_sys::Reflect::construct(&constructor, &js_sys::Array::of1(self.on_resize.as_ref()))?;
        let options = js_sys::Object::new();
        js_sys::Reflect::set(&options, &"box".into(), &"device-pixel-content-box".into())?;
        if call_method(&observer, "observe", &[&self.canvas, &options]).is_err() {
            call_method(&observer, "observe", &[&self.canvas])?;
        }
        Ok(observer)
    }
}

fn call_method(target: &JsValue, name: &str, arguments: &[&JsValue]) -> Result<JsValue, JsValue> {
    let method: js_sys::Function = js_sys::Reflect::get(target, &name.into())?.dyn_into()?;
    let arguments: js_sys::Array = arguments.iter().copied().collect();
    method.apply(target, &arguments)
}

/// The canvas's device-pixel size from a resize observation, where the browser
/// reports one.
fn observed_device_size(entries: &js_sys::Array) -> Option<(u32, u32)> {
    let sizes = js_sys::Reflect::get(&entries.get(0), &"devicePixelContentBoxSize".into()).ok()?;
    let size = js_sys::Reflect::get(&sizes, &0.into()).ok()?;
    let length = |name: &str| {
        js_sys::Reflect::get(&size, &name.into())
            .ok()?
            .as_f64()
            .map(|pixels| pixels.round() as u32)
    };
    Some((length("inlineSize")?, length("blockSize")?))
}
