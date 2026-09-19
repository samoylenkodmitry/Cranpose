//! Drag and drop between nodes, across windows. A `drag_and_drop_source`
//! starts a transfer once a press on it moves past the drag threshold; the
//! shell then routes the transfer by screen position to the
//! `drag_and_drop_target` under the pointer in whichever surface draws it,
//! and the target hears enter, move, exit and drop while the source hears
//! how the transfer ended. The nodes know only their own handlers; the
//! state here is what the shell drives with [`DragAndDropState::route`].

use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::HashMap,
    fmt,
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_core::NodeId;
use cranpose_foundation::{
    DelegatableNode, InvalidationKind, ModifierNode, ModifierNodeContext, ModifierNodeElement,
    NodeCapabilities, NodeState, PointerEvent, PointerEventKind, PointerInputNode,
    gesture_constants::DRAG_THRESHOLD,
};

use super::{Modifier, Point};
use crate::render_state::{AppContextId, current_app_context, with_app_context_by_id};

/// What a transfer carries: any value the source chose, which a target
/// downcasts to the type it expects.
pub type DragAndDropPayload = Rc<dyn Any>;

type PayloadHandler = Rc<dyn Fn(&DragAndDropPayload)>;
type PayloadAtHandler = Rc<dyn Fn(&DragAndDropPayload, Point)>;
type DropHandler = Rc<dyn Fn(&DragAndDropPayload, Point) -> bool>;
type PointHandler = Rc<dyn Fn(DragAndDropPoint)>;

/// How a transfer ended, as the source hears it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragAndDropOutcome {
    /// A target accepted the drop.
    Dropped,
    /// The pointer was released over no target, or the target declined.
    Missed,
    /// The gesture was cancelled before a release.
    Cancelled,
}

/// Where a transfer is: on the screen when the platform knows window
/// positions, and in the source's surface always.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DragAndDropPoint {
    /// The pointer on the screen, in logical pixels, when known.
    pub screen: Option<Point>,
    /// The pointer in the surface that holds the press.
    pub local: Point,
}

/// A payload a node offers to be dragged out of it, and what the node
/// wants to hear about the transfer.
#[derive(Clone)]
pub struct DragAndDropSource {
    payload: DragAndDropPayload,
    on_started: Option<PointHandler>,
    on_moved: Option<PointHandler>,
    on_ended: Option<Rc<dyn Fn(DragAndDropOutcome)>>,
}

impl DragAndDropSource {
    /// Offers `payload` from the node; a target downcasts it.
    pub fn new(payload: impl Any) -> Self {
        Self {
            payload: Rc::new(payload),
            on_started: None,
            on_moved: None,
            on_ended: None,
        }
    }

    /// Called once the press has moved past the drag threshold.
    pub fn on_started(mut self, handler: impl Fn(DragAndDropPoint) + 'static) -> Self {
        self.on_started = Some(Rc::new(handler));
        self
    }

    /// Called with every pointer sample while the transfer runs.
    pub fn on_moved(mut self, handler: impl Fn(DragAndDropPoint) + 'static) -> Self {
        self.on_moved = Some(Rc::new(handler));
        self
    }

    /// Called when the transfer ends, with how it ended.
    pub fn on_ended(mut self, handler: impl Fn(DragAndDropOutcome) + 'static) -> Self {
        self.on_ended = Some(Rc::new(handler));
        self
    }
}

impl fmt::Debug for DragAndDropSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DragAndDropSource").finish_non_exhaustive()
    }
}

/// What a node does with a transfer that reaches it. Positions are in the
/// surface that draws the target.
#[derive(Clone, Default)]
pub struct DragAndDropTarget {
    on_entered: Option<PayloadHandler>,
    on_moved: Option<PayloadAtHandler>,
    on_exited: Option<PayloadHandler>,
    on_drop: Option<DropHandler>,
}

impl DragAndDropTarget {
    /// A target with no handlers yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Called when a transfer arrives over the node.
    pub fn on_entered(mut self, handler: impl Fn(&DragAndDropPayload) + 'static) -> Self {
        self.on_entered = Some(Rc::new(handler));
        self
    }

    /// Called with every pointer sample over the node.
    pub fn on_moved(mut self, handler: impl Fn(&DragAndDropPayload, Point) + 'static) -> Self {
        self.on_moved = Some(Rc::new(handler));
        self
    }

    /// Called when a transfer leaves the node without dropping.
    pub fn on_exited(mut self, handler: impl Fn(&DragAndDropPayload) + 'static) -> Self {
        self.on_exited = Some(Rc::new(handler));
        self
    }

