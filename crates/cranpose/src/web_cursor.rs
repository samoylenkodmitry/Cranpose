//! Applying a [`PointerIcon`] to the canvas's CSS `cursor`.
//!
//! A standard shape is a keyword the browser already knows. A custom shape has
//! to reach CSS as an image URL, so its pixels are encoded once through an
//! offscreen canvas and the resulting `data:` URL is cached: re-encoding a PNG
//! on every pointer move would be the most expensive thing the move does.

use std::{cell::RefCell, collections::HashMap};

use cranpose_ui::{CustomPointerIcon, PointerIcon};
use wasm_bindgen::{Clamped, JsCast, JsValue};
use web_sys::{Document, HtmlCanvasElement, HtmlElement, ImageData};

/// Applies the pointer icon the shell resolved since the last call, if any.
///
/// The frame loop calls this once per frame with
/// `AppShell::take_pointer_icon_change`, which is `None` for every frame where
/// the pointer stayed inside one region.
pub(crate) fn sync_pointer_icon(
    cursors: &RefCell<WebCursors>,
    document: &Document,
    canvas: &HtmlCanvasElement,
    icon: Option<PointerIcon>,
) {
    let Some(icon) = icon else {
        return;
    };
    cursors.borrow_mut().apply(document, canvas, &icon);
}

/// The CSS `cursor` values already built for this canvas, keyed by
/// [`CustomPointerIcon::id`].
#[derive(Default)]
pub(crate) struct WebCursors {
    encoded: HashMap<u64, String>,
    applied: Option<String>,
}

impl WebCursors {
    /// Sets `icon` as the canvas's cursor.
    ///
    /// A custom icon the browser cannot encode falls back to the default arrow,
    /// which is what an unreadable `url()` would leave on screen anyway.
    pub(crate) fn apply(
        &mut self,
        document: &Document,
        canvas: &HtmlCanvasElement,
        icon: &PointerIcon,
    ) {
        let value = match icon {
            PointerIcon::System(system) => system.name().to_string(),
            PointerIcon::Custom(custom) => self
                .css_url(document, custom)
                .unwrap_or_else(|| "default".to_string()),
        };
        if self.applied.as_deref() == Some(value.as_str()) {
            return;
        }
        let Some(element) = canvas.dyn_ref::<HtmlElement>() else {
            return;
        };
        if element.style().set_property("cursor", &value).is_ok() {
            self.applied = Some(value);
        }
    }

    /// The `url(...) hotspot-x hotspot-y, default` value for `custom`, encoding
    /// its pixels the first time it is asked for.
    fn css_url(&mut self, document: &Document, custom: &CustomPointerIcon) -> Option<String> {
        let id = custom.id();
        if let Some(value) = self.encoded.get(&id) {
            return Some(value.clone());
        }
        let data_url = match encode_png_data_url(document, custom) {
            Ok(data_url) => data_url,
            Err(error) => {
                log::warn!("a custom cursor could not be encoded for CSS: {error:?}");
                return None;
            }
        };
        let value = format!(
            "url(\"{data_url}\") {} {}, default",
            custom.hotspot_x(),
            custom.hotspot_y()
        );
        self.encoded.insert(id, value.clone());
        Some(value)
    }
}

/// Encodes a custom icon's straight-alpha RGBA pixels as a PNG `data:` URL,
/// using the browser's own encoder through an offscreen canvas.
fn encode_png_data_url(document: &Document, custom: &CustomPointerIcon) -> Result<String, JsValue> {
    let image = custom.image();
    let scratch = document
        .create_element("canvas")?
        .dyn_into::<HtmlCanvasElement>()?;
    scratch.set_width(image.width());
    scratch.set_height(image.height());
    let context = scratch
        .get_context("2d")?
        .ok_or_else(|| JsValue::from_str("the browser gave no 2d canvas context"))?
        .dyn_into::<web_sys::CanvasRenderingContext2d>()?;
    let data = ImageData::new_with_u8_clamped_array_and_sh(
        Clamped(image.pixels()),
        image.width(),
        image.height(),
    )?;
    context.put_image_data(&data, 0.0, 0.0)?;
    scratch.to_data_url_with_type("image/png")
}
