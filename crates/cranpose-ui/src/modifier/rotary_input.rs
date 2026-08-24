//! Rotary input modifiers (Wear OS crown / rotating bezel).
//!
//! Mirrors Jetpack Compose for Wear OS's `Modifier.onRotaryScrollEvent` and
//! `Modifier.onPreRotaryScrollEvent`. Returning `true` from a handler consumes
//! the event and stops propagation, exactly as in Compose.
//!
//! ## Dispatch order
//!
//! Rotary events run two passes over the target node's modifier chain, matching
//! `RotaryInputModifierNode`'s documented contract:
//!
//! 1. **Capture (pre)** — root to focused node, invoking
//!    [`Modifier::on_pre_rotary_scroll_event`] handlers. An ancestor can
//!    intercept the event before the focused node sees it.
//! 2. **Bubble** — focused node to root, invoking
//!    [`Modifier::on_rotary_scroll_event`] handlers.
//!
//! The first handler that returns `true` ends both passes.

use std::{
    cell::Cell,
    fmt,
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_foundation::{
    impl_pointer_input_node, DelegatableNode, ModifierNode, ModifierNodeElement, NodeCapabilities,
    NodeState, PointerEvent, PointerEventKind, PointerInputNode, RotaryScrollEvent,
};

use super::{inspector_metadata, Modifier};

/// Which dispatch pass a rotary handler listens on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum RotaryPass {
    /// Capture pass, root to focused node (`onPreRotaryScrollEvent`).
    Pre,
    /// Bubble pass, focused node to root (`onRotaryScrollEvent`).
    Bubble,
}

impl RotaryPass {
    fn matches(self, kind: PointerEventKind) -> bool {
        matches!(
            (self, kind),
            (RotaryPass::Pre, PointerEventKind::RotaryScrollPre)
                | (RotaryPass::Bubble, PointerEventKind::RotaryScroll)
        )
    }
}

type RotaryHandler = Rc<dyn Fn(RotaryScrollEvent) -> bool>;

impl Modifier {
    /// Handles rotary scroll events (Pixel Watch crown, Galaxy Watch rotating
    /// bezel) during the **bubble** pass.
    ///
    /// The handler receives a [`RotaryScrollEvent`] whose scroll amounts are
    /// already in pixels. Return `true` to consume the event and stop it
    /// propagating to ancestors; return `false` to let it keep bubbling.
    ///
    /// Equivalent to Compose's `Modifier.onRotaryScrollEvent`.
    ///
    /// ```ignore
    /// Modifier::new().on_rotary_scroll_event(move |event| {
    ///     offset.set(offset.get() + event.vertical_scroll_pixels);
    ///     true
    /// })
    /// ```
    pub fn on_rotary_scroll_event<F>(self, handler: F) -> Self
    where
        F: Fn(RotaryScrollEvent) -> bool + 'static,
    {
        self.rotary_element(RotaryPass::Bubble, Rc::new(handler), "onRotaryScrollEvent")
    }

    /// Handles rotary scroll events during the **capture** pass, before the
    /// focused node sees them.
    ///
    /// Return `true` to consume the event and stop it reaching descendants.
    ///
    /// Equivalent to Compose's `Modifier.onPreRotaryScrollEvent`.
    pub fn on_pre_rotary_scroll_event<F>(self, handler: F) -> Self
    where
        F: Fn(RotaryScrollEvent) -> bool + 'static,
    {
        self.rotary_element(RotaryPass::Pre, Rc::new(handler), "onPreRotaryScrollEvent")
    }

    fn rotary_element(
        self,
        pass: RotaryPass,
        handler: RotaryHandler,
        inspector_name: &'static str,
    ) -> Self {
        let element = RotaryInputElement::new(pass, handler);
        let handler_id = element.handler_id;
        self.then(
            Self::with_element(element).with_inspector_metadata(inspector_metadata(
                inspector_name,
                move |info| {
                    info.add_property("handlerId", handler_id.to_string());
                },
            )),
        )
    }
}

#[derive(Clone)]
struct RotaryInputElement {
    pass: RotaryPass,
    handler: RotaryHandler,
    handler_id: u64,
}

impl RotaryInputElement {
    fn new(pass: RotaryPass, handler: RotaryHandler) -> Self {
        let handler_id = Rc::as_ptr(&handler) as *const () as usize as u64;
        Self {
            pass,
            handler,
            handler_id,
        }
    }
}

