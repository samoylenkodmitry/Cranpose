use std::rc::Rc;

use cranpose_services::{IncomingContent, publish_incoming_content};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{DragEvent, File, HtmlCanvasElement};

pub(crate) fn install(
    canvas: &HtmlCanvasElement,
    request_frame: Rc<dyn Fn()>,
) -> Result<(), JsValue> {
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
