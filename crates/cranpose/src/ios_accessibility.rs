#![allow(unsafe_code)]

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    fmt::Debug,
    rc::Rc,
};

use cranpose_app_shell::{AppShell, PointerSource};
use cranpose_render_common::Renderer;
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, Bool},
    sel,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSString};
use objc2_ui_kit::{
    NSObjectUIAccessibility, NSObjectUIAccessibilityAction, NSObjectUIAccessibilityContainer,
    UIAccessibilityAnnouncementNotification, UIAccessibilityCustomAction, UIAccessibilityElement,
    UIAccessibilityIdentification, UIAccessibilityLayoutChangedNotification,
    UIAccessibilityPostNotification, UIAccessibilityScreenChangedNotification,
    UIAccessibilityTraitAdjustable, UIAccessibilityTraitButton, UIAccessibilityTraitHeader,
    UIAccessibilityTraitImage, UIAccessibilityTraitNone, UIAccessibilityTraitNotEnabled,
    UIAccessibilityTraitSelected, UIAccessibilityTraitStaticText, UIView,
};
use winit::event_loop::EventLoopProxy;

use crate::{
    accessibility::{self, AccessibilityElement, AccessibilityRole},
    ios_file_picker::root_view_controller,
};

const SCROLL_RIGHT: isize = 1;
const SCROLL_LEFT: isize = 2;
const SCROLL_UP: isize = 3;
const SCROLL_DOWN: isize = 4;
const SCROLL_NEXT: isize = 5;
const SCROLL_PREVIOUS: isize = 6;

#[derive(Clone, Default)]
struct ReaderRequests {
    activations: Rc<RefCell<Vec<i32>>>,
    focus: Rc<RefCell<Vec<i32>>>,
    steps: Rc<RefCell<Vec<(i32, bool)>>>,
    scrolls: Rc<RefCell<Vec<(i32, bool)>>>,
    escapes: Rc<Cell<usize>>,
    custom_actions: Rc<RefCell<Vec<(i32, usize)>>>,
}

struct AccessibilityElementIvars {
    element_id: i32,
    actionable: Cell<bool>,
    custom_action_labels: RefCell<Vec<String>>,
    requests: ReaderRequests,
    wake_proxy: EventLoopProxy,
}

define_class!(
    #[unsafe(super(UIAccessibilityElement))]
    #[thread_kind = MainThreadOnly]
    #[name = "CranposeAccessibilityElement"]
    #[ivars = AccessibilityElementIvars]
    struct NativeAccessibilityElement;

    // SAFETY: The class inherits NSObject protocol conformance from
    // UIAccessibilityElement and adds only Rust-owned ivars and one override.
    unsafe impl NSObjectProtocol for NativeAccessibilityElement {}

    impl NativeAccessibilityElement {
        #[unsafe(method(accessibilityActivate))]
        fn accessibility_activate(&self) -> Bool {
            if !self.ivars().actionable.get() {
                return Bool::NO;
            }
            self.ivars()
                .requests.activations
                .borrow_mut()
                .push(self.ivars().element_id);
            self.ivars().wake_proxy.wake_up();
            Bool::YES
        }

        #[unsafe(method(accessibilityIncrement))]
        fn accessibility_increment(&self) {
            self.ivars()
                .requests.steps
                .borrow_mut()
                .push((self.ivars().element_id, true));
            self.ivars().wake_proxy.wake_up();
        }

        #[unsafe(method(accessibilityDecrement))]
        fn accessibility_decrement(&self) {
            self.ivars()
                .requests.steps
                .borrow_mut()
                .push((self.ivars().element_id, false));
            self.ivars().wake_proxy.wake_up();
        }

        #[unsafe(method(accessibilityScroll:))]
        fn accessibility_scroll(&self, direction: isize) -> Bool {
            let forward = match direction {
                SCROLL_RIGHT | SCROLL_DOWN | SCROLL_NEXT => true,
                SCROLL_LEFT | SCROLL_UP | SCROLL_PREVIOUS => false,
                _ => return Bool::NO,
            };
            self.ivars()
                .requests.scrolls
                .borrow_mut()
                .push((self.ivars().element_id, forward));
            self.ivars().wake_proxy.wake_up();
            Bool::YES
        }

        #[unsafe(method(performAccessibilityCustomAction:))]
        fn perform_accessibility_custom_action(
            &self,
            action: &UIAccessibilityCustomAction,
        ) -> Bool {
            let name = action.name().to_string();
            let Some(index) = self
                .ivars()
                .custom_action_labels
                .borrow()
                .iter()
                .position(|label| *label == name)
            else {
                return Bool::NO;
            };
            self.ivars()
                .requests
                .custom_actions
                .borrow_mut()
                .push((self.ivars().element_id, index));
            self.ivars().wake_proxy.wake_up();
            Bool::YES
        }

        #[unsafe(method(accessibilityPerformEscape))]
        fn accessibility_perform_escape(&self) -> Bool {
            if !accessibility::escape_has_a_taker() {
                return Bool::NO;
            }
            let escapes = &self.ivars().requests.escapes;
            escapes.set(escapes.get() + 1);
            self.ivars().wake_proxy.wake_up();
            Bool::YES
        }

        #[unsafe(method(accessibilityElementDidBecomeFocused))]
        fn accessibility_element_did_become_focused(&self) {
            self.ivars()
                .requests.focus
                .borrow_mut()
                .push(self.ivars().element_id);
            self.ivars().wake_proxy.wake_up();
        }
    }
);