impl fmt::Debug for RotaryInputElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RotaryInputElement")
            .field("pass", &self.pass)
            .field("handler_id", &self.handler_id)
            .finish()
    }
}

impl PartialEq for RotaryInputElement {
    fn eq(&self, other: &Self) -> bool {
        // Compare only the pass, never the closure identity: handlers are
        // recreated on every recomposition, and comparing them would drop and
        // rebuild the node each frame. Matches `PointerInputElement`.
        self.pass == other.pass
    }
}

impl Eq for RotaryInputElement {}

impl Hash for RotaryInputElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.pass.hash(state);
    }
}

impl ModifierNodeElement for RotaryInputElement {
    type Node = RotaryInputModifierNode;

    fn create(&self) -> Self::Node {
        RotaryInputModifierNode::new(self.pass, self.handler.clone())
    }

    fn update(&self, node: &mut Self::Node) {
        // Always refresh the closure so the node calls the latest captured
        // state, without restarting anything.
        node.handler.set(Some(self.handler.clone()));
    }

    fn always_update(&self) -> bool {
        true
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::POINTER_INPUT
    }
}

/// Modifier node that forwards rotary scroll events to an app handler.
///
/// Named after Compose's `RotaryInputModifierNode`. It rides the existing
/// pointer dispatch path: the shell sends rotary passes as
/// [`PointerEventKind::RotaryScrollPre`]/[`PointerEventKind::RotaryScroll`]
/// pointer events, and this node filters for its own pass.
pub struct RotaryInputModifierNode {
    handler: Rc<Cell<Option<RotaryHandler>>>,
    dispatch: Rc<dyn Fn(PointerEvent)>,
    state: NodeState,
}

impl RotaryInputModifierNode {
    fn new(pass: RotaryPass, handler: RotaryHandler) -> Self {
        let handler_cell: Rc<Cell<Option<RotaryHandler>>> = Rc::new(Cell::new(Some(handler)));
        let handler_for_dispatch = Rc::clone(&handler_cell);
        // One closure allocated per node at construction time, not per event.
        let dispatch = Rc::new(move |event: PointerEvent| {
            if !pass.matches(event.kind) || event.is_consumed() {
                return;
            }
            let Some(rotary) = event.rotary_scroll_event() else {
                return;
            };
            // Take-and-restore keeps the handler behind a `Cell` (no RefCell
            // borrow can be held across the call, so a handler is free to
            // rebuild the composition).
            let Some(handler) = handler_for_dispatch.take() else {
                return;
            };
            let consumed = handler(rotary);
            if handler_for_dispatch.take().is_none() {
                // Nothing replaced it while we were running: put ours back.
                handler_for_dispatch.set(Some(handler));
            }
            if consumed {
                event.consume();
            }
        });

        Self {
            handler: handler_cell,
            dispatch,
            state: NodeState::new(),
        }
    }
}

impl ModifierNode for RotaryInputModifierNode {
    impl_pointer_input_node!();
}

impl DelegatableNode for RotaryInputModifierNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl PointerInputNode for RotaryInputModifierNode {
    fn pointer_input_handler(&self) -> Option<Rc<dyn Fn(PointerEvent)>> {
        Some(Rc::clone(&self.dispatch))
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use cranpose_ui_graphics::Point;

    use super::*;

    fn rotary_event(kind: PointerEventKind, vertical: f32) -> PointerEvent {
        PointerEvent::rotary(
            kind,
            RotaryScrollEvent::new(vertical, 0.0, 42),
            Point { x: 0.0, y: 0.0 },
        )
    }

    fn node(pass: RotaryPass, handler: RotaryHandler) -> RotaryInputModifierNode {
        RotaryInputModifierNode::new(pass, handler)
    }

    #[test]
    fn bubble_node_receives_bubble_events_only() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&seen);
        let node = node(
            RotaryPass::Bubble,
            Rc::new(move |event: RotaryScrollEvent| {
                sink.borrow_mut().push(event.vertical_scroll_pixels);
                false
            }),
        );
        let dispatch = node.pointer_input_handler().expect("handler");

