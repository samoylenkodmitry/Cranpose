//! Browser accessibility bridge for the canvas renderer.

use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use web_sys::{Document, Element, HtmlCanvasElement, HtmlElement, MouseEvent};

use crate::accessibility::{self, AccessibilityElement, AccessibilityRole};

/// Transparent DOM nodes mirroring the semantic controls painted on canvas.
pub(crate) struct WebAccessibilityBridge {
    root: HtmlElement,
    canvas: HtmlCanvasElement,
    previous: Vec<AccessibilityElement>,
}

impl WebAccessibilityBridge {
    pub(crate) fn install(
        document: &Document,
        canvas: HtmlCanvasElement,
        app: Rc<RefCell<AppShell<WgpuRenderer>>>,
    ) -> Result<Self, JsValue> {
        let root = document.create_element("div")?.dyn_into::<HtmlElement>()?;
        root.set_attribute("data-cranpose-accessibility", "")?;
        root.set_attribute("aria-label", "Application controls")?;
        let style = root.style();
        style.set_property("position", "fixed")?;
        style.set_property("inset", "0")?;
        style.set_property("z-index", "2147483647")?;
        style.set_property("pointer-events", "none")?;

        let click = Closure::wrap(Box::new(move |event: MouseEvent| {
            let Some(target) = event
                .target()
                .and_then(|target| target.dyn_into::<Element>().ok())
            else {
                return;
            };
            let Some(x) = target
                .get_attribute("data-cranpose-x")
                .and_then(|value| value.parse::<f32>().ok())
            else {
                return;
            };
            let Some(y) = target
                .get_attribute("data-cranpose-y")
                .and_then(|value| value.parse::<f32>().ok())
            else {
                return;
            };
            if let Ok(mut shell) = app.try_borrow_mut() {
                shell.set_cursor(x, y);
                shell.pointer_pressed();
                shell.pointer_released_at_position(x, y);
            }
        }) as Box<dyn FnMut(_)>);
        root.add_event_listener_with_callback("click", click.as_ref().unchecked_ref())?;
        click.forget();

        document
            .body()
            .ok_or("document has no body")?
            .append_child(&root)?;
        Ok(Self {
            root,
            canvas,
            previous: Vec::new(),
        })
    }

    pub(crate) fn sync(
        &mut self,
        document: &Document,
        shell: &mut AppShell<WgpuRenderer>,
    ) -> Result<(), JsValue> {
        let elements = accessibility::snapshot(shell);
        if elements == self.previous {
            return Ok(());
        }
        self.previous.clone_from(&elements);
        self.root.set_inner_html("");

        let canvas_rect = self.canvas.get_bounding_client_rect();
        let viewport = shell.viewport_size();
        let scale_x = canvas_rect.width() / viewport.0.max(1.0) as f64;
        let scale_y = canvas_rect.height() / viewport.1.max(1.0) as f64;

        // Identified by the shared accessibility element id rather than the
        // layout node id: a node that publishes drawn controls owns several
        // elements, and two DOM nodes claiming the same identity is exactly
        // the bug a screen reader trips over.
        let ids = accessibility::element_ids(&elements);
        for (id, element) in ids.into_iter().zip(elements) {
            let node = document
                .create_element(if element.clickable { "button" } else { "span" })?
                .dyn_into::<HtmlElement>()?;
            node.set_attribute("aria-label", &element.label)?;
            node.set_attribute("data-cranpose-node", &id.to_string())?;
            match element.role {
                AccessibilityRole::Button => node.set_attribute("role", "button")?,
                AccessibilityRole::StaticText => {
                    node.set_attribute("role", "text")?;
                    node.set_text_content(Some(&element.label));
                }
                AccessibilityRole::TextField => {
                    node.set_attribute("role", "textbox")?;
                    if let Some(value) = &element.value {
                        node.set_attribute("aria-valuetext", value)?;
                    }
                }
                AccessibilityRole::Checkbox => node.set_attribute("role", "checkbox")?,
                AccessibilityRole::Switch => node.set_attribute("role", "switch")?,
                AccessibilityRole::RadioButton => node.set_attribute("role", "radio")?,
                AccessibilityRole::Tab => node.set_attribute("role", "tab")?,
                AccessibilityRole::Image => node.set_attribute("role", "img")?,
                AccessibilityRole::Header => {
                    node.set_attribute("role", "heading")?;
                    node.set_attribute("aria-level", "2")?;
                    node.set_text_content(Some(&element.label));
                }
                AccessibilityRole::Dialog => {
                    node.set_attribute("role", "dialog")?;
                    node.set_attribute("aria-modal", "true")?;
                }
            }
            // ARIA has no direct `stateDescription`; the state a control is in
            // rides on the checked/selected attributes where the role has them,
            // and `aria-description` carries the app's wording either way.
            if let Some(state) = &element.state_description {
                node.set_attribute("aria-description", state)?;
            }
            if let Some(toggled) = element.toggled {
                node.set_attribute("aria-checked", if toggled { "true" } else { "false" })?;
            }
            if let Some(selected) = element.selected {
                let selected = if selected { "true" } else { "false" };
                match element.role {
                    AccessibilityRole::RadioButton => {
                        node.set_attribute("aria-checked", selected)?
                    }
                    _ => node.set_attribute("aria-selected", selected)?,
                }
            }
            if !element.enabled {
                node.set_attribute("aria-disabled", "true")?;
            }
            if element.clickable {
                let (x, y) = element.bounds.center();
                node.set_attribute("data-cranpose-x", &x.to_string())?;
                node.set_attribute("data-cranpose-y", &y.to_string())?;
            } else {
                node.set_attribute("tabindex", "-1")?;
            }
            let style = node.style();
            style.set_property("position", "fixed")?;
            style.set_property(
                "left",
                &format!(
                    "{}px",
                    canvas_rect.left() + element.bounds.x as f64 * scale_x
                ),
            )?;
            style.set_property(
                "top",
                &format!(
                    "{}px",
                    canvas_rect.top() + element.bounds.y as f64 * scale_y
                ),
            )?;
            style.set_property(
                "width",
                &format!("{}px", element.bounds.width as f64 * scale_x),
            )?;
            style.set_property(
                "height",
                &format!("{}px", element.bounds.height as f64 * scale_y),
            )?;
            style.set_property("opacity", "0.001")?;
            style.set_property("pointer-events", "none")?;
            style.set_property("overflow", "hidden")?;
            self.root.append_child(&node)?;
        }
        Ok(())
    }
}
