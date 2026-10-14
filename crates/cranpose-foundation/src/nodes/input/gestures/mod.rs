/// Recognizes a drag by one pointer.
pub mod drag;
/// Measures pointer velocity and reports a fling when the pointer lifts while moving.
pub mod fling;
/// Recognizes wheel and trackpad scrolling.
pub mod scroll;
/// Recognizes a tap by one pointer.
pub mod tap;
pub mod transform;

pub use drag::{DragGesture, DragGestureEvent};
pub use fling::{FlingGesture, FlingGestureEvent};
pub use scroll::{ScrollGesture, ScrollGestureEvent};
pub use tap::{TapGesture, TapGestureEvent};
pub use transform::{TransformGesture, TransformGestureEvent};