    /// Called on release over the node; returns whether the drop was
    /// accepted, which the source hears as [`DragAndDropOutcome::Dropped`].
    pub fn on_drop(
        mut self,
        handler: impl Fn(&DragAndDropPayload, Point) -> bool + 'static,
    ) -> Self {
        self.on_drop = Some(Rc::new(handler));
        self
    }
}

impl fmt::Debug for DragAndDropTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DragAndDropTarget").finish_non_exhaustive()
    }
}

/// One step of a transfer the source recorded for the shell to route.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DragAndDropEvent {
    /// The press moved past the drag threshold.
    Started(DragAndDropPoint),
    /// The pointer moved while the transfer runs.
    Moved(DragAndDropPoint),
    /// The pointer was released.
    Dropped(DragAndDropPoint),
    /// The gesture was cancelled.
    Cancelled,
}

struct Transfer {
    source: DragAndDropSource,
    over: Option<NodeId>,
}

/// The one transfer in flight in an app context, the targets attached in
/// it, and the steps the source recorded since the shell last routed.
#[derive(Default)]
pub struct DragAndDropState {
    targets: RefCell<HashMap<NodeId, DragAndDropTarget>>,
    transfer: RefCell<Option<Transfer>>,
    pending: RefCell<Vec<DragAndDropEvent>>,
}

impl DragAndDropState {
    fn register_target(&self, node: NodeId, target: DragAndDropTarget) {
        self.targets.borrow_mut().insert(node, target);
    }

    fn unregister_target(&self, node: NodeId) {
        self.targets.borrow_mut().remove(&node);
        let mut transfer = self.transfer.borrow_mut();
        if let Some(transfer) = transfer.as_mut()
            && transfer.over == Some(node)
        {
            transfer.over = None;
        }
    }

    /// Whether `node` carries a `drag_and_drop_target` right now.
    pub fn is_target(&self, node: NodeId) -> bool {
        self.targets.borrow().contains_key(&node)
    }

    /// Whether a transfer is in flight.
    pub fn is_active(&self) -> bool {
        self.transfer.borrow().is_some()
    }

    fn start(&self, source: DragAndDropSource, point: DragAndDropPoint) {
        *self.transfer.borrow_mut() = Some(Transfer { source, over: None });
        self.pending
            .borrow_mut()
            .push(DragAndDropEvent::Started(point));
    }

    fn record(&self, event: DragAndDropEvent) {
        if self.transfer.borrow().is_some() {
            self.pending.borrow_mut().push(event);
        }
    }

    /// Delivers every step recorded since the last call. `find_target`
    /// names the target node under a point and the point in that node's
    /// surface; the shell answers it from its surfaces and their positions
    /// on the screen. Returns whether anything was delivered.
    pub fn route(
        &self,
        mut find_target: impl FnMut(DragAndDropPoint) -> Option<(NodeId, Point)>,
    ) -> bool {
        let events = std::mem::take(&mut *self.pending.borrow_mut());
        let routed = !events.is_empty();
        for event in events {
            match event {
                DragAndDropEvent::Started(point) => {
                    self.tell_source(|source| source.on_started.clone(), point);
                    self.hover(find_target(point));
                }
                DragAndDropEvent::Moved(point) => {
                    self.tell_source(|source| source.on_moved.clone(), point);
                    self.hover(find_target(point));
                }
                DragAndDropEvent::Dropped(point) => {
                    let hit = find_target(point);
                    self.hover(hit);
                    let accepted = hit.is_some_and(|(node, local)| self.drop_on(node, local));
                    self.finish(if accepted {
                        DragAndDropOutcome::Dropped
                    } else {
                        DragAndDropOutcome::Missed
                    });
                }
                DragAndDropEvent::Cancelled => {
                    self.hover(None);
                    self.finish(DragAndDropOutcome::Cancelled);
                }
            }
        }
        routed
    }

    fn tell_source(
        &self,
        handler: impl Fn(&DragAndDropSource) -> Option<PointHandler>,
        point: DragAndDropPoint,
    ) {
        let handler = self
            .transfer
            .borrow()
            .as_ref()
            .and_then(|transfer| handler(&transfer.source));
        if let Some(handler) = handler {
            handler(point);
        }
    }

    fn payload(&self) -> Option<DragAndDropPayload> {
        self.transfer
            .borrow()
            .as_ref()
            .map(|transfer| Rc::clone(&transfer.source.payload))
    }

    fn target(&self, node: NodeId) -> Option<DragAndDropTarget> {
        self.targets.borrow().get(&node).cloned()
    }