impl NativeAccessibilityElement {
    fn new(
        container: &AnyObject,
        element_id: i32,
        requests: ReaderRequests,
        wake_proxy: EventLoopProxy,
        mtm: MainThreadMarker,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(AccessibilityElementIvars {
            element_id,
            actionable: Cell::new(false),
            custom_action_labels: RefCell::new(Vec::new()),
            requests,
            wake_proxy,
        });
        // SAFETY: `container` is the retained winit root UIView and implements
        // the UIAccessibilityContainer informal protocol.
        unsafe { msg_send![super(this), initWithAccessibilityContainer: container] }
    }

    fn set_actionable(&self, actionable: bool) {
        self.ivars().actionable.set(actionable);
    }

    fn set_custom_action_labels(&self, labels: &[String]) {
        *self.ivars().custom_action_labels.borrow_mut() = labels.to_vec();
    }
}

pub(crate) struct IosAccessibilityBridge {
    host_view: Retained<UIView>,
    native_elements: HashMap<i32, Retained<NativeAccessibilityElement>>,
    snapshot: Vec<AccessibilityElement>,
    snapshot_ids: Vec<i32>,
    requests: ReaderRequests,
    wake_proxy: EventLoopProxy,
    published_once: bool,
    focused_element: Option<i32>,
    reader_cursor: Option<i32>,
}

impl IosAccessibilityBridge {
    pub(crate) fn new(event_proxy: EventLoopProxy) -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        let host_view = root_view_controller(mtm)?.view()?;
        let host_object: &NSObject = host_view.as_ref();
        host_object.setIsAccessibilityElement(false, mtm);

