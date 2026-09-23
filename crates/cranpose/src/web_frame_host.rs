//! The window a web app's frames are asked of: the one that holds its canvas.
//!
//! A canvas can move to another window -- a Document Picture-in-Picture window
//! floats it -- and a frame asked of the page it left runs on that page's
//! schedule, which a browser pauses or slows while the page is hidden, as it is
//! while the person works in the floating window.

use std::cell::RefCell;

use wasm_bindgen::JsValue;
use web_sys::{HtmlCanvasElement, Window};

thread_local! {
    static CANVAS: RefCell<Option<HtmlCanvasElement>> = const { RefCell::new(None) };
    static ASKED: RefCell<Option<Window>> = const { RefCell::new(None) };
}

/// Makes `canvas` the one whose window frames are asked of.
pub(crate) fn follow(canvas: &HtmlCanvasElement) {
    CANVAS.with(|slot| *slot.borrow_mut() = Some(canvas.clone()));
}

/// The window the canvas is in now, or the page's before there is a canvas.
pub(crate) fn window() -> Option<Window> {
    CANVAS
        .with(|slot| {
            slot.borrow()
                .as_ref()
                .and_then(|canvas| canvas.owner_document())
                .and_then(|document| document.default_view())
        })
        .or_else(web_sys::window)
}

/// Whether a frame already asked for will come to `current`: it was asked of
/// that window, and the window is still open. A frame asked of a window the
/// canvas has since left, or of a floating window since closed, never comes.
pub(crate) fn asked_of(current: &Window) -> bool {
    ASKED.with(|slot| {
        slot.borrow().as_ref().is_some_and(|asked| {
            AsRef::<JsValue>::as_ref(asked) == AsRef::<JsValue>::as_ref(current)
                && !asked.closed().unwrap_or(true)
        })
    })
}

/// Records the window the pending frame was asked of.
pub(crate) fn note_asked(window: &Window) {
    ASKED.with(|slot| *slot.borrow_mut() = Some(window.clone()));
}
