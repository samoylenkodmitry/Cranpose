#![allow(unsafe_code)]

use std::{
    cell::{Cell, OnceCell, RefCell},
    collections::{HashMap, HashSet},
    fmt::Debug,
    rc::Rc,
};

use cranpose_app_shell::AppShell;
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
    UIAccessibilityAnnouncementNotification, UIAccessibilityCustomAction,
    UIAccessibilityDarkerSystemColorsEnabled, UIAccessibilityElement,
    UIAccessibilityIdentification, UIAccessibilityIsBoldTextEnabled,
    UIAccessibilityIsInvertColorsEnabled, UIAccessibilityIsReduceMotionEnabled,
    UIAccessibilityIsReduceTransparencyEnabled, UIAccessibilityIsVoiceOverRunning,
    UIAccessibilityLayoutChangedNotification, UIAccessibilityPostNotification,
    UIAccessibilityScreenChangedNotification, UIAccessibilityTraitAdjustable,
    UIAccessibilityTraitButton, UIAccessibilityTraitHeader, UIAccessibilityTraitImage,
    UIAccessibilityTraitLink, UIAccessibilityTraitNone, UIAccessibilityTraitNotEnabled,
    UIAccessibilityTraitSearchField, UIAccessibilityTraitSelected, UIAccessibilityTraitStaticText,
    UIAccessibilityTraitUpdatesFrequently, UIAccessibilityTraits, UIApplication,
    UIContentSizeCategory, UIContentSizeCategoryAccessibilityExtraExtraExtraLarge,
    UIContentSizeCategoryAccessibilityExtraExtraLarge,
    UIContentSizeCategoryAccessibilityExtraLarge, UIContentSizeCategoryAccessibilityLarge,
    UIContentSizeCategoryAccessibilityMedium, UIContentSizeCategoryExtraExtraExtraLarge,
    UIContentSizeCategoryExtraExtraLarge, UIContentSizeCategoryExtraLarge,
    UIContentSizeCategoryExtraSmall, UIContentSizeCategoryLarge, UIContentSizeCategoryMedium,
    UIContentSizeCategorySmall, UITextField, UIView,
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
    dismissals: Rc<RefCell<Vec<i32>>>,
    custom_actions: Rc<RefCell<Vec<(i32, usize)>>>,
    /// Rows a VoiceOver user asked a list for: the element under the cursor,
    /// and whether the last row was asked for rather than the first.
    jumps: Rc<RefCell<Vec<(i32, bool)>>>,
    /// Elements a VoiceOver user made the magic tap on.
    magic_taps: Rc<RefCell<Vec<i32>>>,
    /// The first element of the screen that declares a magic tap, which the
    /// gesture reaches from a cursor on any other element.
    screen_action: Rc<Cell<Option<i32>>>,
}

struct AccessibilityElementIvars {
    element_id: i32,
    actionable: Cell<bool>,
    dismissable: Cell<bool>,
    magic_tap: Cell<bool>,
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

        #[unsafe(method(cranposeJumpToFirstRow:))]
        fn cranpose_jump_to_first_row(&self, _action: &UIAccessibilityCustomAction) -> Bool {
            self.queue_jump(false)
        }

        #[unsafe(method(cranposeJumpToLastRow:))]
        fn cranpose_jump_to_last_row(&self, _action: &UIAccessibilityCustomAction) -> Bool {
            self.queue_jump(true)
        }

        #[unsafe(method(accessibilityPerformEscape))]
        fn accessibility_perform_escape(&self) -> Bool {
            if self.ivars().dismissable.get() {
                self.ivars()
                    .requests.dismissals
                    .borrow_mut()
                    .push(self.ivars().element_id);
                self.ivars().wake_proxy.wake_up();
                return Bool::YES;
            }
            if !accessibility::escape_has_a_taker() {
                return Bool::NO;
            }
            let escapes = &self.ivars().requests.escapes;
            escapes.set(escapes.get() + 1);
            self.ivars().wake_proxy.wake_up();
            Bool::YES
        }