        Some(Self {
            host_view,
            native_elements: HashMap::new(),
            snapshot: Vec::new(),
            snapshot_ids: Vec::new(),
            requests: ReaderRequests::default(),
            wake_proxy: event_proxy,
            published_once: false,
            focused_element: None,
            reader_cursor: None,
        })
    }

    pub(crate) fn sync<R>(&mut self, shell: &mut AppShell<R>)
    where
        R: Renderer,
        R::Error: Debug,
    {
        let next = accessibility::snapshot(shell);
        self.speak(&next);
        if next == self.snapshot {
            return;
        }

        let structure_changed = !same_structure(&self.snapshot, &next);
        let next_ids = accessibility::element_ids(&next);
        let current_ids: HashSet<i32> = next_ids.iter().copied().collect();
        self.native_elements
            .retain(|element_id, _| current_ids.contains(element_id));

        let mtm = MainThreadMarker::new().expect("accessibility sync runs on UIKit's main thread");
        for (element_id, element) in next_ids.iter().zip(&next) {
            if !self.native_elements.contains_key(element_id) {
                let native = self.create_element(*element_id, mtm);
                self.native_elements.insert(*element_id, native);
            }
            let native = self
                .native_elements
                .get(element_id)
                .expect("accessibility element inserted above");
            update_native_element(native, element, mtm);
        }

        if structure_changed {
            let opened_dialog = opened_dialog(&self.snapshot, &next, &next_ids);
            self.publish_container(&next_ids, opened_dialog, mtm);
        }
        let changed = accessibility::spoken_changes(&self.snapshot, &next);
        self.snapshot = next;
        self.snapshot_ids = next_ids;
        if !self.follow_app_focus() {
            self.respeak_under_cursor(&changed);
        }
    }

    /// Hands VoiceOver text to read out: what the app asked for through
    /// [`cranpose_ui::Announcer`], and the text of any live region that
    /// changed. iOS has no live region of its own, so the change is read as an
    /// announcement. VoiceOver drops these when it is off, so the call costs
    /// nothing then.
    fn speak(&self, next: &[AccessibilityElement]) {
        let mut announcements = accessibility::drain_app_announcements();
        announcements.extend(accessibility::live_region_announcements(
            &self.snapshot,
            next,
        ));
        announcements.extend(accessibility::pane_title_announcements(
            &self.snapshot,
            next,
        ));
        for announcement in announcements {
            let text = NSString::from_str(&announcement.text);
            let argument: &AnyObject = text.as_ref();
            // SAFETY: the announcement notification takes the string to read,
            // and `text` lives until the call returns.
            unsafe {
                UIAccessibilityPostNotification(
                    UIAccessibilityAnnouncementNotification,
                    Some(argument),
                );
            }
        }
    }

    /// Moves the VoiceOver cursor onto the control the app focused, so a
    /// focus move from the keyboard or from the app reaches the reader.
    fn follow_app_focus(&mut self) -> bool {
        let focused = self
            .snapshot_ids
            .iter()
            .zip(&self.snapshot)
            .find(|(_, element)| element.focused)
            .map(|(id, _)| *id);
        if focused == self.focused_element {
            return false;
        }
        self.focused_element = focused;
        let Some(element_id) = focused else {
            return false;
        };
        self.name_to_reader(element_id)
    }

    /// Speaks the control under the VoiceOver cursor again when its words
    /// changed: a toggle that flipped, a counter that moved on. VoiceOver
    /// reads an element again when a layout change names it.
    fn respeak_under_cursor(&self, changed: &[bool]) {
        let Some(element_id) = self.reader_cursor else {
            return;
        };
        let index = self.snapshot_ids.iter().position(|id| *id == element_id);
        if index.is_some_and(|index| changed.get(index).copied().unwrap_or(false)) {
            self.name_to_reader(element_id);
        }
    }

    /// Posts a layout change that names one element, which moves the
    /// VoiceOver cursor onto it and reads it out.
    fn name_to_reader(&self, element_id: i32) -> bool {
        let Some(native) = self.native_elements.get(&element_id) else {
            return false;
        };
        let argument: &AnyObject = native.as_ref();
        // SAFETY: the notification takes the element to move the cursor to,
        // and `native` is a retained accessibility element of this container.
        unsafe {
            UIAccessibilityPostNotification(
                UIAccessibilityLayoutChangedNotification,
                Some(argument),
            );
        }
        true
    }

    /// The element a virtual id stands for in the snapshot last published.
    fn element_for(&self, element_id: i32) -> Option<&AccessibilityElement> {
        self.snapshot_ids
            .iter()
            .position(|id| *id == element_id)
            .and_then(|index| self.snapshot.get(index))
    }

    /// Hands focus to the app when VoiceOver lands its cursor on an element.
    pub(crate) fn drain_focus(&mut self) -> bool {
        let pending = self.requests.focus.take();
        let mut moved = false;
        for element_id in pending {
            let Some((node_id, focusable)) = self
                .element_for(element_id)
                .map(|element| (element.node_id, element.focusable))
            else {
                continue;
            };
            self.reader_cursor = Some(element_id);
            if !focusable {
                continue;
            }
            self.focused_element = Some(element_id);
            moved |= accessibility::focus_node(node_id);
        }
        moved
    }

    pub(crate) fn drain_activations<R>(&mut self, shell: &mut AppShell<R>) -> bool
    where
        R: Renderer,
        R::Error: Debug,
    {
        let pending = self.requests.activations.take();
        let mut changed = false;
        for element_id in pending {
            let Some(element) = self.element_for(element_id) else {
                continue;
            };
            let (x, y) = element.bounds.center();
            shell.set_pointer_source(PointerSource::Touch);
            changed |= shell.set_cursor(x, y);
            changed |= shell.pointer_pressed();
            changed |= shell.pointer_released_at_position(x, y);
        }
        changed
    }

    /// Moves the value of an adjustable control after a VoiceOver swipe up or
    /// down. Answers whether a control took the new value.
    pub(crate) fn drain_value_steps<R>(&mut self, shell: &mut AppShell<R>) -> bool
    where
        R: Renderer,
        R::Error: Debug,
    {
        let pending = self.requests.steps.take();
        if pending.is_empty() {
            return false;
        }
        let mut moved = false;
        for (element_id, up) in pending {
            let Some((node_id, progress)) = self
                .element_for(element_id)
                .map(|element| (element.node_id, element.progress))
            else {
                continue;
            };
            let Some(progress) = progress else {
                continue;
            };
            let next = accessibility::stepped_value(&progress, up);
            moved |= accessibility::run_reader_action(shell, |root| {
                accessibility::set_progress(root, node_id, next)
            });
        }
        moved
    }

    /// Pages the scroll container around the element VoiceOver holds after a
    /// three-finger swipe. Answers whether a container moved.
    pub(crate) fn drain_scrolls<R>(&mut self, shell: &mut AppShell<R>) -> bool
    where
        R: Renderer,
        R::Error: Debug,
    {
        let pending = self.requests.scrolls.take();
        if pending.is_empty() {
            return false;
        }
        let mut moved = false;
        for (element_id, forward) in pending {
            let Some((node_id, dx, dy)) = self
                .element_for(element_id)
                .and_then(|element| accessibility::scroll_container_for(&self.snapshot, element))
                .map(|container| {
                    let (dx, dy) = accessibility::page_delta(container, forward);
                    (container.node_id, dx, dy)
                })
            else {
                continue;
            };
            moved |= accessibility::run_reader_action(shell, |root| {
                accessibility::scroll_by(root, node_id, dx, dy)
            });
        }
        moved
    }

    /// Runs the custom action a VoiceOver user picked from the actions rotor,
    /// on the live tree. Answers whether a handler took it.
    pub(crate) fn drain_custom_actions<R>(&mut self, shell: &mut AppShell<R>) -> bool
    where
        R: Renderer,
        R::Error: Debug,
    {
        let pending = self.requests.custom_actions.take();
        let mut ran = false;
        for (element_id, index) in pending {
            let Some((node_id, canvas_key)) = self
                .element_for(element_id)
                .map(|element| (element.node_id, element.canvas_key))
            else {
                continue;
            };
            ran |= accessibility::run_reader_action(shell, |root| {
                accessibility::perform_custom_action(root, node_id, canvas_key, index)
            });
        }
        ran
    }

    /// Closes the dialog on top, or asks the app to go back, after a VoiceOver
    /// two-finger scrub. Answers whether anything took the request.
    pub(crate) fn drain_escapes<R>(&mut self, shell: &mut AppShell<R>) -> bool
    where
        R: Renderer,
        R::Error: Debug,
    {
        let count = self.requests.escapes.replace(0);
        let mut taken = false;
        for _ in 0..count {
            taken |= shell.dismiss_top_modal() || accessibility::request_back();
        }
        taken
    }

    fn create_element(
        &self,
        element_id: i32,
        mtm: MainThreadMarker,
    ) -> Retained<NativeAccessibilityElement> {
        let container: &AnyObject = self.host_view.as_ref();
        let native = NativeAccessibilityElement::new(
            container,
            element_id,
            self.requests.clone(),
            self.wake_proxy.clone(),
            mtm,
        );
        native.setIsAccessibilityElement(true);
        native.setAccessibilityIdentifier(Some(&NSString::from_str(&format!(
            "cranpose-node-{element_id}"
        ))));
        native
    }

    fn publish_container(
        &mut self,
        next_ids: &[i32],
        opened_dialog: Option<i32>,
        mtm: MainThreadMarker,
    ) {
        let ordered: Vec<Retained<AnyObject>> = next_ids
            .iter()
            .filter_map(|element_id| self.native_elements.get(element_id))
            .map(|element| element.retain().into())
            .collect();
        let array = NSArray::from_retained_slice(&ordered);
        let host_object: &NSObject = self.host_view.as_ref();
        // SAFETY: Every array member is a retained UIAccessibilityElement and
        // both informal-container properties accept NSArray<id>.
        unsafe {
            host_object.setAccessibilityElements(Some(&array), mtm);
            host_object.setAutomationElements(Some(&array), mtm);
        }

        let landing: Option<&AnyObject> = opened_dialog
            .and_then(|element_id| self.native_elements.get(&element_id))
            .map(|native| native.as_ref());
        // SAFETY: UIKit owns both immutable notification constants; a null
        // argument asks the accessibility service to retain its current focus,
        // and a dialog that just opened is the element it moves to.
        unsafe {
            let notification = if self.published_once && landing.is_none() {
                UIAccessibilityLayoutChangedNotification
            } else {
                UIAccessibilityScreenChangedNotification
            };
            UIAccessibilityPostNotification(notification, landing);
        }
        self.published_once = true;
    }
}

