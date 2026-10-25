//! The page's lifecycle: resumed while its tab is visible and focused,
//! paused while visible without focus, and stopped in a background tab or a
//! minimized browser window.

use std::rc::Rc;

use cranpose_services::{advance_lifecycle, window_lifecycle_state};
use wasm_bindgen::{JsCast, prelude::Closure};
use web_sys::{Document, EventTarget, Window};

pub(crate) fn watch(window: &Window, request_frame: Rc<dyn Fn()>) {
    let Some(document) = window.document() else {
        return;
    };
    publish(&document);
    let targets: [(&EventTarget, &str); 3] = [
        (&document, "visibilitychange"),
        (window, "focus"),
        (window, "blur"),
    ];
    for (target, event) in targets {
        let document = document.clone();
        let request_frame = Rc::clone(&request_frame);
        let closure = Closure::wrap(Box::new(move |_event: web_sys::Event| {
            publish(&document);
            request_frame();
        }) as Box<dyn FnMut(_)>);
        if target
            .add_event_listener_with_callback(event, closure.as_ref().unchecked_ref())
            .is_err()
        {
            log::debug!("the browser took no `{event}` listener for the page lifecycle");
        }
        closure.forget();
    }
}

fn publish(document: &Document) {
    let visible = !document.hidden();
    let focused = document.has_focus().unwrap_or(visible);
    advance_lifecycle(window_lifecycle_state(visible, focused));
}
