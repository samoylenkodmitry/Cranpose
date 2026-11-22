use std::{
    cell::{Cell, RefCell},
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_core::NodeId;
use cranpose_foundation::{
    DRAG_THRESHOLD, DelegatableNode, InvalidationKind, ModifierNode, ModifierNodeContext,
    ModifierNodeElement, NodeCapabilities, NodeState, PointerEvent, PointerEventKind, PointerId,
    PointerInputNode, PointerSource,
};
use cranpose_ui_graphics::Color;
use smallvec::SmallVec;

use crate::{Indication, IndicationState, modifier::Point};

/// Where a clickable's press stands.
#[derive(Clone, Copy, Debug)]
enum ClickPress {
    Idle,
    /// A pointer went down here, and the id of that pointer.
    Pressed(Point, PointerId),
    /// The press was cancelled, by a long press that took it among others:
    /// its release does not click.
    Cancelled,
}

// What a clickable shares between its pointer handler and the draw of its
// indication.
#[derive(Debug)]
pub(crate) struct ClickLook {
    press: Cell<ClickPress>,
    // The pointers that rest over the control. Two hands of a headset can
    // rest on one control.
    hovers: RefCell<SmallVec<[PointerId; 2]>>,
    indication: Cell<Indication>,
    // The layout node whose draw shows the indication, once the node is attached
    node: Cell<Option<NodeId>>,
}

impl ClickLook {
    fn new(indication: Indication) -> Self {
        Self {
            press: Cell::new(ClickPress::Idle),
            hovers: RefCell::new(SmallVec::new()),
            indication: Cell::new(indication),
            node: Cell::new(None),
        }
    }

    fn state(&self) -> IndicationState {
        if matches!(self.press.get(), ClickPress::Pressed(..)) {
            IndicationState::Pressed
        } else if self.hovers.borrow().is_empty() {
            IndicationState::Idle
        } else {
            IndicationState::Hovered
        }
    }

    // The color that the indication lays over the control now, or `None`
    // while it shows nothing.
    pub(crate) fn color(&self) -> Option<Color> {
        self.indication.get().color(self.state())
    }

    // A finger rests on nothing: a touch's enter comes with its press and no
    // exit follows its lift.
    fn hover(&self, event: &PointerEvent) {
        match event.kind {
            PointerEventKind::Enter if event.source != PointerSource::Touch => {
                let mut hovers = self.hovers.borrow_mut();
                if !hovers.contains(&event.id) {
                    hovers.push(event.id);
                }
            }
            PointerEventKind::Exit => self.hovers.borrow_mut().retain(|id| *id != event.id),
            _ => {}
        }
    }

    // Draws the control again when its indication changed and shows something.
    fn redraw_after(&self, before: IndicationState) {
        if self.state() == before || self.indication.get() == Indication::NONE {
            return;
        }
        redraw(self.node.get());
    }

    // The node leaves: nothing of its interaction stays for a later attach.
    fn end(&self) {
        self.press.set(ClickPress::Idle);
        self.hovers.borrow_mut().clear();
    }
}

// Draws `node` again, in the app context that is active: outside one no frame draws.
fn redraw(node: Option<NodeId>) {
    if let Some(node) = node
        && crate::render_state::current_app_context().is_some()
    {
        crate::render_state::schedule_draw_repass(node);
    }
}

/// Node that reports a click when a pointer goes down and up without moving farther than the drag
/// threshold: the first pointer that goes down holds the press, and the others pass by until it
/// lifts, so a second finger does not click, and each controller of a headset clicks with its own.
/// A press that travels past the threshold ends at once: the control stops showing it pressed, and
/// its release does not click. Callbacks get positions in the node's local coordinates, in logical
/// pixels.
///
/// The node draws its [`Indication`] over the control while a pointer rests on it or presses it.
pub struct ClickableNode {
    on_press: Option<Rc<dyn Fn(Point)>>,
    on_click: Rc<dyn Fn(Point)>,
    state: NodeState,
    look: Rc<ClickLook>,
    cached_handler: Rc<dyn Fn(PointerEvent)>,
}

impl std::fmt::Debug for ClickableNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClickableNode").finish()
    }
}

impl ClickableNode {
    /// Creates a node that calls `on_click` with the position of the click.
    pub fn new(on_click: impl Fn(Point) + 'static) -> Self {
        Self::with_handler(Rc::new(on_click))
    }

    /// Creates a node that calls the shared callback `on_click` with the position of the click.
    pub fn with_handler(on_click: Rc<dyn Fn(Point)>) -> Self {
        Self::with_handlers(None, on_click)
    }

    /// Creates a node that calls `on_press`, when given, as the pointer goes down, and `on_click`
    /// on a click. Both get the pointer position.
    pub fn with_handlers(on_press: Option<Rc<dyn Fn(Point)>>, on_click: Rc<dyn Fn(Point)>) -> Self {
        Self::with_indication(on_press, on_click, Indication::default())
    }

    fn with_indication(
        on_press: Option<Rc<dyn Fn(Point)>>,
        on_click: Rc<dyn Fn(Point)>,
        indication: Indication,
    ) -> Self {
        let look = Rc::new(ClickLook::new(indication));
        let cached_handler =
            Self::create_handler(on_press.clone(), Rc::clone(&on_click), Rc::clone(&look));
        Self {
            on_press,
            on_click,
            state: NodeState::new(),
            look,
            cached_handler,
        }
    }

    fn create_handler(
        on_press: Option<Rc<dyn Fn(Point)>>,
        on_click: Rc<dyn Fn(Point)>,
        look: Rc<ClickLook>,
    ) -> Rc<dyn Fn(PointerEvent)> {
        Rc::new(move |event: PointerEvent| {
            let before = look.state();
            look.hover(&event);
            handle_press(&look.press, on_press.as_deref(), &*on_click, &event);
            look.redraw_after(before);
        })
    }

