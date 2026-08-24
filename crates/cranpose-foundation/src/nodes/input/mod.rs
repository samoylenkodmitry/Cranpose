pub mod dispatcher;
pub mod gestures;
pub mod rotary;
pub mod types;

pub use rotary::{
    rotary_scroll_pixels_from_detents, RotaryScrollEvent, RotaryStepAccumulator,
    DEFAULT_ROTARY_SCROLL_FACTOR_DP,
};
pub use types::{
    Modifiers, PointerButton, PointerButtons, PointerEvent, PointerEventKind, PointerId,
    PointerPhase, PointerSource,
};

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