    fn hover(&self, hit: Option<(NodeId, Point)>) {
        let Some(payload) = self.payload() else {
            return;
        };
        let previous = self
            .transfer
            .borrow()
            .as_ref()
            .and_then(|transfer| transfer.over);
        let current = hit.map(|(node, _)| node);
        if previous != current {
            if let Some(exited) = previous.and_then(|node| self.target(node))
                && let Some(on_exited) = exited.on_exited
            {
                on_exited(&payload);
            }
            if let Some(entered) = current.and_then(|node| self.target(node))
                && let Some(on_entered) = entered.on_entered
            {
                on_entered(&payload);
            }
            if let Some(transfer) = self.transfer.borrow_mut().as_mut() {
                transfer.over = current;
            }
        }
        if let Some((node, local)) = hit
            && let Some(on_moved) = self.target(node).and_then(|target| target.on_moved)
        {
            on_moved(&payload, local);
        }
    }

    fn drop_on(&self, node: NodeId, local: Point) -> bool {
        let Some(payload) = self.payload() else {
            return false;
        };
        self.target(node)
            .and_then(|target| target.on_drop)
            .is_some_and(|on_drop| on_drop(&payload, local))
    }

    fn finish(&self, outcome: DragAndDropOutcome) {
        let transfer = self.transfer.borrow_mut().take();
        if let Some(on_ended) = transfer.and_then(|transfer| transfer.source.on_ended) {
            on_ended(outcome);
        }
    }
}

#[derive(Default)]
struct SourceGesture {
    press: Option<Point>,
    dragging: bool,
}

impl SourceGesture {
    fn on_event(
        &mut self,
        event: &PointerEvent,
        source: &DragAndDropSource,
        state: &DragAndDropState,
    ) {
        let point = DragAndDropPoint {
            screen: event.screen_position,
            local: event.global_position,
        };
        match event.kind {
            PointerEventKind::Down => {
                self.press = Some(event.global_position);
                self.dragging = false;
            }
            PointerEventKind::Move => self.on_move(event, source, state, point),
            PointerEventKind::Up => {
                if self.dragging {
                    state.record(DragAndDropEvent::Dropped(point));
                    event.consume();
                }
                *self = Self::default();
            }
            PointerEventKind::Cancel => {
                if self.dragging {
                    state.record(DragAndDropEvent::Cancelled);
                }
                *self = Self::default();
            }
            PointerEventKind::Scroll
            | PointerEventKind::Zoom
            | PointerEventKind::RotaryScrollPre
            | PointerEventKind::RotaryScroll
            | PointerEventKind::Enter
            | PointerEventKind::Exit => {}
        }
    }

    fn on_move(
        &mut self,
        event: &PointerEvent,
        source: &DragAndDropSource,
        state: &DragAndDropState,
        point: DragAndDropPoint,
    ) {
        let Some(press) = self.press else {
            return;
        };
        if self.dragging {
            state.record(DragAndDropEvent::Moved(point));
            event.consume();
            return;
        }
        let dx = event.global_position.x - press.x;
        let dy = event.global_position.y - press.y;
        if (dx * dx + dy * dy).sqrt() > DRAG_THRESHOLD {
            self.dragging = true;
            state.start(source.clone(), point);
            event.consume();
        }
    }
}

type PointerHandler = Rc<dyn Fn(PointerEvent)>;

fn source_handler(
    source: Rc<RefCell<DragAndDropSource>>,
    gesture: Rc<RefCell<SourceGesture>>,
) -> PointerHandler {
    Rc::new(move |event: PointerEvent| {
        let Some(context) = current_app_context() else {
            return;
        };
        let source = source.borrow().clone();
        gesture
            .borrow_mut()
            .on_event(&event, &source, context.drag_and_drop());
    })
}

/// Node that starts a transfer when a press on it becomes a drag.
pub struct DragAndDropSourceNode {
    source: Rc<RefCell<DragAndDropSource>>,
    handler: PointerHandler,
    state: NodeState,
}

impl fmt::Debug for DragAndDropSourceNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DragAndDropSourceNode")
            .finish_non_exhaustive()
    }
}

impl DelegatableNode for DragAndDropSourceNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for DragAndDropSourceNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(InvalidationKind::PointerInput);
    }

    cranpose_foundation::impl_modifier_node!(pointer_input);
}

impl PointerInputNode for DragAndDropSourceNode {
    fn on_pointer_event(
        &mut self,
        _context: &mut dyn ModifierNodeContext,
        event: &PointerEvent,
    ) -> bool {
        (self.handler)(event.clone());
        event.is_consumed()
    }

    fn pointer_input_handler(&self) -> Option<PointerHandler> {
        Some(Rc::clone(&self.handler))
    }
}

