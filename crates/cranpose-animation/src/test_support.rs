//! Shared composition-driving harness for animation tests.
//!
//! Every `animate*AsState` specialization (Gap 1) and every `Transition`
//! child accessor (Gap 2) is exercised the same way: render once, change
//! what the call site asks for, then step a frame clock and observe the
//! committed value. Factoring that loop out here keeps each type's test
//! module down to the handful of lines that are actually specific to it.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{Composition, Key, MemoryApplier, State, location_key};

use crate::animation::SpringScalar;

const FRAME_NANOS: u64 = 16_666_667;

pub(crate) struct AnimationHarness<T: Clone + 'static> {
    composition: Composition<MemoryApplier>,
    root_key: Key,
    state_slot: Rc<RefCell<Option<State<T>>>>,
    current_target: Rc<RefCell<T>>,
    pass: Box<dyn FnMut()>,
    frame_time_nanos: u64,
}

impl<T: Clone + 'static> AnimationHarness<T> {
    pub(crate) fn new<R>(initial_target: T, render_at: R) -> Self
    where
        R: FnMut(T) -> State<T> + 'static,
    {
        let composition = Composition::new(MemoryApplier::new());
        let root_key = location_key(file!(), line!(), column!());
        let state_slot: Rc<RefCell<Option<State<T>>>> = Rc::new(RefCell::new(None));
        let current_target = Rc::new(RefCell::new(initial_target));

        let pass: Box<dyn FnMut()> = {
            let state_slot = Rc::clone(&state_slot);
            let current_target = Rc::clone(&current_target);
            let mut render_at = render_at;
            Box::new(move || {
                let target = current_target.borrow().clone();
                let state = render_at(target);
                state_slot.borrow_mut().replace(state);
            })
        };

        let mut harness = Self {
            composition,
            root_key,
            state_slot,
            current_target,
            pass,
            frame_time_nanos: 0,
        };
        harness.render();
        harness
    }

    fn render(&mut self) {
        self.composition
            .render(self.root_key, &mut self.pass)
            .expect("render succeeds");
    }

    pub(crate) fn value(&self) -> T {
        self.state_slot
            .borrow()
            .as_ref()
            .expect("state available after render")
            .get()
    }

    /// Changes what the call site passes as its target and re-renders --
    /// this is a target change mid-composition, not a fresh animation.
    pub(crate) fn retarget(&mut self, target: T) -> T {
        *self.current_target.borrow_mut() = target;
        self.render();
        self.value()
    }

    /// Advances the frame clock by one vsync and drains it, returning the
    /// value the animation committed for that frame.
    pub(crate) fn advance_frame(&mut self) -> T {
        self.frame_time_nanos += FRAME_NANOS;
        let runtime = self.composition.runtime_handle();
        runtime.drain_frame_callbacks(self.frame_time_nanos);
        let _ = self.composition.process_invalid_scopes();
        self.value()
    }

    pub(crate) fn advance_frames(&mut self, count: usize) -> T {
        let mut last = self.value();
        for _ in 0..count {
            last = self.advance_frame();
        }
        last
    }
}

/// Confirms the [`SpringScalar`] contract for a type: linear interpolation
/// matches a per-dimension average, and decomposing then rebuilding a value
/// round-trips exactly. Every new `animate*AsState` specialization in this
/// crate is exercised through this one check.
pub(crate) fn assert_spring_scalar_round_trips<T>(low: T, high: T)
where
    T: SpringScalar + PartialEq + std::fmt::Debug,
{
    use crate::animation::SPRING_MAX_DIMENSIONS;

    let mid = low.lerp(&high, 0.5);
    for index in 0..T::DIMENSIONS {
        let expected = (low.dimension(index) + high.dimension(index)) / 2.0;
        assert!(
            (mid.dimension(index) - expected).abs() < 1e-4,
            "dimension {index} of the midpoint should average its endpoints, \
             got {} expected {expected}",
            mid.dimension(index)
        );
    }

    let mut dimensions = [0.0f32; SPRING_MAX_DIMENSIONS];
    for (index, slot) in dimensions.iter_mut().enumerate().take(T::DIMENSIONS) {
        *slot = low.dimension(index);
    }
    assert_eq!(
        T::from_dimensions(dimensions),
        low,
        "decomposing then rebuilding a value must round-trip"
    );
}

/// Drives a call site from `low` to `high` and back to a fresh render,
/// checking the shape every `animate*AsState` specialization must share:
/// the first frame holds the old value, an intermediate value is observed
/// while animating, and it settles exactly at the target.
pub(crate) fn assert_interpolates_to_target<T, R>(low: T, high: T, render_at: R)
where
    T: SpringScalar + PartialEq + std::fmt::Debug + 'static,
    R: FnMut(T) -> State<T> + 'static,
{
    let mut harness = AnimationHarness::new(low.clone(), render_at);
    assert_eq!(harness.value(), low);

    harness.retarget(high.clone());
    assert_eq!(
        harness.value(),
        low,
        "should not jump before a frame elapses"
    );

    let mut saw_intermediate = false;
    let mut last = low.clone();
    for _ in 0..40 {
        last = harness.advance_frame();
        if last != low && last != high {
            saw_intermediate = true;
        }
    }
    assert!(
        saw_intermediate,
        "should observe an intermediate value, last was {last:?}"
    );
    assert_eq!(last, high, "should settle exactly at the target");
}

/// Drives a call site partway to `first_target`, then switches to
/// `second_target` mid-flight, and confirms the value carries over exactly
/// rather than snapping, then goes on to settle at the new target -- the
/// property every retargetable animation in this crate (`Animatable`, the
/// `animate*AsState` family, `Transition` children) is required to have.
pub(crate) fn assert_retargets_mid_flight_without_snapping<T, R>(
    start: T,
    first_target: T,
    settle_frames: usize,
    second_target: T,
    render_at: R,
) where
    T: SpringScalar + PartialEq + std::fmt::Debug + 'static,
    R: FnMut(T) -> State<T> + 'static,
{
    let mut harness = AnimationHarness::new(start.clone(), render_at);
    harness.retarget(first_target.clone());
    let mid_flight = harness.advance_frames(settle_frames);
    assert_ne!(
        mid_flight, start,
        "should have moved off the start before retargeting"
    );
    assert_ne!(
        mid_flight, first_target,
        "should still be mid-flight before retargeting"
    );

    let value_at_retarget = harness.retarget(second_target.clone());
    assert_eq!(
        value_at_retarget, mid_flight,
        "retargeting must not change the value before the next frame elapses \
         (this is the \"not restarting from zero\" contract)"
    );

    let settled = harness.advance_frames(80);
    assert_eq!(
        settled, second_target,
        "should go on to settle at the new target"
    );
}
