use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    fmt::Write,
    rc::Rc,
};

use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;
use cranpose_ui::LiveRegionMode;
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{
    Document, Element, HtmlCanvasElement, HtmlElement, HtmlInputElement, HtmlTextAreaElement,
    MouseEvent,
};
use web_time::Instant;

use crate::{
    accessibility::{self, AccessibilityElement, AccessibilityRect, Replaced, edits_text},
    accessibility_publish_policy::AccessibilityPublishPolicy,
    web_accessibility_attributes::{
        MirrorAttributes, PageTarget, mirror_tag, mirror_text, page_targets,
    },
    web_accessibility_order::ChildMoves,
};

/// The class every mirror node and action button carries. One rule in the
/// mirror's style sheet gives them what they share: they sit over the canvas,
/// show nothing and take no pointer. Each node's own style holds only where
/// it sits.
const MIRROR_CLASS: &str = "cranpose-mirror";

/// What the mirror's listeners and the bridge share: the live node behind
/// each virtual id, and what a person did through the mirror since the
/// bridge last looked at the app.
#[derive(Default)]
struct MirrorLinks {
    node_ids: RefCell<HashMap<i32, cranpose_core::NodeId>>,
    /// A reader acted through the mirror, so changes show at the interactive
    /// interval for a while.
    read: Cell<bool>,
    /// A person typed in a mirrored field or moved its caret, so the mirror
    /// shows the field as the app holds it at once.
    edited: Cell<bool>,
}

