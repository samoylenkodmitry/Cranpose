//! Text entry on the web. A browser composes text and opens its on-screen
//! keyboard only for an editable element that holds its focus, so the focused
//! text field always has one. While the accessibility mirror is on, that
//! element is the field's node in the mirror, so a screen reader edits the
//! node it reads. Otherwise it is a hidden editor kept for this: one `<input>`
//! for fields of one line, one `<textarea>` for fields of many lines and one
//! password `<input>` for fields that hold a secret, each made the first time
//! a field needs it. All of them take typing, composition and caret moves
//! through the same listeners.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::{Rc, Weak},
};

use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;
use cranpose_ui::text_field_focus::{self, ImeCaretGeometry, ImeEditorState};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{
    Document, Element, HtmlCanvasElement, HtmlElement, HtmlInputElement, HtmlTextAreaElement,
};

use crate::{
    accessibility::{self, AccessibilityRect},
    web_accessibility::{MirrorLinks, Placement, node_id_attribute, on_live_tree},
    web_text_span::changed_span,
};

/// The attribute that marks a hidden editor.
const EDITOR_ATTRIBUTE: &str = "data-cranpose-editor";

/// The field an editable element hands its edits to.
#[derive(Clone, Copy)]
enum FieldOwner {
    /// A mirror node edits the field it stands for, through the semantics
    /// tree a reader reads.
    Node(cranpose_core::NodeId),
    /// A hidden editor edits the focused field.
    Focused,
}

fn field_owner(target: &Element, links: &MirrorLinks) -> Option<FieldOwner> {
    if target.has_attribute(EDITOR_ATTRIBUTE) {
        Some(FieldOwner::Focused)
    } else {
        node_id_attribute(target, "data-cranpose-node", links).map(FieldOwner::Node)
    }
}

/// Hands the app what a person left in an editable element: its text
/// `value`, when `edited_text` says they changed it, and the ends of its
/// selection in the UTF-16 units a browser counts, the anchor first.
fn edit_field(
    app: &Rc<RefCell<AppShell<WgpuRenderer>>>,
    links: &MirrorLinks,
    owner: FieldOwner,
    value: &str,
    edited_text: bool,
    selection: Option<(usize, usize)>,
) {
    let selection = selection.map(|(anchor, focus)| {
        (
            accessibility::byte_offset_for_utf16(value, anchor),
            accessibility::byte_offset_for_utf16(value, focus),
        )
    });
    match owner {
        FieldOwner::Node(node_id) => {
            links.edited.set(true);
            on_live_tree(app, links, |root| {
                let changed = edited_text && accessibility::set_text(root, node_id, value);
                selection.is_some_and(|(anchor, focus)| {
                    accessibility::set_text_selection(root, node_id, anchor, focus)
                }) || changed
            });
        }
        FieldOwner::Focused => {
            let Ok(mut shell) = app.try_borrow_mut() else {
                return;
            };
            if edited_text {
                replace_focused_text(&mut shell, value);
            }
            if let Some((anchor, focus)) = selection {
                shell.on_ime_set_selection(anchor, focus);
            }
        }
    }
}

/// Makes the focused field hold `value`. The stretch of its text that
/// differs is selected and replaced, the way a keyboard replaces it.
fn replace_focused_text(shell: &mut AppShell<WgpuRenderer>, value: &str) {
    let Some(state) = shell.ime_editor_state() else {
        return;
    };
    if let Some(span) = changed_span(&state.text, value)
        && let Some(inserted) = value.get(span.start..span.inserted_end)
    {
        shell.on_ime_set_selection(span.start, span.removed_end);
        shell.on_paste(inserted);
    }
}

