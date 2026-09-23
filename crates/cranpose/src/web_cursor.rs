//! Applying a [`PointerIcon`] to the canvas.
//!
//! A standard shape is a CSS `cursor` keyword the browser already knows. A
//! custom shape reaches the browser as an image, its pixels encoded once
//! through an offscreen canvas into a cached `data:` URL: re-encoding a PNG on
//! every pointer move would be the most expensive thing the move does.
//!
//! A CSS cursor image is the operating system's to draw, and macOS enlarges it
//! by the person's pointer size, which a page cannot read. So a custom cursor
//! asked for at [`CustomCursorSize::AsDrawn`] is drawn by the page instead: the
//! canvas's own cursor is hidden, and an element carrying the image follows
//! the pointer, at its drawn size in CSS pixels whatever the system does.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use cranpose_ui::{CustomPointerIcon, PointerIcon};
use wasm_bindgen::{Clamped, JsCast, JsValue, prelude::Closure};
use web_sys::{Document, HtmlCanvasElement, HtmlElement, ImageData, PointerEvent};

use crate::CustomCursorSize;

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

/// The cursors already built for this canvas, keyed by
/// [`CustomPointerIcon::id`].
pub(crate) struct WebCursors {
    size: CustomCursorSize,
    data_urls: HashMap<u64, String>,
    applied: Option<String>,
    drawn: Rc<RefCell<DrawnCursor>>,
}

/// A custom cursor the page draws itself, following the pointer over the
/// canvas.
#[derive(Default)]
struct DrawnCursor {
    element: Option<HtmlElement>,
    /// The hotspot of the image shown, or `None` while the system draws the
    /// cursor.
    hotspot: Option<(f64, f64)>,
    /// Where the pointer last was over the canvas, in its window's viewport.
    pointer: Option<(f64, f64)>,
}

impl DrawnCursor {
    /// Shows `data_url` as the cursor, `width` by `height` CSS pixels, in the
    /// document that holds `canvas`.
    fn show(
        &mut self,
        canvas: &HtmlCanvasElement,
        data_url: &str,
        size: (u32, u32),
        hotspot: (u32, u32),
    ) -> Result<(), JsValue> {
        let element = self.element_in(canvas)?;
        let style = element.style();
        style.set_property("width", &format!("{}px", size.0))?;
        style.set_property("height", &format!("{}px", size.1))?;
        style.set_property("background-image", &format!("url(\"{data_url}\")"))?;
        style.set_property("background-size", &format!("{}px {}px", size.0, size.1))?;
        self.hotspot = Some((f64::from(hotspot.0), f64::from(hotspot.1)));
        self.place();
        Ok(())
    }

    /// Lets the system draw the cursor again.
    fn hide(&mut self) {
        self.hotspot = None;
        self.place();
    }

    /// Follows the pointer to `at`, or out of the canvas for `None`.
    fn pointer_at(&mut self, canvas: &HtmlCanvasElement, at: Option<(f64, f64)>) {
        self.pointer = at;
        if self.hotspot.is_some() {
            // The canvas may have moved into a floating window since.
            let _ = self.element_in(canvas);
        }
        self.place();
    }

    fn place(&self) {
        let Some(element) = &self.element else {
            return;
        };
        let style = element.style();
        let (Some((x, y)), Some((hot_x, hot_y))) = (self.pointer, self.hotspot) else {
            let _ = style.set_property("display", "none");
            return;
        };
        let _ = style.set_property(
            "transform",
            &format!("translate({}px, {}px)", x - hot_x, y - hot_y),
        );
        let _ = style.set_property("display", "block");
    }

