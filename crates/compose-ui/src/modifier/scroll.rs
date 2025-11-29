//! Scroll modifier extensions for Modifier.
//!
//! Provides horizontal_scroll and vertical_scroll modifier extensions that 
//! combine gesture detection with ScrollNode layout.

use crate::scroll_modifier_node::ScrollNodeElement;
use crate::Modifier;
use compose_foundation::scroll::ScrollState;
use compose_foundation::scrollable::{Orientation, ScrollablePointerInputElement};
use std::rc::Rc;

impl Modifier {
    /// Apply horizontal scrolling to this element.
    ///
    /// This modifier makes the element horizontally scrollable when its content
    /// width exceeds the available width. It combines drag gesture detection
    /// with layout measurement/offsetting.
    ///
    /// # Arguments
    /// * `state` - The ScrollState to control scroll position
    ///
    /// # Example
    /// ```ignore
    /// let scroll_state = compose_core::useState(|| ScrollState::new(0));
    /// Row(
    ///     Modifier::empty().horizontal_scroll(scroll_state.get()),
    ///     // ...
    /// )
    /// ```
    pub fn horizontal_scroll(self, state: ScrollState) -> Self {
        eprintln!("[horizontal_scroll] Creating horizontal scroll modifier");
        
        // Add gesture detection (drag to scroll)
        eprintln!("[horizontal_scroll] Creating ScrollablePointerInputElement");
        let gesture_element = ScrollablePointerInputElement::new(
            Rc::new(state.clone()) as Rc<dyn compose_foundation::scrollable::ScrollableState>,
            Orientation::Horizontal,
            true, // enabled
        );
        let gesture_modifier = Modifier::from_parts(vec![compose_foundation::modifier_element(gesture_element)]);
        
        // Add layout node (infinite constraints + offset)
        eprintln!("[horizontal_scroll] Creating ScrollNodeElement");
        let layout_element = ScrollNodeElement::new(state, false, false);
        let layout_modifier = Modifier::from_parts(vec![compose_foundation::modifier_element(layout_element)]);
        
        eprintln!("[horizontal_scroll] Combining modifiers: self + gesture + layout");
        // Combine: self + gesture + layout
        self.then(gesture_modifier).then(layout_modifier)
    }

    /// Apply vertical scrolling to this element.
    ///
    /// This modifier makes the element vertically scrollable when its content
    /// height exceeds the available height. It combines drag gesture detection
    /// with layout measurement/offsetting.
    ///
    /// # Arguments  
    /// * `state` - The ScrollState to control scroll position
    pub fn vertical_scroll(self, state: ScrollState) -> Self {
        // Add gesture detection (drag to scroll)
        let gesture_element = ScrollablePointerInputElement::new(
            Rc::new(state.clone()) as Rc<dyn compose_foundation::scrollable::ScrollableState>,
            Orientation::Vertical,
            true, // enabled
        );
        let gesture_modifier = Modifier::from_parts(vec![compose_foundation::modifier_element(gesture_element)]);
        
        // Add layout node (infinite constraints + offset)
        let layout_element = ScrollNodeElement::new(state, true, false);
        let layout_modifier = Modifier::from_parts(vec![compose_foundation::modifier_element(layout_element)]);
        
        // Combine: self + gesture + layout
        self.then(gesture_modifier).then(layout_modifier)
    }
}
