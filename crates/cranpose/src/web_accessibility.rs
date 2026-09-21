use std::{cell::RefCell, collections::HashMap, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;
use cranpose_ui::LiveRegionMode;
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{
    Document, Element, HtmlCanvasElement, HtmlElement, HtmlInputElement, HtmlTextAreaElement,
    MouseEvent,
};

use crate::accessibility::{self, AccessibilityElement, AccessibilityRole};

/// The role, value and state a screen reader reads off the mirrored element.
fn apply_role_and_state(node: &HtmlElement, element: &AccessibilityElement) -> Result<(), JsValue> {
    let role = if element.progress.is_some()
        && element.adjustable
        && element.role != AccessibilityRole::ValuePicker
    {
        "slider"
    } else {
        element.role.aria_name()
    };
    if element.role != AccessibilityRole::StaticText && !edits_text(element) {
        node.set_attribute("role", role)?;
    }
    if let Some(title) = &element.pane_title {
        if !element.role.is_named_container() {
            node.set_attribute("role", "region")?;
        }
        node.set_attribute("aria-label", title)?;
    }
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
    node.set_attribute(
        "aria-valuemin",
        &progress.start.min(progress.end).to_string(),
    )?;
    node.set_attribute(
        "aria-valuemax",
        &progress.start.max(progress.end).to_string(),
    )?;
    if let Some(text) = &element.state_description {
        node.set_attribute("aria-valuetext", text)?;
    }
    if element.adjustable && element.enabled {
        node.set_attribute("data-cranpose-value", &progress.current.to_string())?;
        node.set_attribute("data-cranpose-min", &progress.start.to_string())?;
        node.set_attribute("data-cranpose-max", &progress.end.to_string())?;
        node.set_attribute("data-cranpose-step", &progress.step().to_string())?;
    }
    Ok(())
}

/// The scroll container an element sits in: its virtual id, the move one page
/// on makes, and the last row a reader may ask the container for.
struct PageTarget {
    container: i32,
    dx: f32,
    dy: f32,
    last_row: Option<usize>,
}

fn page_targets(ids: &[i32], elements: &[AccessibilityElement]) -> Vec<Option<PageTarget>> {
    elements
        .iter()
        .map(|element| {
            let container = accessibility::scroll_container_for(elements, element)?;
            let index = elements
                .iter()
                .position(|candidate| std::ptr::eq(candidate, container))?;
            let (dx, dy) = accessibility::page_delta(container, true);
            let rows = accessibility::row_count(container);
            Some(PageTarget {
                container: *ids.get(index)?,
                dx,
                dy,
                last_row: (rows > 0).then(|| rows - 1),
            })
        })
        .collect()
}

fn apply_page(
    node: &HtmlElement,
    element: &AccessibilityElement,
    page: Option<PageTarget>,
) -> Result<(), JsValue> {
    let Some(page) = page else {
        return Ok(());
    };
    node.set_attribute("data-cranpose-page", &page.container.to_string())?;
    node.set_attribute("data-cranpose-page-dx", &page.dx.to_string())?;
    node.set_attribute("data-cranpose-page-dy", &page.dy.to_string())?;
    if let Some(last_row) = page.last_row.filter(|_| takes_home_and_end(element)) {
        node.set_attribute("data-cranpose-last-row", &last_row.to_string())?;
    }
    Ok(())
}

/// Whether Home and End on this mirror node may reach the list around it. A
/// text field moves its caret with those two keys and a slider moves its
/// value, so inside a list those two keep them.
fn takes_home_and_end(element: &AccessibilityElement) -> bool {
    !element.role.is_text_field() && !element.adjustable
}

/// Pages the scroll container around the focused mirror node on Page Down and
/// Page Up, so a keyboard reader reaches rows a lazy list has not built yet,
/// and jumps to the ends of a list on Home and End. ARIA has no scroll-to-row
/// action, so the two ends are what the mirror can offer.
fn attach_page_listener(
    root: &HtmlElement,
    app: Rc<RefCell<AppShell<WgpuRenderer>>>,
    node_ids: Rc<RefCell<HashMap<i32, cranpose_core::NodeId>>>,
) -> Result<(), JsValue> {
    let key_down = Closure::wrap(Box::new(move |event: web_sys::KeyboardEvent| {
        let Some(target) = key_target(&event) else {
            return;
        };
        let Some(node_id) = node_id_attribute(&target, "data-cranpose-page", &node_ids) else {
            return;
        };
        match event.key().as_str() {
            "PageDown" => page_mirror(&event, &target, &app, node_id, 1.0),
            "PageUp" => page_mirror(&event, &target, &app, node_id, -1.0),
            "Home" => jump_mirror(&event, &target, &app, node_id, false),
            "End" => jump_mirror(&event, &target, &app, node_id, true),
            _ => {}
        }
    }) as Box<dyn FnMut(_)>);
    root.add_event_listener_with_callback("keydown", key_down.as_ref().unchecked_ref())?;
    key_down.forget();
    Ok(())
}

/// Moves the list around the focused mirror node one page, forward or back.
fn page_mirror(
    event: &web_sys::KeyboardEvent,
    target: &Element,
    app: &Rc<RefCell<AppShell<WgpuRenderer>>>,
    node_id: cranpose_core::NodeId,
    sign: f32,
) {
    let (Some(dx), Some(dy)) = (
        number_attribute(target, "data-cranpose-page-dx"),
        number_attribute(target, "data-cranpose-page-dy"),
    ) else {
        return;
    };
    event.prevent_default();
    on_live_tree(app, |root| {
        accessibility::scroll_by(root, node_id, sign * dx, sign * dy)
    });
}

/// Puts the first or the last row of the list around the focused mirror node
/// in view, which is what Home and End mean inside a list.
fn jump_mirror(
    event: &web_sys::KeyboardEvent,
    target: &Element,
    app: &Rc<RefCell<AppShell<WgpuRenderer>>>,
    node_id: cranpose_core::NodeId,
    last: bool,
) {
    let Some(last_row) = number_attribute(target, "data-cranpose-last-row") else {
        return;
    };
    let index = if last { last_row.max(0.0) as usize } else { 0 };
    event.prevent_default();
    on_live_tree(app, |root| {
        accessibility::scroll_to_index(root, node_id, index)
    });
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

/// Runs one reader action against the live semantics tree, unless the app is
/// busy with its own frame.
fn on_live_tree(
    app: &Rc<RefCell<AppShell<WgpuRenderer>>>,
    act: impl FnOnce(&cranpose_ui::SemanticsNode) -> bool,
) {
    if let Ok(mut shell) = app.try_borrow_mut() {
        accessibility::run_reader_action(&mut shell, act);
    }
}

/// What a role asks for beyond its name: text to read, a heading level, the
/// value of a field, or the modal flag on a dialog.
fn apply_role_extras(node: &HtmlElement, element: &AccessibilityElement) -> Result<(), JsValue> {
    if is_mirror_container(element) && element.role != AccessibilityRole::Dialog {
        return Ok(());
    }
    match element.role {
        AccessibilityRole::StaticText => node.set_text_content(Some(&element.label)),
        AccessibilityRole::TextField | AccessibilityRole::SearchField => {
            node.set_text_content(element.value.as_deref())
        }
        AccessibilityRole::Header => {
            node.set_attribute("aria-level", "2")?;
            node.set_text_content(Some(&element.label));
        }
        AccessibilityRole::Dialog => node.set_attribute(
            "aria-modal",
            if element.is_modal { "true" } else { "false" },
        )?,
        _ => {}
    }
    Ok(())
}

/// What the control says about itself in words: the state description with
/// the reason its content is wrong, whether it holds a secret, and where it
/// sits in a group.
fn apply_aria_description(
    node: &HtmlElement,
    element: &AccessibilityElement,
) -> Result<(), JsValue> {
    if let Some(state) = accessibility::state_with_error(element) {
        node.set_attribute("aria-description", &state)?;
    }
    if element.password {
        node.set_attribute("aria-roledescription", "password")?;
    }
    if element.error.is_some() {
        node.set_attribute("aria-invalid", "true")?;
    }
    if let Some(item) = element.collection_item {
        node.set_attribute("aria-posinset", &item.position.to_string())?;
        node.set_attribute("aria-setsize", &item.count.to_string())?;
    }
    Ok(())
}

/// The checked or selected flag, and whether the control is disabled.
fn apply_aria_state(node: &HtmlElement, element: &AccessibilityElement) -> Result<(), JsValue> {
    apply_aria_description(node, element)?;
    if let Some(expanded) = element.expanded {
        node.set_attribute("aria-expanded", if expanded { "true" } else { "false" })?;
    }
    if let Some(toggled) = element.toggled {
        let flag = if element.role == AccessibilityRole::ToggleButton {
            "aria-pressed"
        } else {
            "aria-checked"
        };
        node.set_attribute(flag, if toggled { "true" } else { "false" })?;
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

fn edits_text(element: &AccessibilityElement) -> bool {
    element.role.is_text_field() && (element.text_selection.is_some() || element.password)
}

fn is_mirror_container(element: &AccessibilityElement) -> bool {
    element.role.is_named_container()
        || element.role == AccessibilityRole::Dialog
        || element.vertical_scroll.is_some()
        || element.horizontal_scroll.is_some()
        || element.pane_title.is_some()
}

/// The element a control is mirrored as: a text field is an input or a text
/// area, so a reader walks and edits its text the way it does any form
/// field; a control a click reaches is a button; anything else is a span.
fn mirror_tag(element: &AccessibilityElement) -> &'static str {
    if edits_text(element) {
        if element.multiline && !element.password {
            "textarea"
        } else {
            "input"
        }
    } else if is_mirror_container(element) {
        "div"
    } else if element.clickable {
        "button"
    } else {
        "span"
    }
}

/// The text a field holds, as the value of its mirrored input, and where its
/// caret or its picked stretch of text sits, in the UTF-16 units a browser
/// counts. The same ends go on the node, so the selection listener can tell
/// a move the app made from one the reader made.
fn apply_field_text(node: &HtmlElement, element: &AccessibilityElement) -> Result<(), JsValue> {
    let (Some(value), Some((anchor, focus))) = (&element.value, element.text_selection) else {
        return Ok(());
    };
    apply_editor_text(node, value, anchor, focus)
}

fn apply_editor_text(
    node: &HtmlElement,
    value: &str,
    anchor: usize,
    focus: usize,
) -> Result<(), JsValue> {
    if node.has_attribute("data-cranpose-composition") {
        return Ok(());
    }
    let anchor = accessibility::utf16_offset(value, anchor) as u32;
    let focus = accessibility::utf16_offset(value, focus) as u32;
    let ends = format!("{anchor}:{focus}");
    let previous = node.get_attribute("data-cranpose-selection");
    let selection_changed = previous.as_deref() != Some(ends.as_str());
    let native_selection = field_selection(node).map(|(anchor, focus)| format!("{anchor}:{focus}"));
    let pending_selection = previous.is_some()
        && node.matches(":focus")?
        && native_selection.as_ref() != previous.as_ref();
    let value_changed = field_value(node).as_deref() != Some(value);
    if value_changed {
        if let Some(input) = node.dyn_ref::<HtmlInputElement>() {
            input.set_value(value);
        } else if let Some(area) = node.dyn_ref::<HtmlTextAreaElement>() {
            area.set_value(value);
        }
    }
    node.set_attribute("data-cranpose-selection", &ends)?;
    if value_changed || (selection_changed && !pending_selection) {
        restore_field_caret(node)?;
    }
    Ok(())
}

/// Puts the caret of a mirrored field back where the app last published it,
/// after the browser's focus moved onto the field.
fn restore_field_caret(node: &HtmlElement) -> Result<(), JsValue> {
    let Some(ends) = node.get_attribute("data-cranpose-selection") else {
        return Ok(());
    };
    let Some((anchor, focus)) = ends.split_once(':').and_then(|(anchor, focus)| {
        Some((anchor.parse::<u32>().ok()?, focus.parse::<u32>().ok()?))
    }) else {
        return Ok(());
    };
    let (start, end) = (anchor.min(focus), anchor.max(focus));
    let direction = if focus < anchor {
        "backward"
    } else {
        "forward"
    };
    if let Some(input) = node.dyn_ref::<HtmlInputElement>() {
        input.set_selection_range_with_direction(start, end, direction)?;
    } else if let Some(area) = node.dyn_ref::<HtmlTextAreaElement>() {
        area.set_selection_range_with_direction(start, end, direction)?;
    }
    Ok(())
}

/// Moves the browser's focus onto a mirror node, and for a field puts the
/// caret back where it was, because a fresh input starts with its caret at
/// the start.
fn focus_mirror_node(node: &HtmlElement) -> Result<(), JsValue> {
    if node.matches(":focus")? {
        return Ok(());
    }
    node.focus()?;
    restore_field_caret(node)
}

/// The two ends of the selection in a mirrored field, the anchor first, in
/// UTF-16 units, or nothing for a node that is not a field.
fn field_selection(element: &Element) -> Option<(usize, usize)> {
    let (start, end, direction) = if let Some(input) = element.dyn_ref::<HtmlInputElement>() {
        (
            input.selection_start().ok()??,
            input.selection_end().ok()??,
            input.selection_direction().ok()??,
        )
    } else {
        let area = element.dyn_ref::<HtmlTextAreaElement>()?;
        (
            area.selection_start().ok()??,
            area.selection_end().ok()??,
            area.selection_direction().ok()??,
        )
    };
    let (start, end) = (start as usize, end as usize);
    Some(if direction == "backward" {
        (end, start)
    } else {
        (start, end)
    })
}

/// Hands the app the caret a reader or a keyboard moved inside a mirrored
/// field, through the browser's own selection change. A change that only
/// echoes the ends the app published is not sent back.
fn attach_selection_listener(
    document: &Document,
    app: Rc<RefCell<AppShell<WgpuRenderer>>>,
    node_ids: Rc<RefCell<HashMap<i32, cranpose_core::NodeId>>>,
) -> Result<(), JsValue> {
    let owner = document.clone();
    let on_change = Closure::wrap(Box::new(move |_event: web_sys::Event| {
        let Some(active) = owner.active_element() else {
            return;
        };
        if active.has_attribute("data-cranpose-composition") {
            return;
        }
        let Some((anchor, focus)) = field_selection(&active) else {
            return;
        };
        let ends = format!("{anchor}:{focus}");
        if active.get_attribute("data-cranpose-selection").as_deref() == Some(ends.as_str()) {
            return;
        }
        let Some(node_id) = node_id_attribute(&active, "data-cranpose-node", &node_ids) else {
            return;
        };
        let Some(value) = field_value(&active) else {
            return;
        };
        let _ = active.set_attribute("data-cranpose-selection", &ends);
        on_live_tree(&app, |root| {
            accessibility::set_text_selection(
                root,
                node_id,
                accessibility::byte_offset_for_utf16(&value, anchor),
                accessibility::byte_offset_for_utf16(&value, focus),
            )
        });
    }) as Box<dyn FnMut(_)>);
    document
        .add_event_listener_with_callback("selectionchange", on_change.as_ref().unchecked_ref())?;
    on_change.forget();
    Ok(())
}

/// One mirror node for a control: a text field as an input, a button when a
/// click reaches it, a span otherwise, carrying its label, role, state and
/// paging data.
fn mirror_node(
    document: &Document,
    id: i32,
    element: &AccessibilityElement,
    page: Option<PageTarget>,
) -> Result<HtmlElement, JsValue> {
    let node = document
        .create_element(mirror_tag(element))?
        .dyn_into::<HtmlElement>()?;
    if element.role != AccessibilityRole::StaticText {
        node.set_attribute("aria-label", &element.label)?;
    }
    if !element.enabled {
        node.set_attribute("disabled", "")?;
    }
    node.set_attribute("data-cranpose-node", &id.to_string())?;
    if let Some(language) = &element.language {
        node.set_attribute("lang", language)?;
    }
    apply_role_and_state(&node, element)?;
    if let Some(input) = node.dyn_ref::<HtmlInputElement>() {
        input.set_type(if element.password {
            "password"
        } else if element.role == AccessibilityRole::SearchField {
            "search"
        } else {
            "text"
        });
    }
    apply_page(&node, element, page)?;
    apply_field_text(&node, element)?;
    if element.clickable {
        let (x, y) = element.bounds.center();
        node.set_attribute("data-cranpose-x", &x.to_string())?;
        node.set_attribute("data-cranpose-y", &y.to_string())?;
    }
    node.set_attribute("tabindex", tab_index(element))?;
    Ok(node)
}

/// Where the mirrored control sits in the Tab order: a focus target or an
/// adjustable control takes Tab, a plain button keeps the browser's default,
/// and text stays out of the way.
fn tab_index(element: &AccessibilityElement) -> &'static str {
    if element.enabled
        && element.tab_stop
        && (element.focusable || element.adjustable || element.clickable)
    {
        "0"
    } else {
        "-1"
    }
}

/// Puts the mirrored element over the control it stands for, so a reader's
/// cursor and a touch exploration land in the same place.
/// Where the canvas sits on the page and how its logical pixels map onto it.
struct Placement {
    left: f64,
    top: f64,
    scale_x: f64,
    scale_y: f64,
}

fn place_node(
    node: &HtmlElement,
    element: &AccessibilityElement,
    placement: &Placement,
) -> Result<(), JsValue> {
    let Placement {
        left,
        top,
        scale_x,
        scale_y,
    } = *placement;
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
    app: Rc<RefCell<AppShell<WgpuRenderer>>>,
    node_ids: Rc<RefCell<HashMap<i32, cranpose_core::NodeId>>>,
) -> Result<(), JsValue> {
    let blur_app = Rc::clone(&app);
    let focus_out = Closure::wrap(Box::new(move |event: web_sys::Event| {
        let Some(target) = event
            .target()
            .and_then(|target| target.dyn_into::<Element>().ok())
            .filter(|target| field_value(target).is_some())
        else {
            return;
        };
        let _ = target.remove_attribute("data-cranpose-composition");
        if let Ok(mut shell) = blur_app.try_borrow_mut() {
            shell.on_ime_finish_composing();
            shell.clear_text_field_focus();
        }
    }) as Box<dyn FnMut(_)>);
    root.add_event_listener_with_callback("focusout", focus_out.as_ref().unchecked_ref())?;
    focus_out.forget();
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
        on_live_tree(&app, |root| accessibility::focus_node(root, node_id));
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
            shell.accessibility_activate_at(x, y);
        }
    }) as Box<dyn FnMut(_)>);
    root.add_event_listener_with_callback("click", click.as_ref().unchecked_ref())?;
    click.forget();
    Ok(())
}