/// Element that creates and updates [`DragAndDropSourceNode`]s.
#[derive(Clone, Debug)]
pub struct DragAndDropSourceElement {
    source: DragAndDropSource,
}

impl PartialEq for DragAndDropSourceElement {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Hash for DragAndDropSourceElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "dragAndDropSource".hash(state);
    }
}

impl ModifierNodeElement for DragAndDropSourceElement {
    type Node = DragAndDropSourceNode;

    fn create(&self) -> Self::Node {
        let source = Rc::new(RefCell::new(self.source.clone()));
        let gesture = Rc::new(RefCell::new(SourceGesture::default()));
        DragAndDropSourceNode {
            handler: source_handler(Rc::clone(&source), gesture),
            source,
            state: NodeState::new(),
        }
    }

    fn update(&self, node: &mut Self::Node) {
        *node.source.borrow_mut() = self.source.clone();
    }

    fn always_update(&self) -> bool {
        true
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::POINTER_INPUT
    }

    fn inspector_name(&self) -> &'static str {
        "dragAndDropSource"
    }
}

/// Node that registers its layout node as a drop target while attached.
/// It is a pointer target so the shell's hit test finds it under a
/// transfer; it handles no pointer events itself.
pub struct DragAndDropTargetNode {
    target: DragAndDropTarget,
    node_id: Cell<Option<NodeId>>,
    owner: Cell<Option<AppContextId>>,
    handler: PointerHandler,
    state: NodeState,
}

impl fmt::Debug for DragAndDropTargetNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DragAndDropTargetNode")
            .field("node_id", &self.node_id.get())
            .finish()
    }
}

impl DragAndDropTargetNode {
    fn register(&self) {
        let Some(node) = self.node_id.get() else {
            return;
        };
        let Some(context) = current_app_context() else {
            return;
        };
        self.owner.set(Some(context.id()));
        context
            .drag_and_drop()
            .register_target(node, self.target.clone());
    }

    fn unregister(&self) {
        let (Some(node), Some(owner)) = (self.node_id.get(), self.owner.take()) else {
            return;
        };
        with_app_context_by_id(owner, |context| {
            context.drag_and_drop().unregister_target(node)
        });
    }
}

impl DelegatableNode for DragAndDropTargetNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for DragAndDropTargetNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        self.node_id.set(context.node_id());
        self.register();
        context.invalidate(InvalidationKind::PointerInput);
    }

    fn on_detach(&mut self) {
        self.unregister();
    }

    cranpose_foundation::impl_modifier_node!(pointer_input);
}

impl PointerInputNode for DragAndDropTargetNode {
    fn pointer_input_handler(&self) -> Option<PointerHandler> {
        Some(Rc::clone(&self.handler))
    }
}

/// Element that creates and updates [`DragAndDropTargetNode`]s.
#[derive(Clone, Debug)]
pub struct DragAndDropTargetElement {
    target: DragAndDropTarget,
}

impl PartialEq for DragAndDropTargetElement {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Hash for DragAndDropTargetElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "dragAndDropTarget".hash(state);
    }
}

impl ModifierNodeElement for DragAndDropTargetElement {
    type Node = DragAndDropTargetNode;

    fn create(&self) -> Self::Node {
        DragAndDropTargetNode {
            target: self.target.clone(),
            node_id: Cell::new(None),
            owner: Cell::new(None),
            handler: Rc::new(|_event: PointerEvent| {}),
            state: NodeState::new(),
        }
    }

    fn update(&self, node: &mut Self::Node) {
        node.target = self.target.clone();
        node.register();
    }

    fn always_update(&self) -> bool {
        true
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::POINTER_INPUT
    }

    fn inspector_name(&self) -> &'static str {
        "dragAndDropTarget"
    }
}

impl Modifier {
    /// Lets a press on the node that moves past the drag threshold carry
    /// `source`'s payload to a [`drag_and_drop_target`](Self::drag_and_drop_target)
    /// anywhere in the app, in this window or another.
    pub fn drag_and_drop_source(self, source: DragAndDropSource) -> Self {
        self.then(Self::with_element(DragAndDropSourceElement { source }))
    }

    /// Lets the node receive a transfer started by a
    /// [`drag_and_drop_source`](Self::drag_and_drop_source), hearing
    /// `target`'s enter, move, exit and drop.
    pub fn drag_and_drop_target(self, target: DragAndDropTarget) -> Self {
        self.then(Self::with_element(DragAndDropTargetElement { target }))
    }
}

#[cfg(test)]
#[path = "tests/drag_and_drop_tests.rs"]
mod tests;