        #[unsafe(method(accessibilityPerformMagicTap))]
        fn accessibility_perform_magic_tap(&self) -> Bool {
            let own = self
                .ivars()
                .magic_tap
                .get()
                .then_some(self.ivars().element_id);
            let Some(target) = own.or(self.ivars().requests.screen_action.get()) else {
                return Bool::NO;
            };
            self.ivars().requests.magic_taps.borrow_mut().push(target);
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
            dismissable: Cell::new(false),
            magic_tap: Cell::new(false),
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

    fn set_dismissable(&self, dismissable: bool) {
        self.ivars().dismissable.set(dismissable);
    }

    fn set_magic_tap(&self, magic_tap: bool) {
        self.ivars().magic_tap.set(magic_tap);
    }

    fn set_custom_action_labels(&self, labels: &[String]) {
        *self.ivars().custom_action_labels.borrow_mut() = labels.to_vec();
    }

    /// Notes that a VoiceOver user picked one end of the list this element
    /// sits in, for the frame loop to run against the live tree.
    fn queue_jump(&self, last: bool) -> Bool {
        self.ivars()
            .requests
            .jumps
            .borrow_mut()
            .push((self.ivars().element_id, last));
        self.ivars().wake_proxy.wake_up();
        Bool::YES
    }
}

pub(crate) struct IosAccessibilityBridge {
    host_view: Retained<UIView>,
    native_elements: HashMap<i32, Retained<NativeAccessibilityElement>>,
    snapshot: accessibility::AccessibilitySnapshot,
    requests: ReaderRequests,
    wake_proxy: EventLoopProxy,
    published_once: bool,
    focused_element: Option<i32>,
    reader_cursor: Option<i32>,
    /// The text field that holds app focus, by virtual id, and the keyboard's
    /// text input view that stands in for it among the elements, so VoiceOver
    /// walks and edits the text through `UITextInput`.
    reader_view: Option<(i32, Retained<AnyObject>)>,
}

impl IosAccessibilityBridge {
    pub(crate) fn new(event_proxy: EventLoopProxy) -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        let host_view = root_view_controller(mtm)?.view()?;
        host_view.setAccessibilityIgnoresInvertColors(true);
        let host_object: &NSObject = host_view.as_ref();
        host_object.setIsAccessibilityElement(false, mtm);

        Some(Self {
            host_view,
            native_elements: HashMap::new(),
            snapshot: accessibility::AccessibilitySnapshot::default(),
            requests: ReaderRequests::default(),
            wake_proxy: event_proxy,
            published_once: false,
            focused_element: None,
            reader_cursor: None,
            reader_view: None,
        })
    }

