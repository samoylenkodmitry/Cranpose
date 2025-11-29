//! Low-level scrollable state trait and gesture detection.
//!
//! This module provides the ScrollableState trait that defines the interface
//! for consuming scroll deltas, similar to Jetpack Compose's ScrollableState.

use crate::modifier::{
    DelegatableNode, ModifierNode, ModifierNodeContext, ModifierNodeElement, NodeCapabilities,
    NodeState, PointerInputNode,
};
use crate::nodes::input::types::{PointerEvent, PointerEventKind};
use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

/// Orientation for scrolling - horizontal or vertical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Orientation {
    Horizontal,
    Vertical,
}

/// Low-level scrollable state interface.
///
/// This trait is implemented by scroll state holders (like ScrollState) to consume
/// scroll deltas from gesture input. It mirrors Jetpack Compose's ScrollableState interface.
pub trait ScrollableState {
    /// Consume a scroll delta and return the amount consumed.
    ///
    /// This is invoked by the scrollable gesture detector when scroll events occur.
    /// Implementers should update their internal state and return the amount of
    /// delta that was actually consumed, which is important for nested scrolling.
    ///
    /// # Arguments
    /// * `delta` - The scroll delta in pixels (positive or negative)
    ///
    /// # Returns
    /// The amount of delta consumed (may be less than requested if at bounds)
    fn consume_scroll_delta(&self, delta: f32) -> f32;
    
    /// Whether this scrollable state is currently scrolling.
    fn is_scroll_in_progress(&self) -> bool;
}

/// Pointer input node that detects drag gestures and converts them to scroll deltas.
///
/// This mirrors Jetpack Compose's DragGestureNode behavior but simplified for initial implementation.
pub struct ScrollablePointerInputNode {
    state: Rc<dyn ScrollableState>,
    orientation: Orientation,
    enabled: bool,
    node_state: NodeState,
    // Track dragging state
    is_dragging: bool,
    last_position: Option<(f32, f32)>,
}

impl ScrollablePointerInputNode {
    pub fn new(state: Rc<dyn ScrollableState>, orientation: Orientation, enabled: bool) -> Self {
        Self {
            state,
            orientation,
            enabled,
            node_state: NodeState::new(),
            is_dragging: false,
            last_position: None,
        }
    }
}

impl DelegatableNode for ScrollablePointerInputNode {
    fn node_state(&self) -> &NodeState {
        &self.node_state
    }
}

impl ModifierNode for ScrollablePointerInputNode {
    fn as_pointer_input_node(&self) -> Option<&dyn PointerInputNode> {
        Some(self)
    }

    fn as_pointer_input_node_mut(&mut self) -> Option<&mut dyn PointerInputNode> {
        Some(self)
    }
}

impl PointerInputNode for ScrollablePointerInputNode {
    fn on_pointer_event(
        &mut self,
        _context: &mut dyn ModifierNodeContext,
        event: &PointerEvent,
        pass: crate::nodes::input::types::PointerEventPass,
    ) -> bool {
        use crate::nodes::input::types::PointerEventPass;

        if !self.enabled {
            return false;
        }

        // Only handle Main pass for now
        if pass != PointerEventPass::Main {
            return false;
        }

        match event.kind() {
            PointerEventKind::Down => {
                if event.is_consumed() {
                    return false;
                }
                // Start tracking
                self.is_dragging = true;
                self.last_position = Some((event.position().x, event.position().y));
                event.consume();
                true
            }
            PointerEventKind::Move => {
                if self.is_dragging {
                    if event.is_consumed() {
                        // If someone else consumed the move, we should probably stop dragging or wait?
                        // For now, let's respect consumption.
                        return false;
                    }

                    if let Some((last_x, last_y)) = self.last_position {
                        // Calculate delta based on orientation
                        let raw_delta = match self.orientation {
                            Orientation::Horizontal => event.position().x - last_x,
                            Orientation::Vertical => event.position().y - last_y,
                        };

                        // INVERT: dragging content LEFT increases scroll (reveals right content)
                        let delta = -raw_delta;

                        // Consume the scroll delta
                        self.state.consume_scroll_delta(delta);

                        // Update position
                        self.last_position = Some((event.position().x, event.position().y));
                        
                        event.consume();
                    }
                    true
                } else {
                    false
                }
            }
            PointerEventKind::Up | PointerEventKind::Cancel => {
                // End drag
                self.is_dragging = false;
                self.last_position = None;
                true
            }
        }
    }

    fn hit_test(&self, _x: f32, _y: f32) -> bool {
        self.enabled
    }

    fn pointer_input_handler(&self) -> Option<Rc<dyn Fn(PointerEvent)>> {
        None
    }
}

/// Element for creating ScrollablePointerInputNode instances.
pub struct ScrollablePointerInputElement {
    state: Rc<dyn ScrollableState>,
    orientation: Orientation,
    enabled: bool,
}

impl ScrollablePointerInputElement {
    pub fn new(state: Rc<dyn ScrollableState>, orientation: Orientation, enabled: bool) -> Self {
        Self {
            state,
            orientation,
            enabled,
        }
    }
}

impl std::fmt::Debug for ScrollablePointerInputElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollablePointerInputElement")
            .field("orientation", &self.orientation)
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl PartialEq for ScrollablePointerInputElement {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(
            Rc::as_ptr(&self.state) as *const (),
            Rc::as_ptr(&other.state) as *const (),
        ) && self.orientation == other.orientation
            && self.enabled == other.enabled
    }
}

impl Hash for ScrollablePointerInputElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (Rc::as_ptr(&self.state) as *const () as usize).hash(state);
        self.orientation.hash(state);
        self.enabled.hash(state);
    }
}

impl ModifierNodeElement for ScrollablePointerInputElement {
    type Node = ScrollablePointerInputNode;

    fn create(&self) -> Self::Node {
        eprintln!("[ScrollablePointerInputElement] create() called - creating ScrollablePointerInputNode");
        ScrollablePointerInputNode::new(self.state.clone(), self.orientation, self.enabled)
    }

    fn update(&self, node: &mut Self::Node) {
        eprintln!("[ScrollablePointerInputElement] update() called");
        node.state = self.state.clone();
        node.orientation = self.orientation;
        node.enabled = self.enabled;
    }

    fn capabilities(&self) -> NodeCapabilities {
        eprintln!("[ScrollablePointerInputElement] capabilities() called, returning POINTER_INPUT");
        NodeCapabilities::POINTER_INPUT
    }
}