/// Pages the scroll container around the focused mirror node on Page Down and
/// Page Up, so a keyboard reader reaches rows a lazy list has not built yet,
/// and jumps to the ends of a list on Home and End. ARIA has no scroll-to-row
/// action, so the two ends are what the mirror can offer.
fn attach_page_listener(
    root: &HtmlElement,
    app: Rc<RefCell<AppShell<WgpuRenderer>>>,
    links: Rc<MirrorLinks>,
) -> Result<(), JsValue> {
    let key_down = Closure::wrap(Box::new(move |event: web_sys::KeyboardEvent| {
        let Some(target) = key_target(&event) else {
            return;
        };
        let Some(node_id) = node_id_attribute(&target, "data-cranpose-page", &links) else {
            return;
        };
        match event.key().as_str() {
            "PageDown" => page_mirror(&event, &target, &app, &links, node_id, 1.0),
            "PageUp" => page_mirror(&event, &target, &app, &links, node_id, -1.0),
            "Home" => jump_mirror(&event, &target, &app, &links, node_id, false),
            "End" => jump_mirror(&event, &target, &app, &links, node_id, true),
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
    links: &MirrorLinks,
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
    on_live_tree(app, links, |root| {
        accessibility::scroll_by(root, node_id, sign * dx, sign * dy)
    });
}

/// Puts the first or the last row of the list around the focused mirror node
/// in view, which is what Home and End mean inside a list.
fn jump_mirror(
    event: &web_sys::KeyboardEvent,
    target: &Element,
    app: &Rc<RefCell<AppShell<WgpuRenderer>>>,
    links: &MirrorLinks,
    node_id: cranpose_core::NodeId,
    last: bool,
) {
    let Some(last_row) = number_attribute(target, "data-cranpose-last-row") else {
        return;
    };
    let index = if last { last_row.max(0.0) as usize } else { 0 };
    event.prevent_default();
    on_live_tree(app, links, |root| {
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
    links: &MirrorLinks,
) -> Option<cranpose_core::NodeId> {
    target
        .get_attribute(name)
        .and_then(|value| value.parse::<i32>().ok())
        .and_then(|element_id| links.node_ids.borrow().get(&element_id).copied())
}

/// Runs one reader action against the live semantics tree, unless the app is
/// busy with its own frame.
fn on_live_tree(
    app: &Rc<RefCell<AppShell<WgpuRenderer>>>,
    links: &MirrorLinks,
    act: impl FnOnce(&cranpose_ui::SemanticsNode) -> bool,
) {
    if let Ok(mut shell) = app.try_borrow_mut() {
        links.read.set(true);
        accessibility::run_reader_action(&mut shell, act);
    }
}

/// The text a field holds, as the value of its mirrored input, and where its
/// caret or its picked stretch of text sits, in the UTF-16 units a browser
/// counts. The same ends go on the node, so the selection listener can tell
/// a move the app made from one the reader made.
fn apply_field_text(node: &HtmlElement, element: &AccessibilityElement) -> Result<(), JsValue> {
    let (Some(value), Some((anchor, focus))) = (&element.value, element.details().text_selection)
    else {
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
    links: Rc<MirrorLinks>,
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
        let Some(node_id) = node_id_attribute(&active, "data-cranpose-node", &links) else {
            return;
        };
        let Some(value) = field_value(&active) else {
            return;
        };
        let _ = active.set_attribute("data-cranpose-selection", &ends);
        links.edited.set(true);
        on_live_tree(&app, &links, |root| {
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

fn mirror_element(document: &Document, tag: &str) -> Result<HtmlElement, JsValue> {
    let node = document.create_element(tag)?.dyn_into::<HtmlElement>()?;
    node.set_class_name(MIRROR_CLASS);
    Ok(node)
}

/// A new mirror node for a control, with every attribute `attributes`
/// describes for it, its text and the text of a field written once.
fn mirror_node(
    document: &Document,
    id: i32,
    element: &AccessibilityElement,
    page: Option<PageTarget>,
    attributes: &mut MirrorAttributes,
) -> Result<HtmlElement, JsValue> {
    let node = mirror_element(document, mirror_tag(element))?;
    attributes.describe(id, element, page);
    for (name, value) in attributes.iter() {
        node.set_attribute(name, value)?;
    }
    if let Some(text) = mirror_text(element).filter(|text| !text.is_empty()) {
        node.set_text_content(Some(text));
    }
    apply_field_text(&node, element)?;
    Ok(node)
}

/// Gives a mirror node that holds no other node the text `text`, through the
/// text node it holds when it holds one.
fn write_text(node: &HtmlElement, text: Option<&str>) {
    match (node.first_child(), text) {
        (Some(child), Some(text)) => child.set_node_value(Some(text)),
        (_, text) => node.set_text_content(text),
    }
}

/// The style sheet with the rule every mirror node and action button
/// shares. The declarations are marked important, so a page rule that
/// matches a button or a span does not show the mirror.
fn mirror_style(document: &Document) -> Result<Element, JsValue> {
    let style = document.create_element("style")?;
    style.set_text_content(Some(&format!(
        ".{MIRROR_CLASS}{{position:fixed!important;opacity:0.001!important;\
         pointer-events:none!important;overflow:hidden!important}}"
    )));
    Ok(style)
}

/// Where the canvas sits on the page and how its logical pixels map onto it.
struct Placement {
    left: f64,
    top: f64,
    scale_x: f64,
    scale_y: f64,
}

impl Placement {
    /// Where the mirror node for a control with these bounds goes: its left,
    /// top, width and height in CSS pixels.
    fn rect(&self, bounds: AccessibilityRect) -> [f64; 4] {
        [
            self.left + f64::from(bounds.x) * self.scale_x,
            self.top + f64::from(bounds.y) * self.scale_y,
            f64::from(bounds.width) * self.scale_x,
            f64::from(bounds.height) * self.scale_y,
        ]
    }
}

/// Hands a Tab landing or a screen reader focus on the mirror back to the app.
fn attach_focus_listener(
    root: &HtmlElement,
    app: Rc<RefCell<AppShell<WgpuRenderer>>>,
    links: Rc<MirrorLinks>,
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
        let Some(node_id) = links.node_ids.borrow().get(&element_id).copied() else {
            return;
        };
        on_live_tree(&app, &links, |root| {
            accessibility::focus_node(root, node_id)
        });
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

fn on_mirror_click(
    root: &HtmlElement,
    mut action: impl FnMut(Element) + 'static,
) -> Result<(), JsValue> {
    let click = Closure::wrap(Box::new(move |event: MouseEvent| {
        if let Some(target) = event
            .target()
            .and_then(|target| target.dyn_into::<Element>().ok())
        {
            action(target);
        }
    }) as Box<dyn FnMut(_)>);
    root.add_event_listener_with_callback("click", click.as_ref().unchecked_ref())?;
    click.forget();
    Ok(())
}

fn attach_click_listener(
    root: &HtmlElement,
    app: Rc<RefCell<AppShell<WgpuRenderer>>>,
    links: Rc<MirrorLinks>,
) -> Result<(), JsValue> {
    on_mirror_click(root, move |target| {
        if !target.has_attribute("data-cranpose-clickable") {
            return;
        }
        let Some(node_id) = node_id_attribute(&target, "data-cranpose-node", &links) else {
            return;
        };
        let canvas_key = target
            .get_attribute("data-cranpose-canvas")
            .and_then(|value| value.parse::<u64>().ok());
        if let Ok(mut shell) = app.try_borrow_mut() {
            links.read.set(true);
            shell.accessibility_activate(node_id, canvas_key);
        }
    })
}

fn attach_action_listener(
    root: &HtmlElement,
    app: Rc<RefCell<AppShell<WgpuRenderer>>>,
    links: Rc<MirrorLinks>,
) -> Result<(), JsValue> {
    on_mirror_click(root, move |target| {
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
        let Some(node_id) = node_id_attribute(&target, "data-cranpose-action-node", &links) else {
            return;
        };
        let canvas_key = target
            .get_attribute("data-cranpose-canvas")
            .and_then(|value| value.parse::<u64>().ok());
        on_live_tree(&app, &links, |root| {
            accessibility::perform_listed_action(root, node_id, canvas_key, named, index)
        });
    })
}

/// Moves the value of an adjustable control with the arrow keys, the way a
/// screen reader and a keyboard user both expect of a slider.
fn attach_key_listener(
    root: &HtmlElement,
    app: Rc<RefCell<AppShell<WgpuRenderer>>>,
    links: Rc<MirrorLinks>,
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
        let Some(node_id) = node_id_attribute(&target, "data-cranpose-node", &links) else {
            return;
        };
        event.prevent_default();
        event.stop_propagation();
        on_live_tree(&app, &links, |root| {
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
    links: Rc<MirrorLinks>,
) -> Result<(), JsValue> {
    let input = Closure::wrap(Box::new(move |event: web_sys::Event| {
        let Some(target) = event
            .target()
            .and_then(|target| target.dyn_into::<Element>().ok())
        else {
            return;
        };
        sync_field_input(&target, &app, &links);
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
    links: &MirrorLinks,
) {
    let Some(node_id) = node_id_attribute(target, "data-cranpose-node", links) else {
        return;
    };
    let Some(value) = field_value(target) else {
        return;
    };
    let selection = field_selection(target);
    if let Some((anchor, focus)) = selection {
        let _ = target.set_attribute("data-cranpose-selection", &format!("{anchor}:{focus}"));
    }
    links.edited.set(true);
    on_live_tree(app, links, |root| {
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
    links: Rc<MirrorLinks>,
) -> Result<(), JsValue> {
    for name in ["compositionstart", "compositionupdate", "compositionend"] {
        let app = Rc::clone(&app);
        let links = Rc::clone(&links);
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
                sync_field_input(&target, &app, &links);
                if let Ok(mut shell) = app.try_borrow_mut() {
                    shell.on_ime_finish_composing();
                }
            } else {
                let start =
                    composition_range(&target).map_or_else(|| anchor.min(focus), |range| range.0);
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
        let Some(node_id) = cranpose_ui::text_field_focus::focused_field_target() else {
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

/// Room for the attributes of a mirror node as the mirror showed it and as
/// it shows it now.
#[derive(Default)]
struct AttributeRoom {
    shown: MirrorAttributes,
    now: MirrorAttributes,
}

struct MirrorEntry {
    node: HtmlElement,
    actions: Vec<HtmlElement>,
    /// The paging data the node carries.
    page: Option<PageTarget>,
    /// Where the node and its action buttons were put last, or nothing while
    /// one of them has no place yet.
    placed: Option<[f64; 4]>,
    /// The virtual id of the scroll container the node sits in, or nothing
    /// for the mirror root.
    parent: Option<i32>,
    /// Where the node sat among its parent's children when the mirror last
    /// wrote their order, its action buttons right after it, or nothing
    /// while none of them sits there.
    written_at: Option<usize>,
    /// Whether the node was built after that order, in place of the one that
    /// sat there.
    replaced: bool,
    /// How many of the action buttons were in that order.
    written_actions: usize,
}

impl MirrorEntry {
    fn new(
        document: &Document,
        id: i32,
        element: &AccessibilityElement,
        page: Option<PageTarget>,
        attributes: &mut MirrorAttributes,
    ) -> Result<Self, JsValue> {
        let mut entry = Self {
            node: mirror_node(document, id, element, page, attributes)?,
            actions: Vec::new(),
            page,
            placed: None,
            parent: None,
            written_at: None,
            replaced: false,
            written_actions: 0,
        };
        entry.update_actions(document, id, element)?;
        Ok(entry)
    }

    /// Takes the node and its action buttons out of the page.
    fn detach(&mut self) {
        self.node.remove();
        for button in &self.actions {
            button.remove();
        }
        self.written_at = None;
    }

    /// Adds the node and its action buttons to the children of their parent
    /// in the order the sync writes, each with where it sat in the order
    /// written last unless the page may hold anything after a failed sync,
    /// and notes where they sit now.
    fn push_children(&mut self, id: i32, children: &mut Vec<MirrorChild>, written: bool) {
        let was = self.written_at.filter(|_| written);
        self.written_at = Some(children.len());
        children.push(MirrorChild {
            id,
            slot: 0,
            was: was.filter(|_| !self.replaced),
        });
        children.extend((0..self.actions.len()).map(|button| {
            MirrorChild {
                id,
                slot: button + 1,
                was: was
                    .filter(|_| button < self.written_actions)
                    .map(|at| at + 1 + button),
            }
        }));
        self.replaced = false;
        self.written_actions = self.actions.len();
    }

    /// The node at slot 0, or an action button at a later slot.
    fn child(&self, slot: usize) -> Option<&HtmlElement> {
        match slot.checked_sub(1) {
            None => Some(&self.node),
            Some(button) => self.actions.get(button),
        }
    }

    /// Brings the node to `element`. A node that showed `shown` as the
    /// same kind of element gets only the attributes and the text that
    /// changed, worked out from the two elements without a read of the page.
    /// Any other node is replaced by a new one, so nothing written before
    /// stays on it.
    fn update(
        &mut self,
        document: &Document,
        id: i32,
        shown: Option<&AccessibilityElement>,
        element: &AccessibilityElement,
        page: Option<PageTarget>,
        attributes: &mut AttributeRoom,
    ) -> Result<(), JsValue> {
        match shown.filter(|shown| mirror_tag(shown) == mirror_tag(element)) {
            Some(shown) => {
                attributes.shown.describe(id, shown, self.page);
                attributes.now.describe(id, element, page);
                for name in attributes.now.dropped_from(&attributes.shown) {
                    self.node.remove_attribute(name)?;
                }
                for (name, value) in attributes.now.written_over(&attributes.shown) {
                    self.node.set_attribute(name, value)?;
                }
                let text = mirror_text(element);
                if mirror_text(shown) != text {
                    write_text(&self.node, text);
                }
                apply_field_text(&self.node, element)?;
            }
            None => {
                let node = mirror_node(document, id, element, page, &mut attributes.now)?;
                std::mem::replace(&mut self.node, node).remove();
                self.placed = None;
                self.replaced = true;
            }
        }
        self.page = page;
        self.update_actions(document, id, element)
    }

    /// Puts the node and its action buttons over the control they stand for,
    /// so a reader's cursor and a touch exploration land in the same place.
    /// Nothing is written when they sit there already.
    fn place(&mut self, rect: [f64; 4], css: &mut String) -> Result<(), JsValue> {
        if self.placed == Some(rect) {
            return Ok(());
        }
        let [left, top, width, height] = rect;
        css.clear();
        write!(
            css,
            "left:{left}px;top:{top}px;width:{width}px;height:{height}px"
        )
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
        for node in std::iter::once(&self.node).chain(&self.actions) {
            node.style().set_css_text(css);
        }
        self.placed = Some(rect);
        Ok(())
    }

    fn update_actions(
        &mut self,
        document: &Document,
        id: i32,
        element: &AccessibilityElement,
    ) -> Result<(), JsValue> {
        let named = accessibility::reader_actions(element).len();
        let actions = accessibility::listed_actions(element);
        for (index, action) in actions.iter().enumerate() {
            if index == self.actions.len() {
                self.actions.push(mirror_element(document, "button")?);
                self.placed = None;
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
        }
        for button in self.actions.drain(actions.len()..) {
            button.remove();
        }
        Ok(())
    }
}

/// The element the mirror showed for the control at `index` of the new
/// snapshot, when it showed one.
fn shown_element(shown: Option<&Replaced>, index: usize) -> Option<&AccessibilityElement> {
    let shown = shown?;
    shown.elements.get((*shown.was.get(index)?)?)
}

/// A node or an action button of a control in the order a sync writes: the
/// control's virtual id, the slot (0 for the node, the buttons after), and
/// where the child sat in the order written last.
#[derive(Clone, Copy)]
struct MirrorChild {
    id: i32,
    slot: usize,
    was: Option<usize>,
}

/// The children of one mirror parent in the order a sync writes.
#[derive(Default)]
struct ChildList {
    children: Vec<MirrorChild>,
    /// Whether the parent's node was built in this sync, so none of the
    /// children sit in it yet.
    replaced: bool,
}

/// Puts the children of `parent` in the order of `list`. A child moves only
/// when it is out of its old order, and then once.
fn reconcile_children(
    parent: &HtmlElement,
    list: &ChildList,
    entries: &HashMap<i32, MirrorEntry>,
    moves: &mut ChildMoves,
) -> Result<(), JsValue> {
    moves.plan(
        list.children
            .iter()
            .map(|child| child.was.filter(|_| !list.replaced)),
    );
    let node = |index: usize| {
        list.children
            .get(index)
            .and_then(|child| entries.get(&child.id)?.child(child.slot))
            .map(AsRef::<web_sys::Node>::as_ref)
            .ok_or_else(|| JsValue::from_str("a mirror child has no node"))
    };
    for (child, before) in moves.moves() {
        let before = before.map(node).transpose()?;
        parent.insert_before(node(child)?, before)?;
    }
    Ok(())
}

/// The children of each mirror parent, the root or a scroll container, in
/// the order a sync writes, and the plan that moves them there.
#[derive(Default)]
struct MirrorOrders {
    lists: HashMap<Option<i32>, ChildList>,
    moves: ChildMoves,
}

impl MirrorOrders {
    /// Empties the lists for a new sync.
    fn start(&mut self) {
        for list in self.lists.values_mut() {
            list.children.clear();
            list.replaced = false;
        }
    }

    /// Adds the node and the action buttons of the entry with virtual id
    /// `id` to the children of `parent`, the scroll container it sits in.
    /// After a failed sync `written` is false: the page may hold anything,
    /// so no child keeps an old position and the whole order is written.
    fn push(&mut self, id: i32, entry: &mut MirrorEntry, parent: Option<i32>, written: bool) {
        if entry.replaced {
            self.lists.entry(Some(id)).or_default().replaced = true;
        }
        if entry.parent != parent {
            // A node that changed containers leaves the old one and has no
            // place in the new one yet.
            entry.detach();
            entry.parent = parent;
        }
        entry.push_children(
            id,
            &mut self.lists.entry(parent).or_default().children,
            written,
        );
    }

    /// Puts the children of every parent in the order of its list.
    fn write(
        &mut self,
        root: &HtmlElement,
        entries: &HashMap<i32, MirrorEntry>,
    ) -> Result<(), JsValue> {
        self.lists.retain(|_, list| !list.children.is_empty());
        for (parent, list) in &self.lists {
            let parent = match parent {
                None => root,
                Some(id) => entries
                    .get(id)
                    .map(|entry| &entry.node)
                    .ok_or_else(|| JsValue::from_str("a mirror parent has no node"))?,
            };
            reconcile_children(parent, list, entries, &mut self.moves)?;
        }
        Ok(())
    }
}

pub(crate) struct WebAccessibilityBridge {
    root: HtmlElement,
    canvas: HtmlCanvasElement,
    /// The elements the mirror shows.
    previous: accessibility::AccessibilitySnapshot,
    /// The semantics revision the mirror last looked at, or nothing when it
    /// must look again whatever the revision: before its first look and after
    /// a failed sync.
    seen_revision: Option<u64>,
    /// The node that held the app's focus when the mirror last looked.
    seen_focus: Option<cranpose_core::NodeId>,
    /// When the mirror looks at a change no reader needs at once.
    policy: AccessibilityPublishPolicy,
    /// Room for the declarations that place one mirror node.
    css: String,
    /// Room for the attributes of one mirror node the sync brings up to date.
    attributes: AttributeRoom,
    orders: MirrorOrders,
    dirty: bool,
    entries: HashMap<i32, MirrorEntry>,
    links: Rc<MirrorLinks>,
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

        let body = document.body().ok_or("document has no body")?;
        let style = mirror_style(document)?;
        body.append_child(&style)?;
        body.append_child(&root)?;
        let polite = live_region(document, "polite")?;
        let assertive = live_region(document, "assertive")?;
        body.append_child(&polite)?;
        body.append_child(&assertive)?;
        let links = Rc::new(MirrorLinks::default());
        attach_click_listener(&root, Rc::clone(&app), Rc::clone(&links))?;
        attach_focus_listener(&root, Rc::clone(&app), Rc::clone(&links))?;
        attach_key_listener(&root, Rc::clone(&app), Rc::clone(&links))?;
        attach_action_listener(&root, Rc::clone(&app), Rc::clone(&links))?;
        attach_selection_listener(document, Rc::clone(&app), Rc::clone(&links))?;
        attach_input_listener(&root, Rc::clone(&app), Rc::clone(&links))?;
        attach_composition_listener(&root, Rc::clone(&app), Rc::clone(&links))?;
        attach_page_listener(&root, app, Rc::clone(&links))?;

        Ok(Self {
            root,
            canvas,
            previous: accessibility::AccessibilitySnapshot::default(),
            seen_revision: None,
            seen_focus: None,
            policy: AccessibilityPublishPolicy::new(),
            css: String::new(),
            attributes: AttributeRoom::default(),
            orders: MirrorOrders::default(),
            dirty: false,
            entries: HashMap::new(),
            links,
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

    /// When a frame must come for the mirror to look at a change the publish
    /// interval holds back, if one waits.
    pub(crate) fn sync_wake(&self) -> Option<Instant> {
        self.policy.wake_deadline()
    }

    /// Puts text a screen reader reads out into the live region that matches
    /// how urgent it is. The same text twice in a row carries a trailing space
    /// one time out of two, because a reader reads a live region only when its
    /// text changes.
    fn speak(&mut self, announcements: Vec<cranpose_ui::Announcement>) {
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

    /// Looks at the app's controls and brings the mirror to them. The mirror
    /// looks at once for its first controls, after a failed sync, when the
    /// app's focus moved and when a person edited a field through it. Any
    /// other change waits for the shared publish interval: a second, or the
    /// interactive interval while a reader acts. Nothing is built while the
    /// controls are as the mirror last saw them. Text the app asks to read
    /// out is spoken every frame, and a live region or a pane title that
    /// changed is spoken when the mirror looks.
    pub(crate) fn sync(
        &mut self,
        document: &Document,
        shell: &mut AppShell<WgpuRenderer>,
    ) -> Result<(), JsValue> {
        let now = Instant::now();
        if self.policy.update_enabled(shell.semantics_active()) {
            self.seen_revision = None;
        }
        // The web cannot tell whether a screen reader runs, so what a reader
        // does through the mirror is the only sign of one.
        if self.links.read.take() {
            self.policy.note_read(now);
        }
        let focus = shell.app_context().enter(cranpose_ui::active_focus_target);
        let at_once =
            self.seen_revision.is_none() || self.links.edited.get() || focus != self.seen_focus;
        let changed = self.seen_revision != Some(shell.semantics_snapshot_revision());
        let looks = self.policy.try_publish_change(now, changed, at_once);
        let elements = if looks {
            self.seen_focus = focus;
            self.links.edited.set(false);
            accessibility::snapshot_if_changed(shell, &mut self.seen_revision, &mut self.previous)
                .and_then(|elements| self.previous.changed(elements, self.dirty))
        } else {
            None
        };
        let mut announcements = accessibility::drain_app_announcements();
        if let Some(elements) = &elements {
            announcements.extend(accessibility::live_region_announcements(
                &self.previous.elements,
                elements,
            ));
            announcements.extend(accessibility::pane_title_announcements(
                &self.previous.elements,
                elements,
            ));
        }
        self.speak(announcements);
        let Some(elements) = elements else {
            // The elements leave out the text of a secret, so a look writes
            // it even when they stay the same.
            return if looks {
                self.sync_password(shell, &self.previous.elements)
            } else {
                Ok(())
            };
        };
        let result = self.show(document, shell, elements);
        // The web never learns whether a tree was read, so the wait never
        // grows.
        self.policy.published(false);
        if self.dirty {
            self.seen_revision = None;
        }
        result
    }

    /// Brings the mirror to `elements`, which differ from the ones it shows.
    fn show(
        &mut self,
        document: &Document,
        shell: &mut AppShell<WgpuRenderer>,
        elements: Vec<AccessibilityElement>,
    ) -> Result<(), JsValue> {
        let opened_dialog = accessibility::opened_dialog(&self.previous.elements, &elements);
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
        let mut next_snapshot = std::mem::take(&mut self.previous);
        let replaced = match next_snapshot.update(elements) {
            Ok(replaced) => replaced,
            Err(error) => {
                self.previous = next_snapshot;
                return Err(JsValue::from_str(&error.to_string()));
            }
        };
        // After a failed sync the mirror may hold anything, so every node is
        // written again.
        let shown = (!self.dirty).then_some(&replaced);
        let mut result = (|| {
            self.reconcile(document, &next_snapshot, shown, &placement)?;
            self.sync_password(shell, &next_snapshot.elements)?;
            for (id, element) in next_snapshot.ids.iter().zip(&next_snapshot.elements) {
                let opened = opened_dialog == Some(element.node_id);
                if !element.focused && !opened {
                    continue;
                }
                let Some(node) = self.entries.get(id).map(|entry| entry.node.clone()) else {
                    continue;
                };
                self.follow_app_focus(&node, element, *id)?;
                if opened {
                    node.focus()?;
                }
            }
            Ok(())
        })();
        next_snapshot.recycle(replaced.elements);
        self.previous = next_snapshot;
        if result.is_ok() {
            result = self.settle_focus(held, app_focus_before);
        }
        self.dirty = result.is_err();
        result
    }

    /// Writes the text and the caret of each secret field among `elements`,
    /// which the elements leave out, from the semantics tree a look built.
    fn sync_password(
        &self,
        shell: &mut AppShell<WgpuRenderer>,
        elements: &[AccessibilityElement],
    ) -> Result<(), JsValue> {
        let mut passwords = elements
            .iter()
            .filter(|element| element.details().password)
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
                && let (Some(text), Some(selection)) = (&field.text, field.details().text_selection)
            {
                apply_editor_text(node, text, selection.start, selection.end)?;
            }
        }
        Ok(())
    }

    /// Brings the mirror to `snapshot`. A control the mirror shows as it was
    /// in `shown`, the update it replaced, keeps its node as written and at
    /// most moves.
    fn reconcile(
        &mut self,
        document: &Document,
        snapshot: &accessibility::AccessibilitySnapshot,
        shown: Option<&Replaced>,
        placement: &Placement,
    ) -> Result<(), JsValue> {
        let elements = &snapshot.elements;
        let ids = &snapshot.ids;
        let pages = page_targets(ids, elements);
        let parents: HashMap<_, _> = ids
            .iter()
            .zip(elements)
            .filter(|(_, element)| element.canvas_key.is_none())
            .map(|(id, element)| (element.node_id, *id))
            .collect();
        // The nodes of controls that left go first.
        self.entries.retain(|id, entry| {
            let stays = snapshot.element(*id).is_some();
            if !stays {
                entry.detach();
            }
            stays
        });
        self.orders.start();
        self.links.node_ids.borrow_mut().clear();
        self.text_input.fields.borrow_mut().clear();
        for (index, ((id, element), page)) in
            ids.iter().copied().zip(elements).zip(pages).enumerate()
        {
            let entry = match self.entries.entry(id) {
                std::collections::hash_map::Entry::Occupied(entry) => {
                    let entry = entry.into_mut();
                    let shown = shown_element(shown, index);
                    let unchanged =
                        entry.page == page && shown.is_some_and(|old| old.same_but_bounds(element));
                    if !unchanged {
                        entry.update(document, id, shown, element, page, &mut self.attributes)?;
                    }
                    entry
                }
                std::collections::hash_map::Entry::Vacant(entry) => entry.insert(MirrorEntry::new(
                    document,
                    id,
                    element,
                    page,
                    &mut self.attributes.now,
                )?),
            };
            entry.place(placement.rect(element.bounds), &mut self.css)?;
            self.links.node_ids.borrow_mut().insert(id, element.node_id);
            if edits_text(element) {
                self.text_input
                    .fields
                    .borrow_mut()
                    .insert(element.node_id, entry.node.clone());
            }
            let parent = element
                .scroll_parent
                .and_then(|parent| parents.get(&parent).copied());
            self.orders.push(id, entry, parent, shown.is_some());
        }
        self.orders.write(&self.root, &self.entries)
    }

    fn settle_focus(
        &mut self,
        held: Option<String>,
        app_focus_before: Option<i32>,
    ) -> Result<(), JsValue> {
        if !self.previous.elements.iter().any(|element| element.focused) {
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