        dispatch(rotary_event(PointerEventKind::RotaryScrollPre, -1.0));
        dispatch(rotary_event(PointerEventKind::RotaryScroll, -2.0));
        dispatch(PointerEvent::new(
            PointerEventKind::Scroll,
            Point { x: 0.0, y: 0.0 },
            Point { x: 0.0, y: 0.0 },
        ));

        assert_eq!(*seen.borrow(), vec![-2.0]);
    }

    #[test]
    fn pre_node_receives_capture_events_only() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&seen);
        let node = node(
            RotaryPass::Pre,
            Rc::new(move |event: RotaryScrollEvent| {
                sink.borrow_mut().push(event.vertical_scroll_pixels);
                false
            }),
        );
        let dispatch = node.pointer_input_handler().expect("handler");

        dispatch(rotary_event(PointerEventKind::RotaryScrollPre, -1.0));
        dispatch(rotary_event(PointerEventKind::RotaryScroll, -2.0));

        assert_eq!(*seen.borrow(), vec![-1.0]);
    }

    #[test]
    fn returning_true_consumes_the_event() {
        let node = node(RotaryPass::Bubble, Rc::new(|_| true));
        let dispatch = node.pointer_input_handler().expect("handler");

        let event = rotary_event(PointerEventKind::RotaryScroll, -8.0);
        dispatch(event.clone());

        assert!(event.is_consumed());
    }

    #[test]
    fn returning_false_leaves_the_event_unconsumed() {
        let node = node(RotaryPass::Bubble, Rc::new(|_| false));
        let dispatch = node.pointer_input_handler().expect("handler");

        let event = rotary_event(PointerEventKind::RotaryScroll, -8.0);
        dispatch(event.clone());

        assert!(!event.is_consumed());
    }

    #[test]
    fn an_already_consumed_event_never_reaches_the_handler() {
        // This is what stops propagation: once an inner node consumed the
        // event, outer nodes in the same pass must not run.
        let calls = Rc::new(Cell::new(0));
        let counter = Rc::clone(&calls);
        let node = node(
            RotaryPass::Bubble,
            Rc::new(move |_| {
                counter.set(counter.get() + 1);
                false
            }),
        );
        let dispatch = node.pointer_input_handler().expect("handler");

        let event = rotary_event(PointerEventKind::RotaryScroll, -8.0);
        event.consume();
        dispatch(event);

        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn handler_sees_the_full_rotary_payload() {
        let captured = Rc::new(Cell::new(None));
        let sink = Rc::clone(&captured);
        let node = node(
            RotaryPass::Bubble,
            Rc::new(move |event: RotaryScrollEvent| {
                sink.set(Some(event));
                true
            }),
        );
        let dispatch = node.pointer_input_handler().expect("handler");

        dispatch(PointerEvent::rotary(
            PointerEventKind::RotaryScroll,
            RotaryScrollEvent::new(-64.0, 32.0, 777),
            Point { x: 0.0, y: 0.0 },
        ));

        assert_eq!(
            captured.get(),
            Some(RotaryScrollEvent::new(-64.0, 32.0, 777))
        );
    }

    #[test]
    fn element_reuses_the_node_across_recomposition() {
        // Equal elements (same pass) must not churn the node, but the closure
        // must still be refreshed so it observes the latest state.
        let first = RotaryInputElement::new(RotaryPass::Bubble, Rc::new(|_| false));
        let second = RotaryInputElement::new(RotaryPass::Bubble, Rc::new(|_| true));
        assert_eq!(first, second);
        assert!(first.always_update());

        let mut node = first.create();
        second.update(&mut node);

        let event = rotary_event(PointerEventKind::RotaryScroll, -1.0);
        node.pointer_input_handler().expect("handler")(event.clone());

        assert!(event.is_consumed(), "updated handler should be in effect");
    }

    #[test]
    fn pre_and_bubble_elements_are_distinct() {
        let pre = RotaryInputElement::new(RotaryPass::Pre, Rc::new(|_| false));
        let bubble = RotaryInputElement::new(RotaryPass::Bubble, Rc::new(|_| false));

        assert_ne!(pre, bubble);
    }

    #[test]
    fn modifier_builders_add_pointer_input_elements() {
        let modifier = Modifier::empty()
            .on_pre_rotary_scroll_event(|_| false)
            .on_rotary_scroll_event(|_| true);

        assert_eq!(modifier.elements().len(), 2);
    }
}
