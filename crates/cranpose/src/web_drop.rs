//! Files dropped onto the canvas.
//!
//! A drag-and-drop is the browser's version of a share or a document intent:
//! content arriving from outside the running composition, named and typed by
//! whoever dropped it rather than picked through
//! [`cranpose_services::FilePicker`]. It travels the same
//! [`IncomingContent`] pipeline as an Android `ACTION_SEND` share (see
//! `nativeOnIncomingContent` in `android_services.rs`), so application code
//! that already collects [`cranpose_services::rememberIncomingContent`] sees
//! a drop without knowing the web target exists.
//!
//! Reading a dropped `File`'s bytes uses `Blob.arrayBuffer()` rather than the
//! `FileReader` dance `cranpose-services`' web file picker uses internally to
//! read an `rfd::FileHandle`. `rfd` builds that handle from a `web_sys::File`
//! through a constructor private to its own crate, so nothing outside it can
//! wrap a dropped `File` the same way; `arrayBuffer()` is the promise-native
//! read the picker's `FileReader` code predates, not a second, divergent
//! reimplementation of it. Both paths still end at the same
//! `BytesContent::new(ContentMetadata::named(name), bytes)` shape a picked
//! file resolves through, by way of [`IncomingContent::content`].

use std::rc::Rc;

use cranpose_services::{publish_incoming_content, IncomingContent};
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{DragEvent, File, HtmlCanvasElement};

/// Wires `dragenter`/`dragover`/`drop` on `canvas` so files dropped onto it
/// arrive as [`IncomingContent`], and wakes `request_frame` once each is read.
pub(crate) fn install(
    canvas: &HtmlCanvasElement,
    request_frame: Rc<dyn Fn()>,
) -> Result<(), JsValue> {
    // A browser refuses to fire `drop` unless something upstream calls
    // `prevent_default` on both `dragenter` and `dragover` — Firefox in
    // particular ignores a drop whose `dragenter` was left at the default.
    for event_name in ["dragenter", "dragover"] {
        let closure = Closure::wrap(Box::new(move |event: DragEvent| {
            event.prevent_default();
        }) as Box<dyn FnMut(_)>);
        canvas.add_event_listener_with_callback(event_name, closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    let closure = Closure::wrap(Box::new(move |event: DragEvent| {
        event.prevent_default();
        let Some(data_transfer) = event.data_transfer() else {
            return;
        };
        let Some(files) = data_transfer.files() else {
            return;
        };
        for index in 0..files.length() {
            let Some(file) = files.get(index) else {
                continue;
            };
            let request_frame = request_frame.clone();
            spawn_local(async move {
                let Some(content) = read_dropped_file(file).await else {
                    return;
                };
                publish_incoming_content(content);
                request_frame();
            });
        }
    }) as Box<dyn FnMut(_)>);
    canvas.add_event_listener_with_callback("drop", closure.as_ref().unchecked_ref())?;
    closure.forget();

    Ok(())
}

/// Reads one dropped `File`'s bytes into an [`IncomingContent`], or reports
/// why it could not be read and drops it.
async fn read_dropped_file(file: File) -> Option<IncomingContent> {
    let name = file.name();
    let mime_type = file.type_();
    let buffer = match JsFuture::from(file.array_buffer()).await {
        Ok(buffer) => buffer,
        Err(error) => {
            log::warn!("dropped file {name:?} could not be read: {error:?}");
            return None;
        }
    };
    let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
    let mut content = IncomingContent::from_bytes(bytes).with_name(name);
    if !mime_type.is_empty() {
        content = content.with_mime_type(mime_type);
    }
    Some(content)
}
