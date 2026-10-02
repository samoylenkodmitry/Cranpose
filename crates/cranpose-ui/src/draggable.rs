//! General drag state, for controls that are dragged but do not scroll.
//!
//! A scrollbar thumb, a bottom sheet, a resizable split, a swipe-away card and
//! a knob all want the same thing from the framework: the drag discipline the
//! scroll containers already have — touch slop before a drag starts, axis
//! locking so a mostly-vertical drag does not steal a horizontal gesture,
//! yielding to whoever already consumed the event, and an observable "is this
//! being dragged right now" for the visuals to react to.
//!
//! [`DraggableState`] carries that, and `Modifier::draggable` runs the same
//! gesture pipeline the scroll modifiers run, so a dragged control and a
//! scrolled list respond to a finger identically.
//!
//! ```text
//! let offset = rememberMutableStateOf(|| 0.0_f32);
//! let drag = rememberDraggableState(move |delta| offset.set(offset.get() + delta));
//! Box(Modifier::empty().size_points(64.0, 64.0).draggable(Axis::Horizontal, drag.clone()), …);
//! ```
//!
//! Deltas arrive in the same logical pixels layout uses, positive along the
//! axis (right for `Axis::Horizontal`, down for `Axis::Vertical`).

#![expect(non_snake_case)]

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{MutableState, State, remember};

/// What a [`DraggableState`] hands each drag delta to.
pub type DragDeltaHandler = Rc<dyn Fn(f32)>;

struct DraggableStateInner {
    on_delta: RefCell<Rc<dyn Fn(f32) -> f32>>,
    dragging: MutableState<bool>,
    offset: Cell<f32>,
}

/// Drag position and progress for one control.
///
/// Cloning shares the state, so a scope can hold a handle and the modifier can
/// hold another without either owning the truth.
#[derive(Clone)]
pub struct DraggableState {
    inner: Rc<DraggableStateInner>,
}

impl PartialEq for DraggableState {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl DraggableState {
    /// Creates a drag state delivering deltas to `on_delta`.
    ///
    /// Prefer [`rememberDraggableState`] inside a composition; this is for
    /// callers that own the state themselves.
    pub fn new(on_delta: impl Fn(f32) + 'static) -> Self {
        Self::new_consuming(move |delta| {
            on_delta(delta);
            delta
        })
    }

    fn new_consuming(on_delta: impl Fn(f32) -> f32 + 'static) -> Self {
        let runtime = cranpose_core::current_runtime_handle()
            .expect("DraggableState::new requires an active runtime");
        Self {
            inner: Rc::new(DraggableStateInner {
                on_delta: RefCell::new(Rc::new(on_delta)),
                dragging: MutableState::with_runtime(false, runtime),
                offset: Cell::new(0.0),
            }),
        }
    }

    /// Replaces the delta handler.
    ///
    /// A composition calls this every recomposition so the handler closes over
    /// the current values rather than the ones the first composition captured.
    pub fn update_handler(&self, on_delta: impl Fn(f32) + 'static) {
        self.update_consuming_handler(move |delta| {
            on_delta(delta);
            delta
        });
    }

    fn update_consuming_handler(&self, on_delta: impl Fn(f32) -> f32 + 'static) {
        *self.inner.on_delta.borrow_mut() = Rc::new(on_delta);
    }

    /// Whether a drag is in flight. Reactive: a composable that reads it
    /// recomposes when the drag starts and when it ends, and not per frame in
    /// between.
    pub fn is_dragging(&self) -> bool {
        self.inner.dragging.value()
    }

    /// [`is_dragging`](Self::is_dragging) as a state a scope can hand around.
    pub fn dragging(&self) -> State<bool> {
        self.inner.dragging.as_state()
    }

    /// Everything this state has been dragged by since it was created.
    pub fn offset(&self) -> f32 {
        self.inner.offset.get()
    }

    /// Drags by `delta` as though a finger had moved that far.
    ///
    /// This is how a control is driven from outside a gesture — a keyboard
    /// arrow, a test, an animation — through exactly the path a finger takes.
    pub fn drag_by(&self, delta: f32) {
        self.dispatch_delta(delta);
    }

    fn dispatch_delta(&self, delta: f32) -> f32 {
        if !delta.is_finite() || delta == 0.0 {
            return 0.0;
        }
        let handler = Rc::clone(&self.inner.on_delta.borrow());
        let consumed = handler(delta);
        self.inner.offset.set(self.inner.offset.get() + consumed);
        consumed
    }

    pub(crate) fn identity(&self) -> usize {
        Rc::as_ptr(&self.inner) as usize
    }

    pub(crate) fn set_dragging(&self, dragging: bool) {
        if self.inner.dragging.get_non_reactive() != dragging {
            self.inner.dragging.set(dragging);
        }
    }
}

/// Remembers a [`DraggableState`] for this composition, keeping its delta
/// handler current across recompositions.
#[track_caller]
pub fn rememberDraggableState(on_delta: impl Fn(f32) + 'static) -> DraggableState {
    let state = remember(|| DraggableState::new(|_| {})).with(Clone::clone);
    state.update_handler(on_delta);
    state
}

/// Scroll input for content that supplies its own placement, such as a wheel.
///
/// The handler receives logical pixel deltas, positive down or right, and
/// returns the amount consumed. A partial consumption stops a fling at a
/// boundary. `Modifier::scrollable` supplies drag, wheel and fling gestures
/// without applying a linear layout translation.
#[derive(Clone, PartialEq)]
pub struct ScrollableState {
    drag: DraggableState,
}

impl ScrollableState {
    /// Creates a scroll state with a delta consumption handler.
    pub fn new(on_delta: impl Fn(f32) -> f32 + 'static) -> Self {
        Self {
            drag: DraggableState::new_consuming(on_delta),
        }
    }

    /// Applies a scroll delta and returns the amount consumed by the content.
    pub fn dispatch_raw_delta(&self, delta: f32) -> f32 {
        self.drag.dispatch_delta(delta)
    }

    /// Whether a pointer is currently dragging this content.
    pub fn is_dragging(&self) -> bool {
        self.drag.is_dragging()
    }

    pub(crate) fn identity(&self) -> usize {
        self.drag.identity()
    }

    pub(crate) fn offset(&self) -> f32 {
        self.drag.offset()
    }

    pub(crate) fn set_dragging(&self, dragging: bool) {
        self.drag.set_dragging(dragging);
    }
}

/// Remembers scroll input state and updates its consumption handler.
#[track_caller]
pub fn rememberScrollableState(on_delta: impl Fn(f32) -> f32 + 'static) -> ScrollableState {
    let state = remember(|| ScrollableState::new(|_| 0.0)).with(Clone::clone);
    state.drag.update_consuming_handler(on_delta);
    state
}

#[cfg(test)]
#[path = "tests/draggable_tests.rs"]
mod draggable_tests;

#[cfg(test)]
#[path = "tests/scrollable_tests.rs"]
mod scrollable_tests;

#[cfg(test)]
#[path = "tests/draggable_drag_state_tests.rs"]
mod tests;
