use std::{
    cell::{Cell, RefCell},
    hash::{Hash, Hasher},
    rc::{Rc, Weak},
};

use cranpose_core::{NodeId, current_runtime_handle, internal::FrameCallbackRegistration};
use cranpose_foundation::{
    DRAG_THRESHOLD, DelegatableNode, ModifierNode, ModifierNodeContext, ModifierNodeElement,
    NodeCapabilities, NodeState, PointerEvent, PointerEventKind, PointerInputNode,
    gesture_constants::LONG_PRESS_TIMEOUT_NANOS, impl_pointer_input_node,
};
use cranpose_ui_graphics::Point;

use super::{Modifier, inspector_metadata};
use crate::render_state::{AppContext, current_app_context};

impl Modifier {
    /// Calls `action` when a press rests on this node for the long press
    /// time without moving, and offers a screen reader the same action
    /// under `label`, as Compose's `combinedClickable(onLongClick)` does.
    ///
    /// The press then belongs to this node: the controls inside it see the
    /// press cancelled and do not click when it ends.
    ///
    /// Example: `Modifier::empty().on_long_press("Show tooltip", move || shown.set(true))`
    pub fn on_long_press(self, label: impl Into<String>, action: impl Fn() + 'static) -> Self {
        let action: Rc<dyn Fn()> = Rc::new(action);
        let reader = Rc::clone(&action);
        self.then(
            Self::with_element(LongPressElement { action })
                .with_inspector_metadata(inspector_metadata("longPress", |_| {})),
        )
        .on_long_click(label, move || {
            reader();
            true
        })
    }
}

#[derive(Clone)]
struct LongPressElement {
    action: Rc<dyn Fn()>,
}

impl std::fmt::Debug for LongPressElement {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("LongPressElement")
    }
}

impl PartialEq for LongPressElement {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.action, &other.action)
    }
}

impl Hash for LongPressElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "longPress".hash(state);
    }
}

impl ModifierNodeElement for LongPressElement {
    type Node = LongPressNode;

    fn create(&self) -> Self::Node {
        let press = Rc::new(Press::new(Rc::clone(&self.action)));
        let dispatch_press = Rc::clone(&press);
        LongPressNode {
            dispatch: Rc::new(move |event| Press::on_event(&dispatch_press, &event)),
            press,
            state: NodeState::new(),
        }
    }

    fn update(&self, node: &mut Self::Node) {
        *node.press.action.borrow_mut() = Rc::clone(&self.action);
    }

    fn always_update(&self) -> bool {
        true
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::POINTER_INPUT
    }
}

/// One held press and the frame timer that turns it into a long press.
struct Press {
    action: RefCell<Rc<dyn Fn()>>,
    node: Cell<Option<NodeId>>,
    /// Where the press went down, while it is held and still a press.
    origin: Cell<Option<Point>>,
    /// The first frame time after the press went down: frames and events
    /// keep different clocks.
    started_nanos: Cell<Option<u64>>,
    fired: Cell<bool>,
    context: RefCell<Weak<AppContext>>,
    timer: RefCell<Option<FrameCallbackRegistration>>,
}

impl Press {
    fn new(action: Rc<dyn Fn()>) -> Self {
        Self {
            action: RefCell::new(action),
            node: Cell::new(None),
            origin: Cell::new(None),
            started_nanos: Cell::new(None),
            fired: Cell::new(false),
            context: RefCell::new(Weak::new()),
            timer: RefCell::new(None),
        }
    }

    fn on_event(press: &Rc<Self>, event: &PointerEvent) {
        if !event.id.is_primary() {
            return;
        }
        match event.kind {
            PointerEventKind::Down if !event.is_consumed() => Self::begin(press, event),
            PointerEventKind::Move if press.fired.get() => event.consume(),
            PointerEventKind::Move => {
                let moved = press.origin.get().is_some_and(|origin| {
                    let to = event.global_position;
                    (to.x - origin.x).hypot(to.y - origin.y) > DRAG_THRESHOLD
                });
                if moved || event.is_consumed() {
                    press.end();
                }
            }
            PointerEventKind::Up => {
                if press.fired.get() {
                    event.consume();
                }
                press.end();
            }
            PointerEventKind::Cancel => press.end(),
            _ => {}
        }
    }

    fn begin(press: &Rc<Self>, event: &PointerEvent) {
        press.end();
        let Some(runtime) = current_runtime_handle() else {
            return;
        };
        press.origin.set(Some(event.global_position));
        *press.context.borrow_mut() = current_app_context()
            .as_ref()
            .map_or_else(Weak::new, Rc::downgrade);
        Self::watch(press, &runtime);
    }

    /// Checks on each frame whether the press has rested long enough.
    fn watch(press: &Rc<Self>, runtime: &cranpose_core::RuntimeHandle) {
        let weak = Rc::downgrade(press);
        let next = runtime.clone();
        let registration = runtime.frame_clock().with_frame_nanos(move |frame_nanos| {
            let Some(press) = weak.upgrade() else {
                return;
            };
            press.timer.borrow_mut().take();
            if press.origin.get().is_none() {
                return;
            }
            let started = press.started_nanos.get().unwrap_or(frame_nanos);
            press.started_nanos.set(Some(started));
            if frame_nanos.saturating_sub(started) < LONG_PRESS_TIMEOUT_NANOS {
                Self::watch(&press, &next);
                return;
            }
            press.fired.set(true);
            if let (Some(context), Some(node)) =
                (press.context.borrow().upgrade(), press.node.get())
            {
                context.claim_pointer(node);
            }
            let action = Rc::clone(&press.action.borrow());
            action();
        });
        *press.timer.borrow_mut() = Some(registration);
    }

    fn end(&self) {
        self.timer.borrow_mut().take();
        self.origin.set(None);
        self.started_nanos.set(None);
        self.fired.set(false);
    }
}

struct LongPressNode {
    dispatch: Rc<dyn Fn(PointerEvent)>,
    press: Rc<Press>,
    state: NodeState,
}

impl ModifierNode for LongPressNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        self.state.set_attached(true);
        self.press.node.set(context.node_id());
        context.invalidate(cranpose_foundation::InvalidationKind::PointerInput);
    }

    fn on_detach(&mut self) {
        self.state.set_attached(false);
        self.press.end();
        self.press.node.set(None);
    }

    impl_pointer_input_node!();
}

impl DelegatableNode for LongPressNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl PointerInputNode for LongPressNode {
    fn pointer_input_handler(&self) -> Option<Rc<dyn Fn(PointerEvent)>> {
        Some(Rc::clone(&self.dispatch))
    }
}