    /// The cursor element, created in, or moved to, the document holding
    /// `canvas`.
    fn element_in(&mut self, canvas: &HtmlCanvasElement) -> Result<HtmlElement, JsValue> {
        let document = canvas
            .owner_document()
            .ok_or_else(|| JsValue::from_str("the canvas is in no document"))?;
        let body = document
            .body()
            .ok_or_else(|| JsValue::from_str("the canvas's document has no body"))?;
        if let Some(element) = &self.element {
            if element.owner_document().as_ref() != Some(&document) {
                body.append_child(element)?;
            }
            return Ok(element.clone());
        }
        let element = document.create_element("div")?.dyn_into::<HtmlElement>()?;
        element.set_attribute("aria-hidden", "true")?;
        element.set_attribute(
            "style",
            "position: fixed; left: 0; top: 0; display: none; pointer-events: none; \
             z-index: 2147483647; background-repeat: no-repeat; \
             image-rendering: pixelated; will-change: transform;",
        )?;
        body.append_child(&element)?;
        self.element = Some(element.clone());
        Ok(element)
    }
}

impl WebCursors {
    /// The cursors for `canvas`, drawn at the size `size` asks for.
    pub(crate) fn new(canvas: &HtmlCanvasElement, size: CustomCursorSize) -> Self {
        let drawn = Rc::new(RefCell::new(DrawnCursor::default()));
        if size == CustomCursorSize::AsDrawn {
            follow_pointer(canvas, &drawn);
        }
        Self {
            size,
            data_urls: HashMap::new(),
            applied: None,
            drawn,
        }
    }

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
            PointerIcon::Custom(custom) => match self.data_url(document, custom) {
                Some(data_url) if self.size == CustomCursorSize::AsDrawn => {
                    let image = custom.image();
                    let shown = self.drawn.borrow_mut().show(
                        canvas,
                        &data_url,
                        (image.width(), image.height()),
                        (custom.hotspot_x(), custom.hotspot_y()),
                    );
                    match shown {
                        Ok(()) => {
                            self.set_css_cursor(canvas, "none".to_string());
                            return;
                        }
                        Err(error) => {
                            log::warn!("the page could not draw a custom cursor: {error:?}");
                            "default".to_string()
                        }
                    }
                }
                Some(data_url) => format!(
                    "url(\"{data_url}\") {} {}, default",
                    custom.hotspot_x(),
                    custom.hotspot_y()
                ),
                None => "default".to_string(),
            },
        };
        self.drawn.borrow_mut().hide();
        self.set_css_cursor(canvas, value);
    }

    fn set_css_cursor(&mut self, canvas: &HtmlCanvasElement, value: String) {
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

    /// The PNG `data:` URL of `custom`, encoding its pixels the first time it
    /// is asked for.
    fn data_url(&mut self, document: &Document, custom: &CustomPointerIcon) -> Option<String> {
        let id = custom.id();
        if let Some(data_url) = self.data_urls.get(&id) {
            return Some(data_url.clone());
        }
        match encode_png_data_url(document, custom) {
            Ok(data_url) => {
                self.data_urls.insert(id, data_url.clone());
                Some(data_url)
            }
            Err(error) => {
                log::warn!("a custom cursor could not be encoded: {error:?}");
                None
            }
        }
    }
}

/// Moves the page-drawn cursor with the pointer over `canvas`, and out of the
/// way when the pointer leaves it or is a finger, which has no cursor. The
/// listeners sit on the canvas, so they go wherever the canvas goes, a
/// floating window included.
fn follow_pointer(canvas: &HtmlCanvasElement, drawn: &Rc<RefCell<DrawnCursor>>) {
    for (event, leaves) in [
        ("pointermove", false),
        ("pointerenter", false),
        ("pointerleave", true),
    ] {
        let drawn = drawn.clone();
        let target = canvas.clone();
        let listener = Closure::wrap(Box::new(move |event: PointerEvent| {
            let at = (!leaves && event.pointer_type() != "touch")
                .then(|| (f64::from(event.client_x()), f64::from(event.client_y())));
            drawn.borrow_mut().pointer_at(&target, at);
        }) as Box<dyn FnMut(PointerEvent)>);
        if canvas
            .add_event_listener_with_callback(event, listener.as_ref().unchecked_ref())
            .is_ok()
        {
            listener.forget();
        }
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