/// Runs the action behind an action button, on the live tree: one the app
/// named, or the way out that sits after them.
fn attach_action_listener(
    root: &HtmlElement,
    app: Rc<RefCell<AppShell<WgpuRenderer>>>,
    node_ids: Rc<RefCell<HashMap<i32, cranpose_core::NodeId>>>,
) -> Result<(), JsValue> {
    let click = Closure::wrap(Box::new(move |event: MouseEvent| {
        let Some(target) = event
            .target()
            .and_then(|target| target.dyn_into::<Element>().ok())
        else {
            return;
        };
        let Some(index) = target
            .get_attribute("data-cranpose-action")
            .and_then(|value| value.parse::<usize>().ok())
        else {
            return;
        };
        let Some(named) = target
            .get_attribute("data-cranpose-action-named")
            .and_then(|value| value.parse::<usize>().ok())
        else {
            return;
        };
        let Some(node_id) = node_id_attribute(&target, "data-cranpose-action-node", &node_ids)
        else {
            return;
        };
        let canvas_key = target
            .get_attribute("data-cranpose-canvas")
            .and_then(|value| value.parse::<u64>().ok());
        on_live_tree(&app, |root| {
            accessibility::perform_listed_action(root, node_id, canvas_key, named, index)
        });
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
        if browser_handles_key(&event, &target) {
            event.stop_propagation();
            return;
        }
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
        event.stop_propagation();
        on_live_tree(&app, |root| {
            accessibility::set_progress(root, node_id, next.clamp(min, max))
        });
    }) as Box<dyn FnMut(_)>);
    root.add_event_listener_with_callback("keydown", key_down.as_ref().unchecked_ref())?;
    key_down.forget();
    let key_up = Closure::wrap(Box::new(move |event: web_sys::KeyboardEvent| {
        if key_target(&event).is_some_and(|target| browser_handles_key(&event, &target)) {
            event.stop_propagation();
        }
    }) as Box<dyn FnMut(_)>);
    root.add_event_listener_with_callback("keyup", key_up.as_ref().unchecked_ref())?;
    key_up.forget();
    Ok(())
}