fn update_native_element(
    native: &NativeAccessibilityElement,
    element: &AccessibilityElement,
    mtm: MainThreadMarker,
) {
    native.set_actionable(element.clickable || element.role == AccessibilityRole::TextField);
    native.setIsAccessibilityElement(
        !element.label.is_empty() || element.role == AccessibilityRole::TextField,
    );
    native.setAccessibilityLabel(Some(&NSString::from_str(&element.label)));
    let place = element
        .collection_item
        .map(|item| format!("{} of {}", item.position, item.count));
    let value = element
        .value
        .clone()
        .or_else(|| element.password.then(|| "password".to_owned()))
        .or_else(|| accessibility::expansion_word(element).map(str::to_owned))
        .or_else(|| accessibility::state_with_error(element))
        .or(place);
    native.setAccessibilityValue(value.as_deref().map(NSString::from_str).as_deref());
    native.setAccessibilityHint(
        element
            .click_label
            .as_deref()
            .map(NSString::from_str)
            .as_deref(),
    );
    native.setAccessibilityFrameInContainerSpace(CGRect::new(
        CGPoint::new(element.bounds.x as f64, element.bounds.y as f64),
        CGSize::new(element.bounds.width as f64, element.bounds.height as f64),
    ));
    // SAFETY: UIKit accessibility trait constants are immutable process-wide
    // values exported by the linked framework.
    let mut traits = unsafe {
        match element.role {
            AccessibilityRole::Button
            | AccessibilityRole::Checkbox
            | AccessibilityRole::Switch
            | AccessibilityRole::RadioButton => UIAccessibilityTraitButton,
            AccessibilityRole::StaticText => UIAccessibilityTraitStaticText,
            AccessibilityRole::TextField => UIAccessibilityTraitNone,
            AccessibilityRole::Tab => UIAccessibilityTraitButton,
            AccessibilityRole::Image => UIAccessibilityTraitImage,
            AccessibilityRole::Header => UIAccessibilityTraitHeader,
            AccessibilityRole::Dialog => UIAccessibilityTraitHeader,
        }
    };
    // SAFETY: as above — immutable framework constants.
    unsafe {
        if element.selected == Some(true) {
            traits |= UIAccessibilityTraitSelected;
        }
        if !element.enabled {
            traits |= UIAccessibilityTraitNotEnabled;
        }
        if element.adjustable {
            traits |= UIAccessibilityTraitAdjustable;
        }
    }
    native.setAccessibilityTraits(traits);
    native.setAccessibilityViewIsModal(element.role == AccessibilityRole::Dialog, mtm);
    offer_custom_actions(native, element, mtm);
}