    pub(crate) fn sync<R>(&mut self, shell: &mut AppShell<R>)
    where
        R: Renderer,
        R::Error: Debug,
    {
        let reader_on = cranpose_services::AccessibilityState {
            screen_reader_on: UIAccessibilityIsVoiceOverRunning(),
        };
        if cranpose_services::set_platform_accessibility_state(reader_on) {
            shell.request_root_render();
        }
        let mtm = MainThreadMarker::new().expect("accessibility sync runs on UIKit's main thread");
        let options = system_options(mtm);
        if accessibility::apply_accessibility_options(shell, options) {
            shell.set_font_scale(options.font_scale);
        }
        let next = accessibility::snapshot(shell);
        self.speak(&next);
        let input_changed = (crate::ios_keyboard::reader_input_active()
            && reader_field(&next).is_some())
            != self.reader_view.is_some();
        if next == self.snapshot.elements && !input_changed {
            return;
        }
        accessibility::log_spoken_tree(&next);
        let structure_changed = input_changed
            || !accessibility::voiceover_same_structure(&self.snapshot.elements, &next);
        let changed = accessibility::spoken_changes(&self.snapshot.elements, &next);
        let opened = accessibility::opened_dialog(&self.snapshot.elements, &next);
        let mut next_snapshot = std::mem::take(&mut self.snapshot);
        if let Err(error) = next_snapshot.update(next) {
            self.snapshot = next_snapshot;
            log::error!("Could not publish accessibility tree: {error}");
            return;
        }
        let next = &next_snapshot.elements;
        let next_ids = &next_snapshot.ids;
        self.requests.screen_action.set(
            next.iter()
                .zip(next_ids)
                .find(|(element, _)| element.magic_tap_label.is_some())
                .map(|(_, id)| *id),
        );

        let current_ids: HashSet<i32> = next_ids.iter().copied().collect();
        self.native_elements
            .retain(|element_id, _| current_ids.contains(element_id));

        let mtm = MainThreadMarker::new().expect("accessibility sync runs on UIKit's main thread");
        for (element_id, element) in next_ids.iter().zip(next) {
            if !self.native_elements.contains_key(element_id) {
                let native = self.create_element(*element_id, mtm);
                self.native_elements.insert(*element_id, native);
            }
            let native = self
                .native_elements
                .get(element_id)
                .expect("accessibility element inserted above");
            let jumpable = accessibility::scroll_container_for(next, element)
                .is_some_and(|container| accessibility::row_count(container) > 0);
            update_native_element(native, element, jumpable, mtm);
        }
        let reader_view_changed = self.update_reader_view(next, next_ids);

        if structure_changed {
            let opened_dialog = next
                .iter()
                .zip(next_ids)
                .find(|(element, _)| Some(element.node_id) == opened)
                .map(|(_, id)| *id);
            self.publish_container(next, next_ids, opened_dialog, reader_view_changed, mtm);
        }
        self.snapshot = next_snapshot;
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
            &self.snapshot.elements,
            next,
        ));
        announcements.extend(accessibility::pane_title_announcements(
            &self.snapshot.elements,
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
            .snapshot
            .ids
            .iter()
            .zip(&self.snapshot.elements)
            .find(|(_, element)| element.focused)
            .map(|(id, _)| *id);
        if focused == self.focused_element {
            return false;
        }
        self.focused_element = focused;
        if focused == self.reader_cursor {
            return false;
        }
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
        let index = self.snapshot.ids.iter().position(|id| *id == element_id);
        if index.is_some_and(|index| changed.get(index).copied().unwrap_or(false)) {
            self.name_to_reader(element_id);
        }
    }

    fn reader_element(&self, element_id: i32) -> Option<&AnyObject> {
        self.reader_view
            .as_ref()
            .filter(|(reader_id, _)| *reader_id == element_id)
            .map(|(_, view)| &**view)
            .or_else(|| {
                self.native_elements
                    .get(&element_id)
                    .map(|native| -> &AnyObject { native.as_ref() })
            })
    }

    fn name_to_reader(&self, element_id: i32) -> bool {
        let Some(argument) = self.reader_element(element_id) else {
            return false;
        };
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
        self.snapshot.element(element_id)
    }

    pub(crate) fn drain_focus<R>(&mut self, shell: &mut AppShell<R>) -> bool
    where
        R: Renderer,
        R::Error: Debug,
    {
        let pending = self.requests.focus.take();
        let mut moved = false;
        for element_id in pending {
            let Some(node_id) = self.element_for(element_id).map(|element| element.node_id) else {
                continue;
            };
            self.reader_cursor = Some(element_id);
            moved |= shell.accessibility_reveal(node_id);
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
            changed |= shell.accessibility_activate(element.node_id, element.canvas_key);
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
                .and_then(|element| {
                    accessibility::scroll_container_for(&self.snapshot.elements, element)
                })
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

    /// Puts the first or the last row of a list in view after a VoiceOver user
    /// picked one of them from the actions rotor. Answers whether a list moved.
    pub(crate) fn drain_jumps<R>(&mut self, shell: &mut AppShell<R>) -> bool
    where
        R: Renderer,
        R::Error: Debug,
    {
        let pending = self.requests.jumps.take();
        let mut moved = false;
        for (element_id, last) in pending {
            let Some((node_id, index)) = self
                .element_for(element_id)
                .and_then(|element| {
                    accessibility::scroll_container_for(&self.snapshot.elements, element)
                })
                .and_then(|container| {
                    let rows = accessibility::row_count(container);
                    (rows > 0).then(|| (container.node_id, if last { rows - 1 } else { 0 }))
                })
            else {
                continue;
            };
            moved |= accessibility::run_reader_action(shell, |root| {
                accessibility::scroll_to_index(root, node_id, index)
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

    /// Runs the magic tap a VoiceOver user made, on the control under the
    /// cursor or on the screen's own, against the live tree. Answers whether
    /// a control took it.
    pub(crate) fn drain_magic_taps<R>(&mut self, shell: &mut AppShell<R>) -> bool
    where
        R: Renderer,
        R::Error: Debug,
    {
        let pending = self.requests.magic_taps.take();
        let mut ran = false;
        for element_id in pending {
            let Some(node_id) = self.element_for(element_id).map(|element| element.node_id) else {
                continue;
            };
            ran |= accessibility::run_reader_action(shell, |root| {
                accessibility::magic_tap(root, node_id)
            });
        }
        ran
    }

    /// Sends away the control a VoiceOver user scrubbed on with two fingers,
    /// on the live tree. Answers whether a control took it.
    pub(crate) fn drain_dismissals<R>(&mut self, shell: &mut AppShell<R>) -> bool
    where
        R: Renderer,
        R::Error: Debug,
    {
        let pending = self.requests.dismissals.take();
        let mut ran = false;
        for element_id in pending {
            let Some(node_id) = self.element_for(element_id).map(|element| element.node_id) else {
                continue;
            };
            ran |= accessibility::run_reader_action(shell, |root| {
                accessibility::dismiss(root, node_id)
            });
        }
        ran
    }

    /// Closes the dialog on top, or asks the app to go back, after a VoiceOver
    /// two-finger scrub on a control with no way out of its own. Answers
    /// whether anything took the request.
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

    fn update_reader_view(&mut self, next: &[AccessibilityElement], next_ids: &[i32]) -> bool {
        let previous = self.reader_view.as_ref().map(|(id, _)| *id);
        self.reader_view = reader_field(next).and_then(|(index, element)| {
            let id = next_ids[index];
            let frame = CGRect::new(
                CGPoint::new(element.bounds.x as f64, element.bounds.y as f64),
                CGSize::new(element.bounds.width as f64, element.bounds.height as f64),
            );
            crate::ios_keyboard::describe_for_reader(
                &element.label,
                element.click_label.as_deref(),
                element.value.as_deref().filter(|_| !element.password),
                frame,
                &self.host_view,
                &format!("cranpose-node-{id}"),
            )
            .map(|view| (id, view))
        });
        if self.reader_view.is_none() {
            crate::ios_keyboard::hide_from_reader();
        }
        previous != self.reader_view.as_ref().map(|(id, _)| *id)
    }

    fn publish_container(
        &mut self,
        next: &[AccessibilityElement],
        next_ids: &[i32],
        opened_dialog: Option<i32>,
        reader_view_changed: bool,
        mtm: MainThreadMarker,
    ) {
        let ordered: Vec<Retained<AnyObject>> = next_ids
            .iter()
            .filter_map(|element_id| {
                if let Some((reader_id, view)) = &self.reader_view
                    && reader_id == element_id
                {
                    return Some(view.clone());
                }
                self.native_elements
                    .get(element_id)
                    .map(|element| element.retain().into())
            })
            .collect();
        let array = NSArray::from_retained_slice(&ordered);
        let host_object: &NSObject = self.host_view.as_ref();
        // SAFETY: Every array member is a retained UIAccessibilityElement and
        // both informal-container properties accept NSArray<id>.
        unsafe {
            host_object.setAccessibilityElements(Some(&array), mtm);
            host_object.setAutomationElements(Some(&array), mtm);
        }

        let cursor = self.reader_cursor.and_then(|id| self.reader_element(id));
        if self.published_once
            && opened_dialog.is_none()
            && !reader_view_changed
            && cursor.is_some()
        {
            return;
        }
        let replacement =
            accessibility::voiceover_replacement_focus(next, next_ids, self.reader_cursor);
        let landing = opened_dialog
            .or(replacement)
            .and_then(|element_id| self.reader_element(element_id));
        let landing = landing.or(if reader_view_changed { cursor } else { None });
        // SAFETY: UIKit owns both immutable notification constants; a null
        // argument asks the accessibility service to retain its current focus,
        // and a dialog that just opened is the element it moves to.
        unsafe {
            let notification =
                if self.published_once && opened_dialog.is_none() && replacement.is_none() {
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
    jumpable: bool,
    mtm: MainThreadMarker,
) {
    native.set_actionable(element.clickable || element.role.is_text_field());
    native.set_dismissable(element.dismissable);
    native.set_magic_tap(element.magic_tap_label.is_some());
    native.setAccessibilityLanguage(
        element
            .language
            .as_deref()
            .map(NSString::from_str)
            .as_deref(),
        mtm,
    );
    let input_labels: Vec<Retained<NSString>> = element
        .input_labels
        .iter()
        .map(|label| NSString::from_str(label))
        .collect();
    let input_labels =
        (!input_labels.is_empty()).then(|| NSArray::from_retained_slice(&input_labels));
    // SAFETY: the array holds retained strings and lives until the call
    // returns; UIKit copies what it keeps.
    unsafe {
        native.setAccessibilityUserInputLabels(input_labels.as_deref(), mtm);
    }
    native.setIsAccessibilityElement(!element.label.is_empty() || element.role.is_text_field());
    native.setAccessibilityLabel(Some(&NSString::from_str(&element.label)));
    let value = accessibility::voiceover_value(element);
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
    let mut traits = role_traits(element.role, mtm);
    // SAFETY: UIKit accessibility trait constants are immutable process-wide
    // values exported by the linked framework.
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
    native.setAccessibilityViewIsModal(element.is_modal, mtm);
    offer_custom_actions(native, element, jumpable, mtm);
}

fn text_field_traits(mtm: MainThreadMarker) -> UIAccessibilityTraits {
    thread_local! {
        static TRAITS: OnceCell<UIAccessibilityTraits> = const { OnceCell::new() };
    }
    TRAITS.with(|traits| {
        *traits.get_or_init(|| {
            let field = UITextField::new(mtm);
            let object: &NSObject = &field;
            object.accessibilityTraits(mtm)
        })
    })
}

fn role_traits(role: AccessibilityRole, mtm: MainThreadMarker) -> UIAccessibilityTraits {
    // SAFETY: UIKit accessibility trait constants are immutable process-wide
    // values exported by the linked framework.
    unsafe {
        match role {
            AccessibilityRole::Button
            | AccessibilityRole::Checkbox
            | AccessibilityRole::Switch
            | AccessibilityRole::RadioButton
            | AccessibilityRole::Tab
            | AccessibilityRole::DropdownList => UIAccessibilityTraitButton,
            AccessibilityRole::StaticText => UIAccessibilityTraitStaticText,
            AccessibilityRole::TextField => text_field_traits(mtm),
            AccessibilityRole::Image => UIAccessibilityTraitImage,
            AccessibilityRole::ValuePicker => UIAccessibilityTraitAdjustable,
            AccessibilityRole::Header | AccessibilityRole::Dialog => UIAccessibilityTraitHeader,
            AccessibilityRole::Link
            | AccessibilityRole::SearchField
            | AccessibilityRole::ProgressBar
            | AccessibilityRole::ToggleButton
            | AccessibilityRole::Alert
            | AccessibilityRole::Toolbar
            | AccessibilityRole::Menu
            | AccessibilityRole::MenuItem
            | AccessibilityRole::TabBar
            | AccessibilityRole::List
            | AccessibilityRole::ListItem
            | AccessibilityRole::RadioGroup => named_role_traits(role, mtm),
        }
    }
}

/// The VoiceOver traits of the roles beyond Compose's own. A toolbar, a
/// menu, a tab bar and a list carry no label, so they are never elements.
fn named_role_traits(role: AccessibilityRole, mtm: MainThreadMarker) -> UIAccessibilityTraits {
    // SAFETY: UIKit accessibility trait constants are immutable process-wide
    // values exported by the linked framework.
    unsafe {
        match role {
            AccessibilityRole::Link => UIAccessibilityTraitLink,
            AccessibilityRole::SearchField => UIAccessibilityTraitSearchField,
            AccessibilityRole::ProgressBar => UIAccessibilityTraitUpdatesFrequently,
            AccessibilityRole::ToggleButton | AccessibilityRole::MenuItem => {
                UIAccessibilityTraitButton
            }
            AccessibilityRole::Alert | AccessibilityRole::ListItem => {
                UIAccessibilityTraitStaticText
            }
            AccessibilityRole::Toolbar
            | AccessibilityRole::Menu
            | AccessibilityRole::TabBar
            | AccessibilityRole::List
            | AccessibilityRole::RadioGroup => UIAccessibilityTraitNone,
            AccessibilityRole::Button
            | AccessibilityRole::StaticText
            | AccessibilityRole::TextField
            | AccessibilityRole::Checkbox
            | AccessibilityRole::Switch
            | AccessibilityRole::RadioButton
            | AccessibilityRole::Tab
            | AccessibilityRole::Image
            | AccessibilityRole::DropdownList
            | AccessibilityRole::ValuePicker
            | AccessibilityRole::Header
            | AccessibilityRole::Dialog => role_traits(role, mtm),
        }
    }
}

/// Lists the actions the element offers in VoiceOver's actions rotor, each
/// one aimed back at the element by name, and after them the two ends of the
/// list the element sits in. VoiceOver has no long press of its own, so a
/// long press is the last of the named actions.
fn offer_custom_actions(
    native: &NativeAccessibilityElement,
    element: &AccessibilityElement,
    jumpable: bool,
    mtm: MainThreadMarker,
) {
    let labels = accessibility::reader_actions(element);
    native.set_custom_action_labels(&labels);
    let mut actions: Vec<Retained<UIAccessibilityCustomAction>> = labels
        .iter()
        .map(|label| rotor_action(native, label, sel!(performAccessibilityCustomAction:), mtm))
        .collect();
    if jumpable {
        actions.push(rotor_action(
            native,
            "first row",
            sel!(cranposeJumpToFirstRow:),
            mtm,
        ));
        actions.push(rotor_action(
            native,
            "last row",
            sel!(cranposeJumpToLastRow:),
            mtm,
        ));
    }
    let native_object: &NSObject = native;
    let list = (!actions.is_empty()).then(|| NSArray::from_retained_slice(&actions));
    native_object.setAccessibilityCustomActions(list.as_deref(), mtm);
}

/// One entry of VoiceOver's actions rotor, named for the user and aimed at a
/// method of the element under the cursor.
fn rotor_action(
    native: &NativeAccessibilityElement,
    label: &str,
    selector: objc2::runtime::Sel,
    mtm: MainThreadMarker,
) -> Retained<UIAccessibilityCustomAction> {
    let target: &AnyObject = native.as_ref();
    // SAFETY: the target is this element, which answers every selector passed
    // here and outlives the action.
    unsafe {
        UIAccessibilityCustomAction::initWithName_target_selector(
            UIAccessibilityCustomAction::alloc(mtm),
            &NSString::from_str(label),
            Some(target),
            selector,
        )
    }
}

/// The text field that holds app focus and publishes its caret, with its
/// virtual id: the one VoiceOver edits through the keyboard's text input
/// view rather than through a plain element.
fn reader_field(elements: &[AccessibilityElement]) -> Option<(usize, &AccessibilityElement)> {
    elements.iter().enumerate().find(|(_, element)| {
        element.role.is_text_field() && element.focused && element.text_selection.is_some()
    })
}

/// What the person set under Settings, Accessibility and Display: the
/// Dynamic Type size as a multiplier of the default body size, and the five
/// switches the framework acts on.
fn system_options(mtm: MainThreadMarker) -> cranpose_services::AccessibilityOptions {
    let category = UIApplication::sharedApplication(mtm).preferredContentSizeCategory();
    cranpose_services::AccessibilityOptions {
        font_scale: dynamic_type_scale(&category),
        reduce_motion: UIAccessibilityIsReduceMotionEnabled(),
        reduce_transparency: UIAccessibilityIsReduceTransparencyEnabled(),
        increase_contrast: UIAccessibilityDarkerSystemColorsEnabled(),
        bold_text: UIAccessibilityIsBoldTextEnabled(),
        invert_colors: UIAccessibilityIsInvertColorsEnabled(),
    }
}

/// The body size of each Dynamic Type step over the 17 points of the
/// default step, from Apple's Human Interface Guidelines table.
fn dynamic_type_scale(category: &UIContentSizeCategory) -> f32 {
    // SAFETY: the names are UIKit's constant strings, valid for the whole
    // life of the process.
    let steps: [(&UIContentSizeCategory, f32); 12] = unsafe {
        [
            (UIContentSizeCategoryExtraSmall, 14.0),
            (UIContentSizeCategorySmall, 15.0),
            (UIContentSizeCategoryMedium, 16.0),
            (UIContentSizeCategoryLarge, 17.0),
            (UIContentSizeCategoryExtraLarge, 19.0),
            (UIContentSizeCategoryExtraExtraLarge, 21.0),
            (UIContentSizeCategoryExtraExtraExtraLarge, 23.0),
            (UIContentSizeCategoryAccessibilityMedium, 28.0),
            (UIContentSizeCategoryAccessibilityLarge, 33.0),
            (UIContentSizeCategoryAccessibilityExtraLarge, 40.0),
            (UIContentSizeCategoryAccessibilityExtraExtraLarge, 47.0),
            (UIContentSizeCategoryAccessibilityExtraExtraExtraLarge, 53.0),
        ]
    };
    steps
        .iter()
        .find(|(name, _)| category.isEqualToString(name))
        .map_or(1.0, |(_, body_points)| body_points / 17.0)
}