fn browser_handles_key(event: &web_sys::KeyboardEvent, target: &Element) -> bool {
    if event.is_composing()
        || event.key_code() == 229
        || target.has_attribute("data-cranpose-composition")
    {
        return true;
    }
    let key = event.key();
    if key == "Tab" {
        return target
            .closest("[aria-modal=\"true\"]")
            .ok()
            .flatten()
            .is_none();
    }
    (key != "Escape"
        && (target.is_instance_of::<HtmlInputElement>()
            || target.is_instance_of::<HtmlTextAreaElement>()))
        || (target.tag_name() == "BUTTON" && matches!(key.as_str(), "Enter" | " "))
}

fn live_region(document: &Document, politeness: &str) -> Result<HtmlElement, JsValue> {
    let region = document.create_element("div")?.dyn_into::<HtmlElement>()?;
    region.set_attribute("aria-live", politeness)?;
    region.set_attribute("aria-atomic", "true")?;
    region.set_attribute("data-cranpose-live", politeness)?;
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

fn attach_input_listener(
    root: &HtmlElement,
    app: Rc<RefCell<AppShell<WgpuRenderer>>>,
    node_ids: Rc<RefCell<HashMap<i32, cranpose_core::NodeId>>>,
) -> Result<(), JsValue> {
    let input = Closure::wrap(Box::new(move |event: web_sys::Event| {
        let Some(target) = event
            .target()
            .and_then(|target| target.dyn_into::<Element>().ok())
        else {
            return;
        };
        sync_field_input(&target, &app, &node_ids);
    }) as Box<dyn FnMut(_)>);
    root.add_event_listener_with_callback("input", input.as_ref().unchecked_ref())?;
    input.forget();
    Ok(())
}

fn field_value(target: &Element) -> Option<String> {
    if let Some(input) = target.dyn_ref::<HtmlInputElement>() {
        Some(input.value())
    } else {
        target
            .dyn_ref::<HtmlTextAreaElement>()
            .map(HtmlTextAreaElement::value)
    }
}

fn composition_range(target: &Element) -> Option<(usize, usize)> {
    let range = target.get_attribute("data-cranpose-composition")?;
    let (start, end) = range.split_once(':')?;
    Some((start.parse().ok()?, end.parse().ok()?))
}

fn sync_field_input(
    target: &Element,
    app: &Rc<RefCell<AppShell<WgpuRenderer>>>,
    node_ids: &RefCell<HashMap<i32, cranpose_core::NodeId>>,
) {
    let Some(node_id) = node_id_attribute(target, "data-cranpose-node", node_ids) else {
        return;
    };
    let Some(value) = field_value(target) else {
        return;
    };
    let selection = field_selection(target);
    if let Some((anchor, focus)) = selection {
        let _ = target.set_attribute("data-cranpose-selection", &format!("{anchor}:{focus}"));
    }
    on_live_tree(app, |root| {
        let changed = accessibility::set_text(root, node_id, &value);
        if let Some((anchor, focus)) = selection {
            let anchor = accessibility::byte_offset_for_utf16(&value, anchor);
            let focus = accessibility::byte_offset_for_utf16(&value, focus);
            return accessibility::set_text_selection(root, node_id, anchor, focus) || changed;
        }
        changed
    });
    if let Some((start, end)) = composition_range(target)
        && let Ok(mut shell) = app.try_borrow_mut()
    {
        shell.on_ime_set_composing_region(
            accessibility::byte_offset_for_utf16(&value, start),
            accessibility::byte_offset_for_utf16(&value, end),
        );
    }
}

fn attach_composition_listener(
    root: &HtmlElement,
    app: Rc<RefCell<AppShell<WgpuRenderer>>>,
    node_ids: Rc<RefCell<HashMap<i32, cranpose_core::NodeId>>>,
) -> Result<(), JsValue> {
    for name in ["compositionstart", "compositionupdate", "compositionend"] {
        let app = Rc::clone(&app);
        let node_ids = Rc::clone(&node_ids);
        let listener = Closure::wrap(Box::new(move |event: web_sys::CompositionEvent| {
            let Some(target) = event
                .target()
                .and_then(|target| target.dyn_into::<Element>().ok())
            else {
                return;
            };
            let Some((anchor, focus)) = field_selection(&target) else {
                return;
            };
            if name == "compositionend" {
                let _ = target.remove_attribute("data-cranpose-composition");
                sync_field_input(&target, &app, &node_ids);
                if let Ok(mut shell) = app.try_borrow_mut() {
                    shell.on_ime_finish_composing();
                }
            } else {
                let start = composition_range(&target).map_or(anchor.min(focus), |range| range.0);
                let end = start + event.data().unwrap_or_default().encode_utf16().count();
                let _ =
                    target.set_attribute("data-cranpose-composition", &format!("{start}:{end}"));
            }
        }) as Box<dyn FnMut(_)>);
        root.add_event_listener_with_callback(name, listener.as_ref().unchecked_ref())?;
        listener.forget();
    }
    for name in ["copy", "cut", "paste"] {
        let listener = Closure::wrap(Box::new(move |event: web_sys::Event| {
            if event
                .target()
                .and_then(|target| target.dyn_into::<Element>().ok())
                .is_some_and(|target| field_value(&target).is_some())
            {
                event.stop_propagation();
            }
        }) as Box<dyn FnMut(_)>);
        root.add_event_listener_with_callback(name, listener.as_ref().unchecked_ref())?;
        listener.forget();
    }
    Ok(())
}

#[derive(Default)]
struct WebTextInput {
    fields: RefCell<HashMap<cranpose_core::NodeId, HtmlElement>>,
    active: RefCell<Option<HtmlElement>>,
}

impl cranpose_app_shell::PlatformTextInputHandler for WebTextInput {
    fn show_keyboard(&self) {
        let Some(node_id) = cranpose_ui::text_field_focus::focused_field_node() else {
            return;
        };
        let Some(node) = self.fields.borrow().get(&node_id).cloned() else {
            return;
        };
        if let Some(editor) = cranpose_ui::text_field_focus::focused_editor_state()
            && field_value(&node).as_deref() != Some(&editor.text)
        {
            let _ = apply_editor_text(
                &node,
                &editor.text,
                editor.selection_start,
                editor.selection_end,
            );
        }
        *self.active.borrow_mut() = Some(node.clone());
        let _ = focus_mirror_node(&node);
    }

    fn hide_keyboard(&self) {
        let active = self.active.borrow_mut().take();
        if let Some(node) = active {
            let _ = node.blur();
        }
    }
}

struct MirrorEntry {
    node: HtmlElement,
    actions: Vec<HtmlElement>,
}

impl MirrorEntry {
    fn update(
        &mut self,
        document: &Document,
        id: i32,
        element: &AccessibilityElement,
        page: Option<PageTarget>,
        placement: &Placement,
    ) -> Result<(), JsValue> {
        let template = mirror_node(document, id, element, page)?;
        if self.node.tag_name() != template.tag_name() {
            self.node.remove();
            self.node = template;
        } else {
            patch_attributes(&self.node, &template)?;
            if !is_mirror_container(element)
                && matches!(
                    element.role,
                    AccessibilityRole::StaticText | AccessibilityRole::Header
                )
                && self.node.text_content() != template.text_content()
            {
                self.node
                    .set_text_content(template.text_content().as_deref());
            }
            apply_field_text(&self.node, element)?;
        }
        place_node(&self.node, element, placement)?;
        self.update_actions(document, id, element, placement)
    }

    fn update_actions(
        &mut self,
        document: &Document,
        id: i32,
        element: &AccessibilityElement,
        placement: &Placement,
    ) -> Result<(), JsValue> {
        let named = accessibility::reader_actions(element).len();
        let actions = accessibility::listed_actions(element);
        for (index, action) in actions.iter().enumerate() {
            if index == self.actions.len() {
                self.actions.push(
                    document
                        .create_element("button")?
                        .dyn_into::<HtmlElement>()?,
                );
            }
            let button = &self.actions[index];
            let label = if element.label.is_empty() {
                action.clone()
            } else {
                format!("{action}, {}", element.label)
            };
            button.set_attribute("aria-label", &label)?;
            button.set_attribute("data-cranpose-action", &index.to_string())?;
            button.set_attribute("data-cranpose-action-named", &named.to_string())?;
            button.set_attribute("data-cranpose-action-node", &id.to_string())?;
            if let Some(key) = element.canvas_key {
                button.set_attribute("data-cranpose-canvas", &key.to_string())?;
            } else {
                button.remove_attribute("data-cranpose-canvas")?;
            }
            place_node(button, element, placement)?;
        }
        for button in self.actions.drain(actions.len()..) {
            button.remove();
        }
        Ok(())
    }
}

fn patch_attributes(node: &HtmlElement, template: &HtmlElement) -> Result<(), JsValue> {
    for name in node
        .get_attribute_names()
        .iter()
        .filter_map(|name| name.as_string())
    {
        if name != "style"
            && name != "data-cranpose-composition"
            && name != "data-cranpose-selection"
            && !template.has_attribute(&name)
        {
            node.remove_attribute(&name)?;
        }
    }
    for name in template
        .get_attribute_names()
        .iter()
        .filter_map(|name| name.as_string())
    {
        if name != "data-cranpose-selection"
            && let Some(value) = template.get_attribute(&name)
            && node.get_attribute(&name).as_ref() != Some(&value)
        {
            node.set_attribute(&name, &value)?;
        }
    }
    Ok(())
}

fn reconcile_children(parent: &HtmlElement, children: &[HtmlElement]) -> Result<(), JsValue> {
    let mut cursor = parent.first_child();
    for child in children {
        if cursor
            .as_ref()
            .is_some_and(|cursor| cursor.is_same_node(Some(child)))
        {
            cursor = child.next_sibling();
        } else {
            parent.insert_before(child, cursor.as_ref())?;
        }
    }
    Ok(())
}

pub(crate) struct WebAccessibilityBridge {
    root: HtmlElement,
    canvas: HtmlCanvasElement,
    previous: Vec<AccessibilityElement>,
    entries: HashMap<i32, MirrorEntry>,
    node_ids: Rc<RefCell<HashMap<i32, cranpose_core::NodeId>>>,
    text_input: Rc<WebTextInput>,
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
        attach_focus_listener(&root, Rc::clone(&app), Rc::clone(&node_ids))?;
        attach_key_listener(&root, Rc::clone(&app), Rc::clone(&node_ids))?;
        attach_action_listener(&root, Rc::clone(&app), Rc::clone(&node_ids))?;
        attach_selection_listener(document, Rc::clone(&app), Rc::clone(&node_ids))?;
        attach_input_listener(&root, Rc::clone(&app), Rc::clone(&node_ids))?;
        attach_composition_listener(&root, Rc::clone(&app), Rc::clone(&node_ids))?;
        attach_page_listener(&root, app, Rc::clone(&node_ids))?;

        Ok(Self {
            root,
            canvas,
            previous: Vec::new(),
            entries: HashMap::new(),
            node_ids,
            text_input: Rc::default(),
            focused_element: None,
            polite,
            assertive,
            announcement_turn: false,
        })
    }

    pub(crate) fn text_input_handler(
        &self,
    ) -> Rc<dyn cranpose_app_shell::PlatformTextInputHandler> {
        self.text_input.clone()
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
        announcements.extend(accessibility::pane_title_announcements(
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
        if edits_text(element) {
            *self.text_input.active.borrow_mut() = Some(node.clone());
        }
        focus_mirror_node(node)
    }

    pub(crate) fn sync(
        &mut self,
        document: &Document,
        shell: &mut AppShell<WgpuRenderer>,
    ) -> Result<(), JsValue> {
        let elements = accessibility::snapshot(shell);
        self.speak(&elements);
        if elements == self.previous {
            return self.sync_password(shell, &elements);
        }
        let opened_dialog = opened_dialog(&self.previous, &elements);
        let held = reader_focus(document).filter(|_| opened_dialog.is_none());
        let app_focus_before = self.focused_element;
        let canvas_rect = self.canvas.get_bounding_client_rect();
        let viewport = shell.viewport_size();
        let placement = Placement {
            left: canvas_rect.left(),
            top: canvas_rect.top(),
            scale_x: canvas_rect.width() / viewport.0.max(1.0) as f64,
            scale_y: canvas_rect.height() / viewport.1.max(1.0) as f64,
        };
        self.reconcile(document, &elements, &placement)?;
        self.sync_password(shell, &elements)?;
        for (id, element) in accessibility::element_ids(&elements)
            .into_iter()
            .zip(&elements)
        {
            let node = self.entries[&id].node.clone();
            self.follow_app_focus(&node, element, id)?;
            if opened_dialog == Some(element.node_id) {
                node.focus()?;
            }
        }
        self.previous = elements;
        self.settle_focus(held, app_focus_before)
    }

    fn sync_password(
        &self,
        shell: &mut AppShell<WgpuRenderer>,
        elements: &[AccessibilityElement],
    ) -> Result<(), JsValue> {
        let mut passwords = elements
            .iter()
            .filter(|element| element.password)
            .peekable();
        if passwords.peek().is_none() {
            return Ok(());
        }
        let Some(tree) = shell.semantics_tree() else {
            return Ok(());
        };
        let fields = self.text_input.fields.borrow();
        for element in passwords {
            if let Some(node) = fields.get(&element.node_id)
                && let Some(field) =
                    accessibility::find_semantics_node(tree.root(), element.node_id)
                && let (Some(text), Some(selection)) = (&field.text, field.text_selection)
            {
                apply_editor_text(node, text, selection.start, selection.end)?;
            }
        }
        Ok(())
    }

    fn reconcile(
        &mut self,
        document: &Document,
        elements: &[AccessibilityElement],
        placement: &Placement,
    ) -> Result<(), JsValue> {
        let ids = accessibility::element_ids(elements);
        let pages = page_targets(&ids, elements);
        let parents: HashMap<_, _> = ids
            .iter()
            .zip(elements)
            .filter(|(_, element)| element.canvas_key.is_none())
            .map(|(id, element)| (element.node_id, *id))
            .collect();
        let mut children: HashMap<Option<i32>, Vec<HtmlElement>> = HashMap::new();
        self.node_ids.borrow_mut().clear();
        self.text_input.fields.borrow_mut().clear();
        for ((id, element), page) in ids.iter().copied().zip(elements).zip(pages) {
            let entry = match self.entries.entry(id) {
                std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::hash_map::Entry::Vacant(entry) => entry.insert(MirrorEntry {
                    node: document
                        .create_element(mirror_tag(element))?
                        .dyn_into::<HtmlElement>()?,
                    actions: Vec::new(),
                }),
            };
            entry.update(document, id, element, page, placement)?;
            self.node_ids.borrow_mut().insert(id, element.node_id);
            if edits_text(element) {
                self.text_input
                    .fields
                    .borrow_mut()
                    .insert(element.node_id, entry.node.clone());
            }
            let parent = element
                .scroll_parent
                .and_then(|parent| parents.get(&parent).copied());
            let siblings = children.entry(parent).or_default();
            siblings.push(entry.node.clone());
            siblings.extend(entry.actions.iter().cloned());
        }
        for (parent, children) in children {
            let parent = parent.map_or(&self.root, |id| &self.entries[&id].node);
            reconcile_children(parent, &children)?;
        }
        self.entries.retain(|id, entry| {
            if ids.contains(id) {
                true
            } else {
                entry.node.remove();
                for button in &entry.actions {
                    button.remove();
                }
                false
            }
        });
        Ok(())
    }

    fn settle_focus(
        &mut self,
        held: Option<String>,
        app_focus_before: Option<i32>,
    ) -> Result<(), JsValue> {
        if !self.previous.iter().any(|element| element.focused) {
            self.focused_element = None;
        }
        let Some(selector) = held.filter(|_| self.focused_element == app_focus_before) else {
            return Ok(());
        };
        if let Some(node) = self.root.query_selector(&selector)?
            && let Ok(node) = node.dyn_into::<HtmlElement>()
            && !node.matches(":focus")?
        {
            focus_mirror_node(&node)?;
        }
        Ok(())
    }
}

/// The node id of a dialog that is in the next snapshot and was not in the
/// current one: the node a reader's cursor should land on.
fn opened_dialog(
    current: &[AccessibilityElement],
    next: &[AccessibilityElement],
) -> Option<cranpose_core::NodeId> {
    next.iter()
        .find(|element| {
            element.role == AccessibilityRole::Dialog
                && !current.iter().any(|old| {
                    old.node_id == element.node_id && old.role == AccessibilityRole::Dialog
                })
        })
        .map(|element| element.node_id)
}

fn reader_focus(document: &Document) -> Option<String> {
    let node = document.active_element()?;
    if let Some(id) = node.get_attribute("data-cranpose-node") {
        let id = id.parse::<i32>().ok()?;
        Some(format!("[data-cranpose-node=\"{id}\"]"))
    } else {
        let id = node
            .get_attribute("data-cranpose-action-node")?
            .parse::<i32>()
            .ok()?;
        let index = node
            .get_attribute("data-cranpose-action")?
            .parse::<usize>()
            .ok()?;
        Some(format!(
            "[data-cranpose-action-node=\"{id}\"][data-cranpose-action=\"{index}\"]"
        ))
    }
}