/// Lists the element's custom actions in VoiceOver's actions rotor, each one
/// aimed back at the element by name.
fn offer_custom_actions(
    native: &NativeAccessibilityElement,
    element: &AccessibilityElement,
    mtm: MainThreadMarker,
) {
    native.set_custom_action_labels(&element.custom_actions);
    let target: &AnyObject = native.as_ref();
    let actions: Vec<Retained<UIAccessibilityCustomAction>> = element
        .custom_actions
        .iter()
        .map(|label| {
            // SAFETY: the target is this element, which answers
            // performAccessibilityCustomAction: and outlives the action, and
            // the selector names that method.
            unsafe {
                UIAccessibilityCustomAction::initWithName_target_selector(
                    UIAccessibilityCustomAction::alloc(mtm),
                    &NSString::from_str(label),
                    Some(target),
                    sel!(performAccessibilityCustomAction:),
                )
            }
        })
        .collect();
    let native_object: &NSObject = native;
    let list = (!actions.is_empty()).then(|| NSArray::from_retained_slice(&actions));
    native_object.setAccessibilityCustomActions(list.as_deref(), mtm);
}

/// The virtual id of a dialog that is in the next snapshot and was not in
/// the current one: the element a reader's cursor should land on.
fn opened_dialog(
    current: &[AccessibilityElement],
    next: &[AccessibilityElement],
    next_ids: &[i32],
) -> Option<i32> {
    next.iter()
        .zip(next_ids)
        .find(|(element, _)| {
            element.role == AccessibilityRole::Dialog
                && !current.iter().any(|old| {
                    old.node_id == element.node_id && old.role == AccessibilityRole::Dialog
                })
        })
        .map(|(_, id)| *id)
}

fn same_structure(current: &[AccessibilityElement], next: &[AccessibilityElement]) -> bool {
    current.len() == next.len()
        && current.iter().zip(next).all(|(current, next)| {
            current.node_id == next.node_id
                && current.label == next.label
                && current.value == next.value
                && current.role == next.role
                && current.clickable == next.clickable
                && current.canvas_key == next.canvas_key
        })
}

#[cfg(test)]
mod tests {
    use super::same_structure;
    use crate::accessibility::{AccessibilityElement, AccessibilityRect, AccessibilityRole};

    fn element(x: f32) -> AccessibilityElement {
        AccessibilityElement {
            node_id: 7,
            label: "Library".into(),
            bounds: AccessibilityRect::new(x, 20.0, 80.0, 64.0),
            role: AccessibilityRole::Button,
            clickable: true,
            ..AccessibilityElement::default()
        }
    }

    #[test]
    fn moving_an_element_does_not_rebuild_accessibility_focus_order() {
        assert!(same_structure(&[element(0.0)], &[element(24.0)]));
    }
}
