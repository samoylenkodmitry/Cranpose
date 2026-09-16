use std::{cell::RefCell, collections::HashMap, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{Document, Element, HtmlCanvasElement, HtmlElement, MouseEvent};

use crate::accessibility::{self, AccessibilityElement, AccessibilityRole};

/// The role, value and state a screen reader reads off the mirrored element.
fn apply_role_and_state(node: &HtmlElement, element: &AccessibilityElement) -> Result<(), JsValue> {
    node.set_attribute("role", aria_role(element.role))?;
    apply_role_extras(node, element)?;
    apply_aria_state(node, element)
}

/// The ARIA role a screen reader reads the control as.
fn aria_role(role: AccessibilityRole) -> &'static str {
    match role {
        AccessibilityRole::Button => "button",
        AccessibilityRole::StaticText => "text",
        AccessibilityRole::TextField => "textbox",
        AccessibilityRole::Checkbox => "checkbox",
        AccessibilityRole::Switch => "switch",
        AccessibilityRole::RadioButton => "radio",
        AccessibilityRole::Tab => "tab",
        AccessibilityRole::Image => "img",
        AccessibilityRole::Header => "heading",
        AccessibilityRole::Dialog => "dialog",
    }
}

/// What a role asks for beyond its name: text to read, a heading level, the
/// value of a field, or the modal flag on a dialog.
fn apply_role_extras(node: &HtmlElement, element: &AccessibilityElement) -> Result<(), JsValue> {
    match element.role {
        AccessibilityRole::StaticText => node.set_text_content(Some(&element.label)),
        AccessibilityRole::TextField => {
            if let Some(value) = &element.value {
                node.set_attribute("aria-valuetext", value)?;
            }
        }
        AccessibilityRole::Header => {
            node.set_attribute("aria-level", "2")?;
            node.set_text_content(Some(&element.label));
        }
        AccessibilityRole::Dialog => node.set_attribute("aria-modal", "true")?,
        _ => {}
    }
    Ok(())
}

/// The state description, the checked or selected flag, and whether the
/// control is disabled.
fn apply_aria_state(node: &HtmlElement, element: &AccessibilityElement) -> Result<(), JsValue> {
    if let Some(state) = &element.state_description {
        node.set_attribute("aria-description", state)?;
    }
    if let Some(toggled) = element.toggled {
        node.set_attribute("aria-checked", if toggled { "true" } else { "false" })?;
    }
    if let Some(selected) = element.selected {
        let selected = if selected { "true" } else { "false" };
        match element.role {
            AccessibilityRole::RadioButton => node.set_attribute("aria-checked", selected)?,
            _ => node.set_attribute("aria-selected", selected)?,
        }
    }
    if !element.enabled {
        node.set_attribute("aria-disabled", "true")?;
    }
    Ok(())
}

/// Puts the mirrored element over the control it stands for, so a reader's
/// cursor and a touch exploration land in the same place.
fn place_node(
    node: &HtmlElement,
    element: &AccessibilityElement,
    left: f64,
    top: f64,
    scale_x: f64,
    scale_y: f64,
) -> Result<(), JsValue> {
    let style = node.style();
    style.set_property("position", "fixed")?;
    style.set_property(
        "left",
        &format!("{}px", left + element.bounds.x as f64 * scale_x),
    )?;
    style.set_property(
        "top",
        &format!("{}px", top + element.bounds.y as f64 * scale_y),
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
    Ok(())
}

/// Hands a Tab landing or a screen reader focus on the mirror back to the app.
fn attach_focus_listener(
    root: &HtmlElement,
    node_ids: Rc<RefCell<HashMap<i32, cranpose_core::NodeId>>>,
) -> Result<(), JsValue> {
    let focus_in = Closure::wrap(Box::new(move |event: web_sys::Event| {
        let Some(target) = event.target().and_then(|t| t.dyn_into::<Element>().ok()) else {
            return;
        };
        let Some(element_id) = target
            .get_attribute("data-cranpose-node")
            .and_then(|value| value.parse::<i32>().ok())
        else {
            return;
        };
        let Some(node_id) = node_ids.borrow().get(&element_id).copied() else {
            return;
        };
        accessibility::focus_node(node_id);
    }) as Box<dyn FnMut(_)>);
    root.add_event_listener_with_callback("focusin", focus_in.as_ref().unchecked_ref())?;
    focus_in.forget();
    Ok(())
}

pub(crate) struct WebAccessibilityBridge {
    root: HtmlElement,
    canvas: HtmlCanvasElement,
    previous: Vec<AccessibilityElement>,
    node_ids: Rc<RefCell<HashMap<i32, cranpose_core::NodeId>>>,
    focused_element: Option<i32>,
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
        let node_ids: Rc<RefCell<HashMap<i32, cranpose_core::NodeId>>> =
            Rc::new(RefCell::new(HashMap::new()));
        attach_focus_listener(&root, Rc::clone(&node_ids))?;

        Ok(Self {
            root,
            canvas,
            previous: Vec::new(),
            node_ids,
            focused_element: None,
        })
    }

    /// Moves the browser's focus onto the control the app focused, so a
    /// screen reader on the mirror follows a focus move the app made.
    fn follow_app_focus(
        &mut self,
        node: &HtmlElement,
        element: &AccessibilityElement,
        id: i32,
    ) -> Result<(), JsValue> {
        if !element.focused || self.focused_element == Some(id) {
            return Ok(());
        }
        self.focused_element = Some(id);
        node.focus()
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
        self.node_ids.borrow_mut().clear();

        let canvas_rect = self.canvas.get_bounding_client_rect();
        let viewport = shell.viewport_size();
        let scale_x = canvas_rect.width() / viewport.0.max(1.0) as f64;
        let scale_y = canvas_rect.height() / viewport.1.max(1.0) as f64;

        let ids = accessibility::element_ids(&elements);
        for (id, element) in ids.into_iter().zip(elements) {
            let node = document
                .create_element(if element.clickable { "button" } else { "span" })?
                .dyn_into::<HtmlElement>()?;
            node.set_attribute("aria-label", &element.label)?;
            node.set_attribute("data-cranpose-node", &id.to_string())?;
            apply_role_and_state(&node, &element)?;
            if element.clickable {
                let (x, y) = element.bounds.center();
                node.set_attribute("data-cranpose-x", &x.to_string())?;
                node.set_attribute("data-cranpose-y", &y.to_string())?;
            }
            node.set_attribute(
                "tabindex",
                if element.focusable {
                    "0"
                } else if element.clickable {
                    "auto"
                } else {
                    "-1"
                },
            )?;
            self.node_ids.borrow_mut().insert(id, element.node_id);
            place_node(
                &node,
                &element,
                canvas_rect.left(),
                canvas_rect.top(),
                scale_x,
                scale_y,
            )?;
            self.root.append_child(&node)?;
            self.follow_app_focus(&node, &element, id)?;
        }
        if !self.previous.iter().any(|element| element.focused) {
            self.focused_element = None;
        }
        Ok(())
    }
}