    /// Returns the click callback.
    pub fn handler(&self) -> Rc<dyn Fn(Point)> {
        Rc::clone(&self.on_click)
    }

    /// The state that the node's indication shows.
    pub fn indication_state(&self) -> IndicationState {
        self.look.state()
    }

    pub(crate) fn look(&self) -> Rc<ClickLook> {
        Rc::clone(&self.look)
    }
}

fn handle_press(
    press: &Cell<ClickPress>,
    on_press: Option<&dyn Fn(Point)>,
    on_click: &dyn Fn(Point),
    event: &PointerEvent,
) {
    if matches!(press.get(), ClickPress::Pressed(_, holder) if holder != event.id) {
        return;
    }

    if event.is_consumed() {
        press.set(ClickPress::Idle);
        return;
    }

    match event.kind {
        PointerEventKind::Down => {
            press.set(ClickPress::Pressed(event.travelled_to(), event.id));
            if let Some(on_press) = on_press {
                on_press(event.position);
            }
        }
        PointerEventKind::Move => {
            if let ClickPress::Pressed(origin, _) = press.get()
                && (event.travelled_to() - origin).distance() > DRAG_THRESHOLD
            {
                press.set(ClickPress::Cancelled);
            }
        }
        PointerEventKind::Up => {
            let should_click = match press.replace(ClickPress::Idle) {
                ClickPress::Pressed(origin, _) => {
                    (event.travelled_to() - origin).distance() <= DRAG_THRESHOLD
                }
                // A release with no press before it clicks for the primary pointer only: another
                // finger's release after the first one clicked is not a click of its own.
                ClickPress::Idle => event.id.is_primary(),
                ClickPress::Cancelled => false,
            };

            if should_click {
                on_click(Point {
                    x: event.position.x,
                    y: event.position.y,
                });
                event.consume();
            }
        }
        PointerEventKind::Cancel => {
            press.set(ClickPress::Cancelled);
        }
        _ => {}
    }
}

impl DelegatableNode for ClickableNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for ClickableNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        self.look.node.set(context.node_id());
        context.invalidate(InvalidationKind::PointerInput);
    }

    fn on_detach(&mut self) {
        self.look.end();
        self.look.node.set(None);
    }

    fn as_pointer_input_node(&self) -> Option<&dyn PointerInputNode> {
        Some(self)
    }

    fn as_pointer_input_node_mut(&mut self) -> Option<&mut dyn PointerInputNode> {
        Some(self)
    }
}

impl PointerInputNode for ClickableNode {
    fn on_pointer_event(
        &mut self,
        _context: &mut dyn ModifierNodeContext,
        event: &PointerEvent,
    ) -> bool {
        (self.cached_handler)(event.clone());
        event.is_consumed()
    }

    fn hit_test(&self, _x: f32, _y: f32) -> bool {
        true
    }

    fn pointer_input_handler(&self) -> Option<Rc<dyn Fn(PointerEvent)>> {
        Some(Rc::clone(&self.cached_handler))
    }
}

/// Element that creates and updates clickable nodes.
#[derive(Clone)]
pub struct ClickableElement {
    on_press: Option<Rc<dyn Fn(Point)>>,
    on_click: Rc<dyn Fn(Point)>,
    indication: Indication,
}

impl ClickableElement {
    /// Creates an element that calls `on_click` with the position of the click.
    pub fn new(on_click: impl Fn(Point) + 'static) -> Self {
        Self::with_handler(Rc::new(on_click))
    }

    /// Creates an element that calls the shared callback `on_click` with the position of the click.
    pub fn with_handler(on_click: Rc<dyn Fn(Point)>) -> Self {
        Self {
            on_press: None,
            on_click,
            indication: Indication::default(),
        }
    }

    /// Creates an element that calls `on_press` as the pointer goes down and `on_click` on a click.
    /// Both get the pointer position.
    pub fn with_handlers(on_press: Rc<dyn Fn(Point)>, on_click: Rc<dyn Fn(Point)>) -> Self {
        Self {
            on_press: Some(on_press),
            ..Self::with_handler(on_click)
        }
    }

    /// The element with `indication` in place of the default one.
    #[must_use]
    pub fn indication(self, indication: Indication) -> Self {
        Self { indication, ..self }
    }
}

impl std::fmt::Debug for ClickableElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClickableElement").finish()
    }
}

impl PartialEq for ClickableElement {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for ClickableElement {}

impl Hash for ClickableElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "clickable".hash(state);
    }
}

impl ModifierNodeElement for ClickableElement {
    type Node = ClickableNode;

    fn create(&self) -> Self::Node {
        ClickableNode::with_indication(
            self.on_press.clone(),
            Rc::clone(&self.on_click),
            self.indication,
        )
    }

    fn update(&self, node: &mut Self::Node) {
        node.on_press.clone_from(&self.on_press);
        node.on_click.clone_from(&self.on_click);
        if node.look.indication.replace(self.indication) != self.indication {
            redraw(node.look.node.get());
        }
        node.cached_handler = ClickableNode::create_handler(
            node.on_press.clone(),
            Rc::clone(&node.on_click),
            Rc::clone(&node.look),
        );
    }

    // A clickable that shows no indication draws nothing, so its chain keeps
    // the draws it had without one.
    fn capabilities(&self) -> NodeCapabilities {
        if self.indication == Indication::NONE {
            NodeCapabilities::POINTER_INPUT
        } else {
            NodeCapabilities::POINTER_INPUT | NodeCapabilities::DRAW
        }
    }

    fn always_update(&self) -> bool {
        true
    }
}
