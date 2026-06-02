pub mod dispatcher;
pub mod focus;
pub mod gestures;
pub mod types;

pub use types::{
    PointerButton, PointerButtons, PointerEvent, PointerEventKind, PointerId, PointerPhase,
};

pub mod prelude {
    pub use super::gestures::{
        DragGesture, DragGestureEvent, FlingGesture, FlingGestureEvent, ScrollGesture,
        ScrollGestureEvent, TapGesture, TapGestureEvent,
    };
    pub use super::types::{
        PointerButton, PointerButtons, PointerEvent, PointerEventKind, PointerId, PointerPhase,
    };
}
