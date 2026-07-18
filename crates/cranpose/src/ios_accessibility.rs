//! UIKit accessibility bridge for the retained Cranpose semantics tree.
#![allow(unsafe_code)]

use crate::accessibility::{self, AccessibilityElement, AccessibilityRole};
use crate::ios_file_picker::root_view_controller;
use cranpose_app_shell::{AppShell, PointerSource};
use cranpose_core::NodeId;
use cranpose_render_common::Renderer;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSString};
use objc2_ui_kit::{
    NSObjectUIAccessibility, NSObjectUIAccessibilityContainer, UIAccessibilityElement,
    UIAccessibilityIdentification, UIAccessibilityLayoutChangedNotification,
    UIAccessibilityPostNotification, UIAccessibilityScreenChangedNotification,
    UIAccessibilityTraitButton, UIAccessibilityTraitNone, UIAccessibilityTraitStaticText, UIView,
};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::rc::Rc;
use winit::event_loop::EventLoopProxy;

struct AccessibilityElementIvars {
    node_id: NodeId,
    actionable: Cell<bool>,
    pending_activations: Rc<RefCell<Vec<NodeId>>>,
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
                .push(self.ivars().node_id);
            self.ivars().wake_proxy.wake_up();
            Bool::YES
        }
    }
);

impl NativeAccessibilityElement {
    fn new(
        container: &AnyObject,
        node_id: NodeId,
        pending_activations: Rc<RefCell<Vec<NodeId>>>,
        wake_proxy: EventLoopProxy,
        mtm: MainThreadMarker,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(AccessibilityElementIvars {
            node_id,
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
    native_elements: HashMap<NodeId, Retained<NativeAccessibilityElement>>,
    snapshot: Vec<AccessibilityElement>,
    pending_activations: Rc<RefCell<Vec<NodeId>>>,
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
        let current_ids: HashSet<NodeId> = next.iter().map(|element| element.node_id).collect();
        self.native_elements
            .retain(|node_id, _| current_ids.contains(node_id));

        let mtm = MainThreadMarker::new().expect("accessibility sync runs on UIKit's main thread");
        for element in &next {
            if !self.native_elements.contains_key(&element.node_id) {
                let native = self.create_element(element, mtm);
                self.native_elements.insert(element.node_id, native);
            }
            let native = self
                .native_elements
                .get(&element.node_id)
                .expect("accessibility element inserted above");
            update_native_element(native, element);
        }

        if structure_changed {
            self.publish_container(&next, mtm);
        }
        self.snapshot = next;
    }

    /// Runs queued VoiceOver/XCTest activations through Cranpose pointer input.
    pub(crate) fn drain_activations<R>(&mut self, shell: &mut AppShell<R>) -> bool
    where
        R: Renderer,
        R::Error: Debug,
    {
        let pending = self.pending_activations.take();
        let mut changed = false;
        for node_id in pending {
            let Some(element) = self
                .snapshot
                .iter()
                .find(|element| element.node_id == node_id)
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
        element: &AccessibilityElement,
        mtm: MainThreadMarker,
    ) -> Retained<NativeAccessibilityElement> {
        let container: &AnyObject = self.host_view.as_ref();
        let native = NativeAccessibilityElement::new(
            container,
            element.node_id,
            Rc::clone(&self.pending_activations),
            self.wake_proxy.clone(),
            mtm,
        );
        native.setIsAccessibilityElement(true);
        native.setAccessibilityIdentifier(Some(&NSString::from_str(&format!(
            "cranpose-node-{}",
            element.node_id
        ))));
        native
    }

    fn publish_container(&mut self, next: &[AccessibilityElement], mtm: MainThreadMarker) {
        let ordered: Vec<Retained<AnyObject>> = next
            .iter()
            .filter_map(|element| self.native_elements.get(&element.node_id))
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

fn update_native_element(native: &NativeAccessibilityElement, element: &AccessibilityElement) {
    native.set_actionable(element.clickable || element.role == AccessibilityRole::TextField);
    native.setAccessibilityLabel(Some(&NSString::from_str(&element.label)));
    native.setAccessibilityValue(element.value.as_deref().map(NSString::from_str).as_deref());
    native.setAccessibilityFrameInContainerSpace(CGRect::new(
        CGPoint::new(element.bounds.x as f64, element.bounds.y as f64),
        CGSize::new(element.bounds.width as f64, element.bounds.height as f64),
    ));
    // SAFETY: UIKit accessibility trait constants are immutable process-wide
    // values exported by the linked framework.
    let traits = unsafe {
        match element.role {
            AccessibilityRole::Button => UIAccessibilityTraitButton,
            AccessibilityRole::StaticText => UIAccessibilityTraitStaticText,
            AccessibilityRole::TextField => UIAccessibilityTraitNone,
        }
    };
    native.setAccessibilityTraits(traits);
}

fn same_structure(current: &[AccessibilityElement], next: &[AccessibilityElement]) -> bool {
    current.len() == next.len()
        && current.iter().zip(next).all(|(current, next)| {
            current.node_id == next.node_id
                && current.label == next.label
                && current.value == next.value
                && current.role == next.role
                && current.clickable == next.clickable
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
            value: None,
            bounds: AccessibilityRect::new(x, 20.0, 80.0, 64.0),
            role: AccessibilityRole::Button,
            clickable: true,
        }
    }

    #[test]
    fn moving_an_element_does_not_rebuild_accessibility_focus_order() {
        assert!(same_structure(&[element(0.0)], &[element(24.0)]));
    }
}
