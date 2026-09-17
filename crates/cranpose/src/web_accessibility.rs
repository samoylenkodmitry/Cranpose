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
    let role = match element.progress {
        Some(_) => "slider",
        None => element.role.aria_name(),
    };
    if !edits_text(element) {
        node.set_attribute("role", role)?;
    }
    if let Some(title) = &element.pane_title {
        node.set_attribute("role", "region")?;
        node.set_attribute("aria-label", title)?;
    }
    let scrolls = element.vertical_scroll.is_some() || element.horizontal_scroll.is_some();
    if element.label.is_empty() && scrolls {
        node.set_attribute("aria-hidden", "true")?;
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
    match element.role {
        AccessibilityRole::StaticText => node.set_text_content(Some(&element.label)),
        AccessibilityRole::TextField | AccessibilityRole::SearchField => {
            node.set_text_content(element.value.as_deref())
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

/// Whether a control is a text field a reader edits: one that publishes its
/// caret. A field that holds a secret publishes none and stays a plain node.
fn edits_text(element: &AccessibilityElement) -> bool {
    element.role.is_text_field() && element.text_selection.is_some()
}

/// Whether a field's text runs over more than one line.
fn holds_lines(element: &AccessibilityElement) -> bool {
    element
        .value
        .as_deref()
        .is_some_and(|value| value.contains('\n'))
}

/// The element a control is mirrored as: a text field is an input or a text
/// area, so a reader walks and edits its text the way it does any form
/// field; a control a click reaches is a button; anything else is a span.
fn mirror_tag(element: &AccessibilityElement) -> &'static str {
    if edits_text(element) {
        if holds_lines(element) {
            "textarea"
        } else {
            "input"
        }
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
    let anchor = accessibility::utf16_offset(value, anchor) as u32;
    let focus = accessibility::utf16_offset(value, focus) as u32;
    node.set_attribute("data-cranpose-selection", &format!("{anchor}:{focus}"))?;
    let (start, end) = (anchor.min(focus), anchor.max(focus));
    let direction = if focus < anchor {
        "backward"
    } else {
        "forward"
    };
    if let Some(input) = node.dyn_ref::<HtmlInputElement>() {
        input.set_type(if element.role == AccessibilityRole::SearchField {
            "search"
        } else {
            "text"
        });
        input.set_value(value);
        input.set_selection_range_with_direction(start, end, direction)?;
    } else if let Some(area) = node.dyn_ref::<HtmlTextAreaElement>() {
        area.set_value(value);
        area.set_selection_range_with_direction(start, end, direction)?;
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
        let _ = active.set_attribute("data-cranpose-selection", &ends);
        on_live_tree(&app, |root| {
            accessibility::set_text_selection_utf16(root, node_id, anchor, focus)
        });
    }) as Box<dyn FnMut(_)>);
    document
        .add_event_listener_with_callback("selectionchange", on_change.as_ref().unchecked_ref())?;
    on_change.forget();
    Ok(())
}

/// The one element whose text or caret changed while everything else on the
/// screen stayed the same, when that element is the focused field: the case
/// of a keystroke or a caret move, which must not rebuild the mirror, or the
/// reader would hear the whole field again instead of one character.
fn only_focused_field_changed(
    previous: &[AccessibilityElement],
    next: &[AccessibilityElement],
) -> Option<usize> {
    if previous.len() != next.len() {
        return None;
    }
    let mut changed = None;
    for (index, (before, after)) in previous.iter().zip(next).enumerate() {
        if before == after {
            continue;
        }
        if changed.is_some() || !after.focused || !edits_text(after) {
            return None;
        }
        let mut same_but_text = after.clone();
        same_but_text.label.clone_from(&before.label);
        same_but_text.value.clone_from(&before.value);
        same_but_text.text_selection = before.text_selection;
        if same_but_text != *before {
            return None;
        }
        changed = Some(index);
    }
    changed
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
    node.set_attribute("aria-label", &element.label)?;
    node.set_attribute("data-cranpose-node", &id.to_string())?;
    if let Some(language) = &element.language {
        node.set_attribute("lang", language)?;
    }
    apply_role_and_state(&node, element)?;
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
            accessibility::set_progress(root, node_id, next.clamp(min, max))
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
        attach_action_listener(&root, Rc::clone(&app), Rc::clone(&node_ids))?;
        attach_selection_listener(document, Rc::clone(&app), Rc::clone(&node_ids))?;
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
        focus_mirror_node(node)
    }

    /// Puts a keystroke or a caret move into the focused field's mirror node
    /// in place, so the browser's focus and the reader's place in the text
    /// stay where they are. Answers whether that was all that changed.
    fn patch_focused_field(&mut self, elements: &[AccessibilityElement]) -> Result<bool, JsValue> {
        let Some(index) = only_focused_field_changed(&self.previous, elements) else {
            return Ok(false);
        };
        let id = accessibility::element_ids(elements)[index];
        let selector = format!("[data-cranpose-node=\"{id}\"]");
        let Some(node) = self.root.query_selector(&selector)? else {
            return Ok(false);
        };
        let node = node.dyn_into::<HtmlElement>()?;
        let element = &elements[index];
        node.set_attribute("aria-label", &element.label)?;
        apply_field_text(&node, element)?;
        self.previous = elements.to_vec();
        Ok(true)
    }

    pub(crate) fn sync(
        &mut self,
        document: &Document,
        shell: &mut AppShell<WgpuRenderer>,
    ) -> Result<(), JsValue> {
        let elements = accessibility::snapshot(shell);
        self.speak(&elements);
        if elements == self.previous || self.patch_focused_field(&elements)? {
            return Ok(());
        }
        let opened_dialog = opened_dialog(&self.previous, &elements);
        self.previous.clone_from(&elements);
        let held = reader_focus(document).filter(|_| opened_dialog.is_none());
        let app_focus_before = self.focused_element;
        self.root.set_inner_html("");
        self.node_ids.borrow_mut().clear();

        let canvas_rect = self.canvas.get_bounding_client_rect();
        let viewport = shell.viewport_size();
        let scale_x = canvas_rect.width() / viewport.0.max(1.0) as f64;
        let scale_y = canvas_rect.height() / viewport.1.max(1.0) as f64;
        let placement = Placement {
            left: canvas_rect.left(),
            top: canvas_rect.top(),
            scale_x,
            scale_y,
        };

        let ids = accessibility::element_ids(&elements);
        let pages = page_targets(&ids, &elements);
        for ((id, element), page) in ids.into_iter().zip(elements).zip(pages) {
            let node = mirror_node(document, id, &element, page)?;
            self.node_ids.borrow_mut().insert(id, element.node_id);
            place_node(&node, &element, &placement)?;
            self.root.append_child(&node)?;
            self.append_action_buttons(document, &element, id, &placement)?;
            self.follow_app_focus(&node, &element, id)?;
            if opened_dialog == Some(element.node_id) {
                node.focus()?;
            }
        }
        self.settle_focus(held, app_focus_before)
    }

    /// One button per action the control offers, over the control it belongs
    /// to, so a reader lists "Dismiss, Milk" right after "Milk" and a keyboard
    /// reaches it with Tab. ARIA has no actions menu, no long press and no
    /// dismiss action of its own, so the long press and then the way out sit
    /// here after the actions the app named.
    fn append_action_buttons(
        &self,
        document: &Document,
        element: &AccessibilityElement,
        id: i32,
        placement: &Placement,
    ) -> Result<(), JsValue> {
        let named = accessibility::reader_actions(element).len();
        for (index, action) in accessibility::listed_actions(element)
            .into_iter()
            .enumerate()
        {
            let button = document
                .create_element("button")?
                .dyn_into::<HtmlElement>()?;
            let label = if element.label.is_empty() {
                action.to_owned()
            } else {
                format!("{action}, {}", element.label)
            };
            button.set_attribute("aria-label", &label)?;
            button.set_attribute("data-cranpose-action", &index.to_string())?;
            button.set_attribute("data-cranpose-action-named", &named.to_string())?;
            button.set_attribute("data-cranpose-action-node", &id.to_string())?;
            if let Some(key) = element.canvas_key {
                button.set_attribute("data-cranpose-canvas", &key.to_string())?;
            }
            place_node(&button, element, placement)?;
            self.root.append_child(&button)?;
        }
        Ok(())
    }

    /// Forgets an app focus that left, and puts the browser's focus back on
    /// the mirror node a reader held before the rebuild, so a page or a value
    /// change does not drop its cursor. An app that moved focus itself wins.
    fn settle_focus(
        &mut self,
        held: Option<i32>,
        app_focus_before: Option<i32>,
    ) -> Result<(), JsValue> {
        if !self.previous.iter().any(|element| element.focused) {
            self.focused_element = None;
        }
        let Some(id) = held.filter(|_| self.focused_element == app_focus_before) else {
            return Ok(());
        };
        let selector = format!("[data-cranpose-node=\"{id}\"]");
        if let Some(node) = self.root.query_selector(&selector)?
            && let Ok(node) = node.dyn_into::<HtmlElement>()
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

/// The mirror node the browser's focus sits on, by its virtual id.
fn reader_focus(document: &Document) -> Option<i32> {
    document
        .active_element()?
        .get_attribute("data-cranpose-node")?
        .parse()
        .ok()
}
