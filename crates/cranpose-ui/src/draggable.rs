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

#![allow(non_snake_case)]

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{remember, MutableState, State};

/// What a [`DraggableState`] hands each drag delta to.
pub type DragDeltaHandler = Rc<dyn Fn(f32)>;

struct DraggableStateInner {
    on_delta: RefCell<DragDeltaHandler>,
    dragging: MutableState<bool>,
    /// Everything this state has been dragged by, in logical pixels. The
    /// gesture pipeline reads it to reason about direction; a caller that keeps
    /// its own position never has to.
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
    /// Two handles are equal when they drive the same control — identity, not
    /// value, so a composable that takes one skips on recomposition.
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
        if !delta.is_finite() || delta == 0.0 {
            return;
        }
        self.inner.offset.set(self.inner.offset.get() + delta);
        let handler = Rc::clone(&self.inner.on_delta.borrow());
        handler(delta);
    }

    /// A stable identity for this state, used to key the gesture that drives
    /// it so a recomposition reuses the running gesture instead of restarting
    /// it mid-drag.
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
pub fn rememberDraggableState(on_delta: impl Fn(f32) + 'static) -> DraggableState {
    let state = remember(|| DraggableState::new(|_| {})).with(|state| state.clone());
    state.update_handler(on_delta);
    state
}

#[cfg(test)]
#[path = "tests/draggable_tests.rs"]
mod draggable_tests;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use cranpose_core::{DefaultScheduler, Runtime};

    use super::*;

    fn with_runtime<T>(body: impl FnOnce() -> T) -> T {
        let _runtime = Runtime::new(Arc::new(DefaultScheduler));
        body()
    }

    /// A remembered handle has to survive recomposition -- a control given a
    /// fresh state each pass would forget it was being dragged -- while the
    /// closure it delivers to has to be the latest pass's, or a delta lands on
    /// a value the first pass captured.
    #[test]
    fn a_remembered_drag_state_survives_recomposition_and_takes_the_new_handler() {
        use cranpose_core::{location_key, Composition, MemoryApplier};

        let mut composition = Composition::new(MemoryApplier::new());
        let seen = Rc::new(RefCell::new(Vec::new()));
        let handles: Rc<RefCell<Vec<DraggableState>>> = Rc::new(RefCell::new(Vec::new()));
        let pass = Rc::new(Cell::new(0usize));

        let key = location_key(file!(), line!(), column!());
        for _ in 0..2 {
            let seen = Rc::clone(&seen);
            let handles = Rc::clone(&handles);
            let pass = Rc::clone(&pass);
            let mut render = move || {
                let tag = pass.get();
                pass.set(tag + 1);
                let recorder = Rc::clone(&seen);
                let state = rememberDraggableState(move |delta| {
                    recorder.borrow_mut().push((tag, delta));
                });
                handles.borrow_mut().push(state);
            };
            composition.render(key, &mut render).expect("render");
        }

        let handles = handles.borrow();
        assert_eq!(handles.len(), 2);
        assert!(
            handles[0] == handles[1],
            "a remembered state must survive the slot"
        );

        handles[1].drag_by(3.0);
        assert_eq!(
            *seen.borrow(),
            vec![(1, 3.0)],
            "the delta must reach the handler the latest pass supplied"
        );
    }

    #[test]
    fn a_drag_delta_reaches_the_current_handler() {
        with_runtime(|| {
            let seen = Rc::new(RefCell::new(Vec::new()));
            let recorder = Rc::clone(&seen);
            let state = DraggableState::new(move |delta| recorder.borrow_mut().push(delta));
            state.drag_by(4.0);
            state.drag_by(-1.5);
            assert_eq!(seen.borrow().as_slice(), [4.0, -1.5]);
            assert_eq!(state.offset(), 2.5);
        });
    }

    #[test]
    fn replacing_the_handler_redirects_later_deltas() {
        with_runtime(|| {
            let first = Rc::new(Cell::new(0.0));
            let second = Rc::new(Cell::new(0.0));
            let recorder = Rc::clone(&first);
            let state = DraggableState::new(move |delta| recorder.set(recorder.get() + delta));
            state.drag_by(2.0);
            let recorder = Rc::clone(&second);
            state.update_handler(move |delta| recorder.set(recorder.get() + delta));
            state.drag_by(3.0);
            assert_eq!(first.get(), 2.0);
            assert_eq!(second.get(), 3.0);
        });
    }

    #[test]
    fn a_delta_that_is_not_a_movement_is_not_delivered() {
        with_runtime(|| {
            let count = Rc::new(Cell::new(0u32));
            let recorder = Rc::clone(&count);
            let state = DraggableState::new(move |_| recorder.set(recorder.get() + 1));
            state.drag_by(0.0);
            state.drag_by(f32::NAN);
            assert_eq!(count.get(), 0);
            assert_eq!(state.offset(), 0.0);
        });
    }

    #[test]
    fn dragging_is_observable_and_clones_share_it() {
        with_runtime(|| {
            let state = DraggableState::new(|_| {});
            let handle = state.clone();
            assert!(!handle.is_dragging());
            state.set_dragging(true);
            assert!(handle.is_dragging());
            state.set_dragging(false);
            assert!(!handle.is_dragging());
            assert!(state == handle);
        });
    }
}
