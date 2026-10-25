use std::{
    cell::Cell,
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_foundation::{
    DRAG_THRESHOLD, DelegatableNode, InvalidationKind, ModifierNode, ModifierNodeContext,
    ModifierNodeElement, NodeCapabilities, NodeState, PointerEvent, PointerEventKind,
    PointerInputNode,
};

use crate::modifier::Point;

/// Where a clickable's press stands.
#[derive(Clone, Copy, Debug)]
enum ClickPress {
    Idle,
    /// A pointer went down here, and the id of that pointer.
    Pressed(Point, cranpose_foundation::PointerId),
    /// The press was cancelled, by a long press that took it among others:
    /// its release does not click.
    Cancelled,
}

/// Node that reports a click when a pointer goes down and up without moving farther than the drag
/// threshold: the first pointer that goes down holds the press, and the others pass by until it
/// lifts, so a second finger does not click, and each controller of a headset clicks with its own.
/// Callbacks get positions in the node's local coordinates, in logical pixels.
pub struct ClickableNode {
    on_press: Option<Rc<dyn Fn(Point)>>,
    on_click: Rc<dyn Fn(Point)>,
    state: NodeState,
    press_position: Rc<Cell<ClickPress>>,
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
        let press_position = Rc::new(Cell::new(ClickPress::Idle));
        let cached_handler = Self::create_handler(
            on_press.clone(),
            Rc::clone(&on_click),
            Rc::clone(&press_position),
        );
        Self {
            on_press,
            on_click,
            state: NodeState::new(),
            press_position,
            cached_handler,
        }
    }

    fn create_handler(
        on_press: Option<Rc<dyn Fn(Point)>>,
        on_click: Rc<dyn Fn(Point)>,
        press_position: Rc<Cell<ClickPress>>,
    ) -> Rc<dyn Fn(PointerEvent)> {
        Rc::new(move |event: PointerEvent| {
            if matches!(press_position.get(), ClickPress::Pressed(_, holder) if holder != event.id)
            {
                return;
            }

            if event.is_consumed() {
                press_position.set(ClickPress::Idle);
                return;
            }

            match event.kind {
                PointerEventKind::Down => {
                    press_position.set(ClickPress::Pressed(event.travelled_to(), event.id));
                    if let Some(on_press) = on_press.as_ref() {
                        on_press(event.position);
                    }
                }
                PointerEventKind::Move => {}
                PointerEventKind::Up => {
                    let should_click = match press_position.replace(ClickPress::Idle) {
                        ClickPress::Pressed(press_pos, _) => {
                            let travelled_to = event.travelled_to();
                            let dx = travelled_to.x - press_pos.x;
                            let dy = travelled_to.y - press_pos.y;
                            let distance = (dx * dx + dy * dy).sqrt();
                            distance <= DRAG_THRESHOLD
                        }
                        // A release with no press before it clicks for the
                        // primary pointer only: another finger's release after
                        // the first one clicked is not a click of its own.
                        ClickPress::Idle => event.id == 0,
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
                    press_position.set(ClickPress::Cancelled);
                }
                _ => {}
            }
        })
    }

    /// Returns the click callback.
    pub fn handler(&self) -> Rc<dyn Fn(Point)> {
        Rc::clone(&self.on_click)
    }
}

impl DelegatableNode for ClickableNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for ClickableNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(InvalidationKind::PointerInput);
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
}

impl ClickableElement {
    /// Creates an element that calls `on_click` with the position of the click.
    pub fn new(on_click: impl Fn(Point) + 'static) -> Self {
        Self {
            on_press: None,
            on_click: Rc::new(on_click),
        }
    }

    /// Creates an element that calls the shared callback `on_click` with the position of the click.
    pub fn with_handler(on_click: Rc<dyn Fn(Point)>) -> Self {
        Self {
            on_press: None,
            on_click,
        }
    }

    /// Creates an element that calls `on_press` as the pointer goes down and `on_click` on a click.
    /// Both get the pointer position.
    pub fn with_handlers(on_press: Rc<dyn Fn(Point)>, on_click: Rc<dyn Fn(Point)>) -> Self {
        Self {
            on_press: Some(on_press),
            on_click,
        }
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
        ClickableNode::with_handlers(self.on_press.clone(), Rc::clone(&self.on_click))
    }

    fn update(&self, node: &mut Self::Node) {
        node.on_press.clone_from(&self.on_press);
        node.on_click.clone_from(&self.on_click);
        node.cached_handler = ClickableNode::create_handler(
            node.on_press.clone(),
            Rc::clone(&node.on_click),
            Rc::clone(&node.press_position),
        );
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::POINTER_INPUT
    }

    fn always_update(&self) -> bool {
        true
    }
}
