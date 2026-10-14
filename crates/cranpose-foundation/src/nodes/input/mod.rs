pub mod dispatcher;
/// Recognizers that turn a stream of pointer events into gestures such as taps and drags.
pub mod gestures;
pub mod rotary;
/// The types of pointer input: events, pointers, buttons and keyboard modifiers.
pub mod types;

pub use rotary::{
    DEFAULT_ROTARY_SCROLL_FACTOR_DP, RotaryScrollEvent, RotaryStepAccumulator,
    rotary_scroll_pixels_from_detents,
};
pub use types::{
    Modifiers, PointerButton, PointerButtons, PointerEvent, PointerEventKind, PointerId,
    PointerPhase, PointerSource,
};

/// The input types and gesture recognizers that modifier nodes use most.
pub mod prelude {
    pub use super::{
        gestures::{
            DragGesture, DragGestureEvent, FlingGesture, FlingGestureEvent, ScrollGesture,
            ScrollGestureEvent, TapGesture, TapGestureEvent, TransformGesture,
            TransformGestureEvent,
        },
        rotary::{RotaryScrollEvent, RotaryStepAccumulator},
        types::{
            Modifiers, PointerButton, PointerButtons, PointerEvent, PointerEventKind, PointerId,
            PointerPhase, PointerSource,
        },
    };
}
