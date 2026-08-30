//! Finite, state-driven, multi-property animation.
//!
//! Mirrors Jetpack Compose's `updateTransition`/`Transition<S>`: a
//! [`Transition`] holds a target state, and each child animation (created
//! with [`Transition::animateValue`] or one of the typed accessors) derives
//! its own target value from that state and animates toward it in lockstep
//! with its siblings. The transition is [`Transition::is_running`] only
//! while at least one child is still mid-flight, so it settles exactly when
//! every child has.
//!
//! Children reuse the same [`Animatable`]/[`SpringScalar`] machinery as
//! [`animateFloatAsState`](crate::animateFloatAsState): each call site
//! remembers its own `Animatable`, so retargeting mid-flight (the target
//! state changing before the previous one finished) continues from the
//! current value instead of snapping back to a start point, exactly like
//! the single-value `animate*AsState` family already does.
//!
//! Note: This module uses camelCase for function/method names to maintain
//! 1:1 API parity with Jetpack Compose.

#![allow(non_snake_case)]

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{DisposableEffectResult, Owned, State, with_current_composer};
use cranpose_ui_graphics::{Color, Dp};

use crate::animation::{Animatable, AnimationType, SpringScalar};

trait TransitionChild {
    fn is_running(&self) -> bool;
}

impl<T: SpringScalar + 'static> TransitionChild for Animatable<T> {
    fn is_running(&self) -> bool {
        Animatable::is_running(self)
    }
}

struct TransitionInner<S> {
    target_state: RefCell<S>,
    children: RefCell<Vec<Rc<dyn TransitionChild>>>,
}

impl<S> TransitionInner<S> {
    fn add_child(&self, child: Rc<dyn TransitionChild>) {
        self.children.borrow_mut().push(child);
    }

    fn remove_child(&self, child: &Rc<dyn TransitionChild>) {
        let mut children = self.children.borrow_mut();
        if let Some(index) = children
            .iter()
            .position(|existing| Rc::ptr_eq(existing, child))
        {
            children.remove(index);
        }
    }

    fn is_running(&self) -> bool {
        self.children
            .borrow()
            .iter()
            .any(|child| child.is_running())
    }
}

/// A finite, multi-property animation driven by a target state `S`.
///
/// Obtained from [`updateTransition`]; see the module docs for the overall
/// model.
pub struct Transition<S: 'static> {
    inner: Rc<TransitionInner<S>>,
}

impl<S> Clone for Transition<S> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<S: Clone + 'static> Transition<S> {
    fn new(target_state: S) -> Self {
        Self {
            inner: Rc::new(TransitionInner {
                target_state: RefCell::new(target_state),
                children: RefCell::new(Vec::new()),
            }),
        }
    }

    fn set_target_state(&self, target_state: S) {
        *self.inner.target_state.borrow_mut() = target_state;
    }

    /// The state this transition is currently animating towards.
    pub fn target_state(&self) -> S {
        self.inner.target_state.borrow().clone()
    }

    /// `true` while any child animation is still mid-flight. A transition
    /// with no children yet, or whose children have all settled, reports
    /// `false` -- it is finished only when every child is.
    pub fn is_running(&self) -> bool {
        self.inner.is_running()
    }

    /// Generic child animation, built on the same [`SpringScalar`]
    /// vector-converter core as [`crate::animateValueAsState`]. `target_value`
    /// is the value this child should hold once the transition settles for
    /// the current [`Transition::target_state`] -- recomputed by the caller
    /// on every call, exactly like the standalone `animate*AsState` family.
    #[track_caller]
    pub fn animateValue<T: SpringScalar + PartialEq + 'static>(
        &self,
        target_value: T,
        animation: AnimationType,
        label: &str,
    ) -> State<T> {
        let _ = label;
        let caller = cranpose_core::caller_location_key();
        with_current_composer(|composer| {
            let runtime = composer.runtime_handle();
            let anim: Owned<Animatable<T>> = composer.remember_at(caller, || {
                Animatable::new_with_animation(target_value.clone(), animation, runtime)
            });
            anim.update(|animatable| {
                let is_new_target = animatable.target() != target_value;
                let is_new_animation = animatable.animation_type() != animation;
                if is_new_target || is_new_animation {
                    animatable.animateTo(target_value.clone(), animation);
                }
            });

            let animatable_clone = anim.with(|animatable| animatable.clone());
            let identity = animatable_clone.identity();
            let transition_inner = Rc::clone(&self.inner);
            cranpose_core::__disposable_effect_impl(
                caller ^ cranpose_core::location_key(file!(), line!(), column!()),
                identity,
                move |_scope| {
                    let child: Rc<dyn TransitionChild> = Rc::new(animatable_clone);
                    transition_inner.add_child(Rc::clone(&child));
                    let transition_inner = Rc::clone(&transition_inner);
                    DisposableEffectResult::new(move || {
                        transition_inner.remove_child(&child);
                    })
                },
            );

            anim.with(|animatable| animatable.state())
        })
    }

    /// Child float animation. Mirrors Jetpack Compose's `Transition.animateFloat`.
    #[track_caller]
    pub fn animateFloat(
        &self,
        target_value: f32,
        animation: AnimationType,
        label: &str,
    ) -> State<f32> {
        self.animateValue(target_value, animation, label)
    }

    /// Child density-independent-length animation. Mirrors Jetpack Compose's
    /// `Transition.animateDp`.
    #[track_caller]
    pub fn animateDp(&self, target_value: Dp, animation: AnimationType, label: &str) -> State<Dp> {
        self.animateValue(target_value, animation, label)
    }

    /// Child color animation. Mirrors Jetpack Compose's `Transition.animateColor`.
    #[track_caller]
    pub fn animateColor(
        &self,
        target_value: Color,
        animation: AnimationType,
        label: &str,
    ) -> State<Color> {
        self.animateValue(target_value, animation, label)
    }
}

/// Creates or updates a [`Transition`] targeting `target_state`. Every child
/// animation added with [`Transition::animateValue`] (or a typed accessor)
/// retargets when `target_state` changes, continuing from its current value
/// rather than snapping.
///
/// Mirrors Jetpack Compose: `updateTransition(targetState, label)`.
#[track_caller]
pub fn updateTransition<S: Clone + 'static>(target_state: S, label: &str) -> Transition<S> {
    let _ = label;
    let caller = cranpose_core::caller_location_key();
    with_current_composer(|composer| {
        let transition: Owned<Transition<S>> =
            composer.remember_at(caller, || Transition::new(target_state.clone()));
        transition.with(|transition| transition.set_target_state(target_state.clone()));
        transition.with(|transition| transition.clone())
    })
}

#[cfg(test)]
#[path = "tests/transition_tests.rs"]
mod tests;
