//! UIKit accessibility bridge for the retained Cranpose semantics tree.
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
    define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, Bool},
    DefinedClass, MainThreadMarker, MainThreadOnly, Message,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSString};
use objc2_ui_kit::{
    NSObjectUIAccessibility, NSObjectUIAccessibilityContainer, UIAccessibilityElement,
    UIAccessibilityIdentification, UIAccessibilityLayoutChangedNotification,
    UIAccessibilityPostNotification, UIAccessibilityScreenChangedNotification,
    UIAccessibilityTraitButton, UIAccessibilityTraitHeader, UIAccessibilityTraitImage,
    UIAccessibilityTraitNone, UIAccessibilityTraitNotEnabled, UIAccessibilityTraitSelected,
    UIAccessibilityTraitStaticText, UIView,
};
use winit::event_loop::EventLoopProxy;

use crate::{
    accessibility::{self, AccessibilityElement, AccessibilityRole},
    ios_file_picker::root_view_controller,
};

struct AccessibilityElementIvars {
    /// The shared accessibility element id, not the layout node id: a node
    /// that publishes drawn controls owns several elements, and VoiceOver has
    /// to activate the one it is focused on.
    element_id: i32,
    actionable: Cell<bool>,
    pending_activations: Rc<RefCell<Vec<i32>>>,
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
                .pending_activations
                .borrow_mut()
                .push(self.ivars().element_id);
            self.ivars().wake_proxy.wake_up();
            Bool::YES
        }
    }
);

impl NativeAccessibilityElement {
    fn new(
        container: &AnyObject,
        element_id: i32,
        pending_activations: Rc<RefCell<Vec<i32>>>,
        wake_proxy: EventLoopProxy,
        mtm: MainThreadMarker,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(AccessibilityElementIvars {
            element_id,
            actionable: Cell::new(false),
            pending_activations,
            wake_proxy,
        });
        // SAFETY: `container` is the retained winit root UIView and implements
        // the UIAccessibilityContainer informal protocol.
        unsafe { msg_send![super(this), initWithAccessibilityContainer: container] }
    }

    fn set_actionable(&self, actionable: bool) {
        self.ivars().actionable.set(actionable);
    }
}

/// Owns UIKit's retained accessibility elements and dispatches their actions.
pub(crate) struct IosAccessibilityBridge {
    host_view: Retained<UIView>,
    native_elements: HashMap<i32, Retained<NativeAccessibilityElement>>,
    snapshot: Vec<AccessibilityElement>,
    snapshot_ids: Vec<i32>,
    pending_activations: Rc<RefCell<Vec<i32>>>,
    wake_proxy: EventLoopProxy,
    published_once: bool,
}

impl IosAccessibilityBridge {
    /// Attaches an accessibility container to winit's root UIKit view.
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
            pending_activations: Rc::new(RefCell::new(Vec::new())),
            wake_proxy: event_proxy,
            published_once: false,
        })
    }

    /// Reconciles native elements with the current retained semantics snapshot.
    pub(crate) fn sync<R>(&mut self, shell: &mut AppShell<R>)
    where
        R: Renderer,
        R::Error: Debug,
    {
        let next = accessibility::snapshot(shell);
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
            self.publish_container(&next_ids, mtm);
        }
        self.snapshot = next;
        self.snapshot_ids = next_ids;
    }

    /// Runs queued VoiceOver/XCTest activations through Cranpose pointer input.
    pub(crate) fn drain_activations<R>(&mut self, shell: &mut AppShell<R>) -> bool
    where
        R: Renderer,
        R::Error: Debug,
    {
        let pending = self.pending_activations.take();
        let mut changed = false;
        for element_id in pending {
            let Some(element) = self
                .snapshot_ids
                .iter()
                .position(|id| *id == element_id)
                .and_then(|index| self.snapshot.get(index))
            else {
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

    fn create_element(
        &self,
        element_id: i32,
        mtm: MainThreadMarker,
    ) -> Retained<NativeAccessibilityElement> {
        let container: &AnyObject = self.host_view.as_ref();
        let native = NativeAccessibilityElement::new(
            container,
            element_id,
            Rc::clone(&self.pending_activations),
            self.wake_proxy.clone(),
            mtm,
        );
        native.setIsAccessibilityElement(true);
        native.setAccessibilityIdentifier(Some(&NSString::from_str(&format!(
            "cranpose-node-{element_id}"
        ))));
        native
    }

    fn publish_container(&mut self, next_ids: &[i32], mtm: MainThreadMarker) {
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

        // SAFETY: UIKit owns both immutable notification constants; a null
        // argument asks the accessibility service to retain its current focus.
        unsafe {
            let notification = if self.published_once {
                UIAccessibilityLayoutChangedNotification
            } else {
                UIAccessibilityScreenChangedNotification
            };
            UIAccessibilityPostNotification(notification, None);
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
    native.setAccessibilityLabel(Some(&NSString::from_str(&element.label)));
    // VoiceOver reads the value after the label, which is where Compose's
    // `stateDescription` belongs; a text field's own text still wins, since it
    // is the value in the literal sense.
    let value = element
        .value
        .as_deref()
        .or(element.state_description.as_deref());
    native.setAccessibilityValue(value.map(NSString::from_str).as_deref());
    // The click label is a verb phrase ("Pause"), which is exactly what a hint
    // is for: VoiceOver reads it as what activating will do.
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
            // VoiceOver treats a modal container as a header-like landmark it
            // announces on entry; the modality itself is set below through
            // `accessibilityViewIsModal` on the element itself, since
            // Cranpose publishes a flat sibling array rather than nested
            // views for `accessibilityViewIsModal` to scope by containment.
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
    }
    native.setAccessibilityTraits(traits);
    // While a dialog element exists in the published array, VoiceOver must
    // ignore its siblings in that same array — the rest of the screen behind
    // the dialog — exactly as `aria-modal` does for the web backend's
    // `AccessibilityRole::Dialog` case.
    native.setAccessibilityViewIsModal(element.role == AccessibilityRole::Dialog, mtm);
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
