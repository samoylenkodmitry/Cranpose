use std::{cell::RefCell, collections::HashMap, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;
use cranpose_ui::LiveRegionMode;
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{Document, Element, HtmlCanvasElement, HtmlElement, MouseEvent};

use crate::accessibility::{self, AccessibilityElement, AccessibilityRole};

/// The role, value and state a screen reader reads off the mirrored element.
fn apply_role_and_state(node: &HtmlElement, element: &AccessibilityElement) -> Result<(), JsValue> {
    let role = match element.progress {
        Some(_) => "slider",
        None => aria_role(element.role),
    };
    node.set_attribute("role", role)?;
    apply_role_extras(node, element)?;
    apply_progress(node, element)?;
    apply_aria_state(node, element)
}

/// The value an adjustable control holds, and the stops an arrow key moves it
/// by. A screen reader reads the value and offers its own way to change it.
fn apply_progress(node: &HtmlElement, element: &AccessibilityElement) -> Result<(), JsValue> {
    let Some(progress) = element.progress else {
        return Ok(());
    };
    node.set_attribute("aria-valuenow", &progress.current.to_string())?;
    node.set_attribute("aria-valuemin", &progress.start.to_string())?;
    node.set_attribute("aria-valuemax", &progress.end.to_string())?;
    if let Some(text) = &element.state_description {
        node.set_attribute("aria-valuetext", text)?;
    }
    if element.adjustable {
        node.set_attribute("data-cranpose-value", &progress.current.to_string())?;
        node.set_attribute("data-cranpose-min", &progress.start.to_string())?;
        node.set_attribute("data-cranpose-max", &progress.end.to_string())?;
        node.set_attribute("data-cranpose-step", &progress.step().to_string())?;
    }
    Ok(())
}

/// The scroll container each element sits in, with the move one page on
/// makes: the container's virtual id and the forward delta.
fn page_targets(ids: &[i32], elements: &[AccessibilityElement]) -> Vec<Option<(i32, f32, f32)>> {
    elements
        .iter()
        .map(|element| {
            let container = accessibility::scroll_container_for(elements, element)?;
            let index = elements
                .iter()
                .position(|candidate| std::ptr::eq(candidate, container))?;
            let (dx, dy) = accessibility::page_delta(container, true);
            Some((*ids.get(index)?, dx, dy))
        })
        .collect()
}

fn apply_page(node: &HtmlElement, page: Option<(i32, f32, f32)>) -> Result<(), JsValue> {
    let Some((container, dx, dy)) = page else {
        return Ok(());
    };
    node.set_attribute("data-cranpose-page", &container.to_string())?;
    node.set_attribute("data-cranpose-page-dx", &dx.to_string())?;
    node.set_attribute("data-cranpose-page-dy", &dy.to_string())?;
    Ok(())
}

/// Pages the scroll container around the focused mirror node on Page Down and
/// Page Up, so a keyboard reader reaches rows a lazy list has not built yet.
fn attach_page_listener(
    root: &HtmlElement,
    app: Rc<RefCell<AppShell<WgpuRenderer>>>,
    node_ids: Rc<RefCell<HashMap<i32, cranpose_core::NodeId>>>,
) -> Result<(), JsValue> {
    let key_down = Closure::wrap(Box::new(move |event: web_sys::KeyboardEvent| {
        let sign = match event.key().as_str() {
            "PageDown" => 1.0,
            "PageUp" => -1.0,
            _ => return,
        };
        let Some(target) = key_target(&event) else {
            return;
        };
        let (Some(dx), Some(dy)) = (
            number_attribute(&target, "data-cranpose-page-dx"),
            number_attribute(&target, "data-cranpose-page-dy"),
        ) else {
            return;
        };
        let Some(node_id) = node_id_attribute(&target, "data-cranpose-page", &node_ids) else {
            return;
        };
        event.prevent_default();
        on_live_tree(&app, |root| {
            accessibility::scroll_by(root, node_id, sign * dx, sign * dy);
        });
    }) as Box<dyn FnMut(_)>);
    root.add_event_listener_with_callback("keydown", key_down.as_ref().unchecked_ref())?;
    key_down.forget();
    Ok(())
}

/// The mirror node a key event landed on.
fn key_target(event: &web_sys::KeyboardEvent) -> Option<Element> {
    event
        .target()
        .and_then(|target| target.dyn_into::<Element>().ok())
}

fn number_attribute(target: &Element, name: &str) -> Option<f32> {
    target
        .get_attribute(name)
        .and_then(|value| value.parse::<f32>().ok())
}

/// The live node behind the virtual id an attribute on the mirror carries.
fn node_id_attribute(
    target: &Element,
    name: &str,
    node_ids: &RefCell<HashMap<i32, cranpose_core::NodeId>>,
) -> Option<cranpose_core::NodeId> {
    target
        .get_attribute(name)
        .and_then(|value| value.parse::<i32>().ok())
        .and_then(|element_id| node_ids.borrow().get(&element_id).copied())
}

/// Runs one change against the live semantics tree, unless the app is busy
/// with its own frame.
fn on_live_tree(
    app: &Rc<RefCell<AppShell<WgpuRenderer>>>,
    act: impl FnOnce(&cranpose_ui::SemanticsNode),
) {
    if let Ok(mut shell) = app.try_borrow_mut()
        && let Some(tree) = shell.semantics_tree()
    {
        act(tree.root());
    }
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

/// Where the mirrored control sits in the Tab order: a focus target or an
/// adjustable control takes Tab, a plain button keeps the browser's default,
/// and text stays out of the way.
fn tab_index(element: &AccessibilityElement) -> &'static str {
    if element.focusable || element.adjustable {
        "0"
    } else if element.clickable {
        "auto"
    } else {
        "-1"
    }
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

/// The element that holds one mirrored control per element on screen. It
/// covers the canvas and takes no pointer of its own.
fn mirror_root(document: &Document) -> Result<HtmlElement, JsValue> {
    let root = document.create_element("div")?.dyn_into::<HtmlElement>()?;
    root.set_attribute("data-cranpose-accessibility", "")?;
    root.set_attribute("aria-label", "Application controls")?;
    let style = root.style();
    style.set_property("position", "fixed")?;
    style.set_property("inset", "0")?;
    style.set_property("z-index", "2147483647")?;
    style.set_property("pointer-events", "none")?;
    Ok(root)
}

/// Hands a screen reader's activation of a mirrored control back to the app as
/// a press at the middle of the control it stands for.
fn attach_click_listener(
    root: &HtmlElement,
    app: Rc<RefCell<AppShell<WgpuRenderer>>>,
) -> Result<(), JsValue> {
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
    Ok(())
}

/// Moves the value of an adjustable control with the arrow keys, the way a
/// screen reader and a keyboard user both expect of a slider.
fn attach_key_listener(
    root: &HtmlElement,
    app: Rc<RefCell<AppShell<WgpuRenderer>>>,
    node_ids: Rc<RefCell<HashMap<i32, cranpose_core::NodeId>>>,
) -> Result<(), JsValue> {
    let key_down = Closure::wrap(Box::new(move |event: web_sys::KeyboardEvent| {
        let Some(target) = key_target(&event) else {
            return;
        };
        let read = |name: &str| number_attribute(&target, name);
        let (Some(current), Some(min), Some(max), Some(step)) = (
            read("data-cranpose-value"),
            read("data-cranpose-min"),
            read("data-cranpose-max"),
            read("data-cranpose-step"),
        ) else {
            return;
        };
        let next = match event.key().as_str() {
            "ArrowRight" | "ArrowUp" => current + step,
            "ArrowLeft" | "ArrowDown" => current - step,
            "Home" => min,
            "End" => max,
            _ => return,
        };
        let Some(node_id) = node_id_attribute(&target, "data-cranpose-node", &node_ids) else {
            return;
        };
        event.prevent_default();
        on_live_tree(&app, |root| {
            accessibility::set_progress(root, node_id, next.clamp(min, max));
        });
    }) as Box<dyn FnMut(_)>);
    root.add_event_listener_with_callback("keydown", key_down.as_ref().unchecked_ref())?;
    key_down.forget();
    Ok(())
}

/// A text holder a screen reader watches and reads out when its text changes.
/// It sits outside the mirrored controls and stays for the life of the page:
/// the mirror is rebuilt on every change, and a live region that appears
/// together with its text is read by no reader.
fn live_region(document: &Document, politeness: &str) -> Result<HtmlElement, JsValue> {
    let region = document.create_element("div")?.dyn_into::<HtmlElement>()?;
    region.set_attribute("aria-live", politeness)?;
    region.set_attribute("aria-atomic", "true")?;
    let style = region.style();
    style.set_property("position", "fixed")?;
    style.set_property("width", "1px")?;
    style.set_property("height", "1px")?;
    style.set_property("overflow", "hidden")?;
    style.set_property("clip", "rect(0 0 0 0)")?;
    style.set_property("white-space", "nowrap")?;
    style.set_property("pointer-events", "none")?;
    Ok(region)
}

pub(crate) struct WebAccessibilityBridge {
    root: HtmlElement,
    canvas: HtmlCanvasElement,
    previous: Vec<AccessibilityElement>,
    node_ids: Rc<RefCell<HashMap<i32, cranpose_core::NodeId>>>,
    focused_element: Option<i32>,
    polite: HtmlElement,
    assertive: HtmlElement,
    announcement_turn: bool,
}

impl WebAccessibilityBridge {
    pub(crate) fn install(
        document: &Document,
        canvas: HtmlCanvasElement,
        app: Rc<RefCell<AppShell<WgpuRenderer>>>,
    ) -> Result<Self, JsValue> {
        let root = mirror_root(document)?;
        attach_click_listener(&root, Rc::clone(&app))?;

        let body = document.body().ok_or("document has no body")?;
        body.append_child(&root)?;
        let polite = live_region(document, "polite")?;
        let assertive = live_region(document, "assertive")?;
        body.append_child(&polite)?;
        body.append_child(&assertive)?;
        let node_ids: Rc<RefCell<HashMap<i32, cranpose_core::NodeId>>> =
            Rc::new(RefCell::new(HashMap::new()));
        attach_focus_listener(&root, Rc::clone(&node_ids))?;
        attach_key_listener(&root, Rc::clone(&app), Rc::clone(&node_ids))?;
        attach_page_listener(&root, app, Rc::clone(&node_ids))?;

        Ok(Self {
            root,
            canvas,
            previous: Vec::new(),
            node_ids,
            focused_element: None,
            polite,
            assertive,
            announcement_turn: false,
        })
    }

    /// Puts text a screen reader reads out into the live region that matches
    /// how urgent it is. The same text twice in a row carries a trailing space
    /// one time out of two, because a reader reads a live region only when its
    /// text changes.
    fn speak(&mut self, next: &[AccessibilityElement]) {
        let mut announcements = accessibility::drain_app_announcements();
        announcements.extend(accessibility::live_region_announcements(
            &self.previous,
            next,
        ));
        for announcement in announcements {
            self.announcement_turn = !self.announcement_turn;
            let text = if self.announcement_turn {
                format!("{} ", announcement.text)
            } else {
                announcement.text
            };
            let region = match announcement.mode {
                LiveRegionMode::Assertive => &self.assertive,
                LiveRegionMode::Polite => &self.polite,
            };
            region.set_text_content(Some(&text));
        }
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
        self.speak(&elements);
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
        let pages = page_targets(&ids, &elements);
        for ((id, element), page) in ids.into_iter().zip(elements).zip(pages) {
            let node = document
                .create_element(if element.clickable { "button" } else { "span" })?
                .dyn_into::<HtmlElement>()?;
            node.set_attribute("aria-label", &element.label)?;
            node.set_attribute("data-cranpose-node", &id.to_string())?;
            apply_role_and_state(&node, &element)?;
            apply_page(&node, page)?;
            if element.clickable {
                let (x, y) = element.bounds.center();
                node.set_attribute("data-cranpose-x", &x.to_string())?;
                node.set_attribute("data-cranpose-y", &y.to_string())?;
            }
            node.set_attribute("tabindex", tab_index(&element))?;
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