/// Writes the text a field holds as the value of its editable element, and
/// where its caret or its picked stretch of text sits, in the UTF-16 units a
/// browser counts. The same ends go on the element, so the selection
/// listener can tell a move the app made from one a person made.
pub(crate) fn apply_editor_text(
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

/// Puts the caret of an editable element back where the app last published
/// it, after the browser's focus moved onto it.
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

/// Moves the browser's focus onto a mirror node or an editor, and for a
/// field puts the caret back where it was, because a fresh input starts with
/// its caret at the start.
pub(crate) fn focus_field(node: &HtmlElement) -> Result<(), JsValue> {
    if node.matches(":focus")? {
        return Ok(());
    }
    node.focus()?;
    restore_field_caret(node)
}

/// The two ends of the selection in an editable element, the anchor first,
/// in UTF-16 units, or nothing for an element that is not one.
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

pub(crate) fn field_value(target: &Element) -> Option<String> {
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

/// Hands the app the caret a reader or a keyboard moved inside an editable
/// element, through the browser's own selection change. A change that only
/// echoes the ends the app published is not sent back.
pub(crate) fn attach_selection_listener(
    document: &Document,
    app: Rc<RefCell<AppShell<WgpuRenderer>>>,
    links: Rc<MirrorLinks>,
) -> Result<(), JsValue> {
    let page = document.clone();
    let on_change = Closure::wrap(Box::new(move |_event: web_sys::Event| {
        let Some(active) = page.active_element() else {
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
        let Some(owner) = field_owner(&active, &links) else {
            return;
        };
        let Some(value) = field_value(&active) else {
            return;
        };
        let _ = active.set_attribute("data-cranpose-selection", &ends);
        edit_field(&app, &links, owner, &value, false, Some((anchor, focus)));
    }) as Box<dyn FnMut(_)>);
    document
        .add_event_listener_with_callback("selectionchange", on_change.as_ref().unchecked_ref())?;
    on_change.forget();
    Ok(())
}

/// The listeners every editable element needs, on the element itself or on
/// an element that holds them: a blur that ends the field's focus, the keys
/// the browser handles kept from the app, and typing, composition and the
/// clipboard handed to the app.
pub(crate) fn attach_field_listeners(
    target: &HtmlElement,
    app: Rc<RefCell<AppShell<WgpuRenderer>>>,
    links: Rc<MirrorLinks>,
) -> Result<(), JsValue> {
    attach_blur_listener(target, Rc::clone(&app))?;
    attach_field_key_listener(target)?;
    attach_input_listener(target, Rc::clone(&app), Rc::clone(&links))?;
    attach_composition_listener(target, app, links)
}

/// Ends the app's field focus when its editable element loses the browser's
/// focus, finishing a composition left open.
fn attach_blur_listener(
    target: &HtmlElement,
    app: Rc<RefCell<AppShell<WgpuRenderer>>>,
) -> Result<(), JsValue> {
    let focus_out = Closure::wrap(Box::new(move |event: web_sys::Event| {
        let Some(target) = event
            .target()
            .and_then(|target| target.dyn_into::<Element>().ok())
            .filter(|target| field_value(target).is_some())
        else {
            return;
        };
        let _ = target.remove_attribute("data-cranpose-composition");
        if let Ok(mut shell) = app.try_borrow_mut() {
            shell.on_ime_finish_composing();
            shell.clear_text_field_focus();
        }
    }) as Box<dyn FnMut(_)>);
    target.add_event_listener_with_callback("focusout", focus_out.as_ref().unchecked_ref())?;
    focus_out.forget();
    Ok(())
}

/// Keeps each event named in `names` that reaches an element `keep` picks
/// from the listeners above `target`, the app's own on the document among
/// them.
fn stop_events(
    target: &HtmlElement,
    names: &[&str],
    keep: fn(&web_sys::Event, &Element) -> bool,
) -> Result<(), JsValue> {
    for name in names {
        let listener = Closure::wrap(Box::new(move |event: web_sys::Event| {
            if event
                .target()
                .and_then(|target| target.dyn_into::<Element>().ok())
                .is_some_and(|element| keep(&event, &element))
            {
                event.stop_propagation();
            }
        }) as Box<dyn FnMut(_)>);
        target.add_event_listener_with_callback(name, listener.as_ref().unchecked_ref())?;
        listener.forget();
    }
    Ok(())
}

/// Keeps a key the browser handles on its own element from the app's key
/// listener on the document, so a keystroke is never handled twice.
pub(crate) fn attach_field_key_listener(target: &HtmlElement) -> Result<(), JsValue> {
    stop_events(target, &["keydown", "keyup"], |event, element| {
        event
            .dyn_ref::<web_sys::KeyboardEvent>()
            .is_some_and(|key| browser_handles_key(key, element))
    })
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
        // The page holds a hidden editor in no tab order, so the app moves
        // its own focus on from the field the editor stands in for.
        return !target.has_attribute(EDITOR_ATTRIBUTE)
            && target
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

fn attach_input_listener(
    target: &HtmlElement,
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
    target.add_event_listener_with_callback("input", input.as_ref().unchecked_ref())?;
    input.forget();
    Ok(())
}

fn sync_field_input(
    target: &Element,
    app: &Rc<RefCell<AppShell<WgpuRenderer>>>,
    links: &MirrorLinks,
) {
    let Some(owner) = field_owner(target, links) else {
        return;
    };
    let Some(value) = field_value(target) else {
        return;
    };
    let selection = field_selection(target);
    if let Some((anchor, focus)) = selection {
        let _ = target.set_attribute("data-cranpose-selection", &format!("{anchor}:{focus}"));
    }
    edit_field(app, links, owner, &value, true, selection);
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
    target: &HtmlElement,
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
        target.add_event_listener_with_callback(name, listener.as_ref().unchecked_ref())?;
        listener.forget();
    }
    stop_events(target, &["copy", "cut", "paste"], |_, element| {
        field_value(element).is_some()
    })
}

/// The three hidden editors, by the fields they take.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EditorKind {
    /// A field of one line.
    Line,
    /// A field of many lines.
    Area,
    /// A field that holds a secret: a password input, which a software
    /// keyboard neither learns from nor suggests for, whatever its lines.
    Secret,
}

impl EditorKind {
    fn of(state: &ImeEditorState, secret: bool) -> Self {
        if secret {
            Self::Secret
        } else if state.single_line {
            Self::Line
        } else {
            Self::Area
        }
    }
}

/// An editable element no one sees and no pointer reaches. It stays out of
/// the page's tab order, since the app moves its focus between fields
/// itself, and its text has the size at which a phone does not zoom in on
/// a focused field. A secret field's editor is a password input that asks
/// for the browser's saved password, so a password manager can fill a login
/// field through the same input events.
fn hidden_editor(document: &Document, kind: EditorKind) -> Result<HtmlElement, JsValue> {
    let tag = match kind {
        EditorKind::Area => "textarea",
        EditorKind::Line | EditorKind::Secret => "input",
    };
    let editor = document.create_element(tag)?.dyn_into::<HtmlElement>()?;
    editor.set_attribute(EDITOR_ATTRIBUTE, "")?;
    editor.set_attribute("tabindex", "-1")?;
    if kind == EditorKind::Secret {
        editor.set_attribute("type", "password")?;
        editor.set_attribute("autocomplete", "current-password")?;
    }
    editor.style().set_css_text(
        "position:fixed;left:0;top:0;width:1px;height:1px;opacity:0;\
         pointer-events:none;border:0;padding:0;margin:0;font-size:16px;\
         resize:none;overflow:hidden",
    );
    Ok(editor)
}

/// Puts an editor over the text of the focused field, so a browser opens a
/// composition window next to it and a phone scrolls the field into view,
/// not the corner of the page.
fn place(
    editor: &HtmlElement,
    geometry: &ImeCaretGeometry,
    placement: &Placement,
) -> Result<(), JsValue> {
    let text = geometry.range_rect(0, geometry.caret_xs.len().saturating_sub(1));
    let rect = placement.rect(AccessibilityRect::new(
        text.x,
        text.y,
        text.width,
        text.height,
    ));
    let style = editor.style();
    for (name, value) in ["left", "top", "width", "height"].into_iter().zip(rect) {
        style.set_property(name, &format!("{value}px"))?;
    }
    Ok(())
}

/// The ends of the focused field's selection, the anchor first. The app
/// keeps no direction for them, so a selection the browser holds backward
/// over the same text stays backward.
fn selection_ends(node: &Element, state: &ImeEditorState) -> (usize, usize) {
    let (start, end) = (state.selection_start, state.selection_end);
    let backward = (
        accessibility::utf16_offset(&state.text, end),
        accessibility::utf16_offset(&state.text, start),
    );
    if start != end && field_selection(node) == Some(backward) {
        (end, start)
    } else {
        (start, end)
    }
}

pub(crate) struct WebTextInput {
    me: Weak<WebTextInput>,
    document: Document,
    canvas: HtmlCanvasElement,
    app: Weak<RefCell<AppShell<WgpuRenderer>>>,
    links: Rc<MirrorLinks>,
    /// The mirror's field nodes by the field each stands for, while the
    /// mirror is on.
    pub(crate) fields: RefCell<HashMap<cranpose_core::NodeId, HtmlElement>>,
    /// The element that holds the browser's focus for the focused field.
    active: RefCell<Option<HtmlElement>>,
    /// Whether the mirror is on, so its field nodes are the editors.
    mirrored: Cell<bool>,
    /// The hidden editors by [`EditorKind`], each made the first time a
    /// field needs it.
    editors: [RefCell<Option<HtmlElement>>; 3],
    /// Whether the app asked for a hidden editor and has not let it go.
    wanted: Cell<bool>,
    /// Whether an editor waits to be opened after the event that asked.
    opening: Cell<bool>,
    /// The field the hidden editor holds.
    field: Cell<Option<cranpose_core::NodeId>>,
    /// The field as the hidden editor last showed it.
    shown: RefCell<Option<ImeEditorState>>,
}

impl WebTextInput {
    pub(crate) fn new(
        document: Document,
        canvas: HtmlCanvasElement,
        app: Weak<RefCell<AppShell<WgpuRenderer>>>,
        links: Rc<MirrorLinks>,
    ) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            me: Weak::clone(me),
            document,
            canvas,
            app,
            links,
            fields: RefCell::default(),
            active: RefCell::default(),
            mirrored: Cell::new(false),
            editors: Default::default(),
            wanted: Cell::new(false),
            opening: Cell::new(false),
            field: Cell::new(None),
            shown: RefCell::default(),
        })
    }

    /// Makes `node`, the mirror node of a field the app focused, the element
    /// that takes the field's typing.
    pub(crate) fn set_active(&self, node: &HtmlElement) {
        *self.active.borrow_mut() = Some(node.clone());
    }

    /// Lets the hidden editor go: nothing follows the field any more.
    fn release(&self) {
        self.wanted.set(false);
        self.field.set(None);
        *self.shown.borrow_mut() = None;
    }

    /// Hands text entry to the mirror's field nodes when the mirror turns
    /// on. The hidden editors leave the page, and the mirror focuses the
    /// focused field's node as it shows it.
    pub(crate) fn hand_to_mirror(&self) {
        self.mirrored.set(true);
        self.release();
        for slot in &self.editors {
            if let Some(editor) = slot.borrow_mut().take() {
                editor.remove();
            }
        }
        let mut active = self.active.borrow_mut();
        if active
            .as_ref()
            .is_some_and(|node| node.has_attribute(EDITOR_ATTRIBUTE))
        {
            *active = None;
        }
    }

    /// Brings the hidden editor to the focused field once a frame while the
    /// mirror is off: the text and the caret the app holds after a change
    /// the app made itself, such as a click that moved the caret. A field
    /// the app focused without asking for the keyboard opens in its own
    /// editor. Nothing is written while the field is as the editor last
    /// showed it.
    pub(crate) fn follow(&self, shell: &mut AppShell<WgpuRenderer>) -> Result<(), JsValue> {
        if !self.wanted.get() {
            return Ok(());
        }
        if shell
            .app_context()
            .enter(text_field_focus::focused_field_target)
            != self.field.get()
        {
            return self.open(shell);
        }
        let Some(state) = shell.ime_editor_state() else {
            return Ok(());
        };
        if self.shown.borrow().as_ref() == Some(&state) {
            return Ok(());
        }
        if let Some(editor) = self.active.borrow().as_ref() {
            let (anchor, focus) = selection_ends(editor, &state);
            apply_editor_text(editor, &state.text, anchor, focus)?;
        }
        *self.shown.borrow_mut() = Some(state);
        Ok(())
    }

    /// Opens the focused field in the hidden editor of its kind: placed
    /// over the field's text, with its text and caret, and with the
    /// browser's focus. Whether the field holds a secret is read once here,
    /// from the field's own semantics. The shell stays borrowed while the
    /// focus moves, so the editor the focus leaves does not end the field's
    /// focus in the app.
    fn open(&self, shell: &mut AppShell<WgpuRenderer>) -> Result<(), JsValue> {
        self.field.set(
            shell
                .app_context()
                .enter(text_field_focus::focused_field_target),
        );
        let Some(state) = shell.ime_editor_state() else {
            return Ok(());
        };
        let editor = self.editor(EditorKind::of(&state, shell.ime_field_is_password()))?;
        if let Some(geometry) = shell.ime_caret_geometry() {
            place(
                &editor,
                &geometry,
                &Placement::of(&self.canvas, shell.viewport_size()),
            )?;
        }
        let (anchor, focus) = selection_ends(&editor, &state);
        apply_editor_text(&editor, &state.text, anchor, focus)?;
        *self.shown.borrow_mut() = Some(state);
        self.set_active(&editor);
        focus_field(&editor)
    }

    /// Opens the editor the app asked for, after the event that asked: the
    /// shell, which alone can read the field's semantics, is busy with that
    /// event while it asks. A microtask still runs inside the person's
    /// gesture, so a phone opens its keyboard for the editor.
    fn open_after_event(&self) {
        self.opening.set(false);
        if !self.wanted.get() || self.mirrored.get() {
            return;
        }
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let Ok(mut shell) = app.try_borrow_mut() else {
            return;
        };
        if let Err(error) = self.open(&mut shell) {
            log::error!("web text editor unavailable: {error:?}");
        }
    }

    /// The hidden editor of `kind`, made the first time a field needs it.
    fn editor(&self, kind: EditorKind) -> Result<HtmlElement, JsValue> {
        let slot = &self.editors[kind as usize];
        if let Some(editor) = slot.borrow().as_ref() {
            return Ok(editor.clone());
        }
        let app = self.app.upgrade().ok_or("the app is gone")?;
        let editor = hidden_editor(&self.document, kind)?;
        attach_field_listeners(&editor, app, Rc::clone(&self.links))?;
        self.document
            .body()
            .ok_or("document has no body")?
            .append_child(&editor)?;
        *slot.borrow_mut() = Some(editor.clone());
        Ok(editor)
    }
}

impl cranpose_app_shell::PlatformTextInputHandler for WebTextInput {
    fn show_keyboard(&self) {
        if !self.mirrored.get() {
            self.wanted.set(true);
            self.field.set(None);
            if !self.opening.replace(true)
                && let Some(input) = self.me.upgrade()
            {
                wasm_bindgen_futures::spawn_local(async move { input.open_after_event() });
            }
            return;
        }
        let state = text_field_focus::focused_editor_state();
        let Some(node) = text_field_focus::focused_field_target()
            .and_then(|node_id| self.fields.borrow().get(&node_id).cloned())
        else {
            return;
        };
        if let Some(state) = &state {
            let (anchor, focus) = selection_ends(&node, state);
            let _ = apply_editor_text(&node, &state.text, anchor, focus);
        }
        self.set_active(&node);
        let _ = focus_field(&node);
    }

    fn hide_keyboard(&self) {
        self.release();
        let active = self.active.borrow_mut().take();
        if let Some(node) = active {
            let _ = node.blur();
        }
    }
}
