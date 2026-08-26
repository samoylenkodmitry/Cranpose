//! Crossfade composable
//!
//! Mirrors Jetpack Compose's `Crossfade` from
//! `androidx.compose.animation.Crossfade`.

#![allow(non_snake_case)]

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_animation::AnimationType;
use cranpose_core::{State, remember};

use crate::{
    composable,
    modifier::{GraphicsLayer, Modifier},
    widgets::box_widget::{Box, BoxSpec},
};

/// Alpha below which a fading-out content is considered fully transparent
/// and can leave the composition.
const CROSSFADE_ALPHA_EPSILON: f32 = 0.001;

/// Animate a float towards `target`, starting from `initial` when the value
/// is first composed. Thin alias over
/// [`cranpose_animation::animate_float_as_state_with_initial`].
pub(crate) fn animate_float_with_initial(
    initial: f32,
    target: f32,
    animation: AnimationType,
) -> State<f32> {
    cranpose_animation::animate_float_as_state_with_initial(initial, target, animation, "crossfade")
}

struct CrossfadeEntry<T> {
    /// Stable key for the composition group of this content.
    id: u64,
    value: T,
    /// Alpha state produced the last time this entry was composed.
    alpha: Option<State<f32>>,
    /// Whether this entry should fade in from 0 when it first appears.
    fade_in: bool,
}

type CrossfadeContentFn<T> = std::boxed::Box<dyn FnMut(T)>;

struct CrossfadeStateInner<T> {
    entries: RefCell<Vec<CrossfadeEntry<T>>>,
    next_id: Cell<u64>,
    content: RefCell<Option<CrossfadeContentFn<T>>>,
}

/// Shared, cheaply cloneable holder for the in-flight crossfade contents.
struct CrossfadeStateHandle<T> {
    inner: Rc<CrossfadeStateInner<T>>,
}

impl<T> CrossfadeStateHandle<T> {
    fn new() -> Self {
        Self {
            inner: Rc::new(CrossfadeStateInner {
                entries: RefCell::new(Vec::new()),
                next_id: Cell::new(0),
                content: RefCell::new(None),
            }),
        }
    }

    fn allocate_id(&self) -> u64 {
        let id = self.inner.next_id.get();
        self.inner.next_id.set(id.wrapping_add(1));
        id
    }
}

impl<T> Clone for CrossfadeStateHandle<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl<T> PartialEq for CrossfadeStateHandle<T> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

/// Crossfade allows switching between two layouts with a crossfade
/// animation.
///
/// Mirrors Jetpack Compose:
/// `Crossfade(targetState, animationSpec) { state -> ... }`.
///
/// When `target_state` changes, the content for the previous state fades out
/// while the content for the new state fades in. Compose semantics are
/// preserved: both compositions stay alive during the transition, each
/// wrapped in an alpha graphics layer, and the outgoing content leaves the
/// composition once its fade-out completes. The content shown on first
/// composition appears without an animation, and retargeting mid-transition
/// keeps every in-flight content fading until it finishes.
///
/// `content` is invoked with each in-flight state, so it must not capture
/// the current target; use the provided value instead.
#[composable(no_skip)]
pub fn Crossfade<T, F>(target_state: T, animation: AnimationType, content: F)
where
    T: Clone + PartialEq + 'static,
    F: FnMut(T) + 'static,
{
    let handle = remember(|| CrossfadeStateHandle::<T>::new()).with(CrossfadeStateHandle::clone);
    // Refresh the stored content closure so recompositions triggered by
    // animation frames observe the latest captured environment.
    *handle.inner.content.borrow_mut() = Some(std::boxed::Box::new(content));
    CrossfadeContents(handle, target_state, animation);
}

/// Reactive part of [`Crossfade`]: reads the per-entry alpha states so each
/// animation frame recomposes it, and drops entries whose fade-out finished.
#[composable]
fn CrossfadeContents<T>(state: CrossfadeStateHandle<T>, target_state: T, animation: AnimationType)
where
    T: Clone + PartialEq + 'static,
{
    {
        let mut entries = state.inner.entries.borrow_mut();
        if entries.is_empty() {
            // First composition: the initial content appears without a fade,
            // matching Compose (the transition starts at the target state).
            let id = state.allocate_id();
            entries.push(CrossfadeEntry {
                id,
                value: target_state.clone(),
                alpha: None,
                fade_in: false,
            });
        } else if !entries.iter().any(|entry| entry.value == target_state) {
            let id = state.allocate_id();
            entries.push(CrossfadeEntry {
                id,
                value: target_state.clone(),
                alpha: None,
                fade_in: true,
            });
        }

        // Remove contents whose fade-out completed. Reading the alpha state
        // here subscribes this recompose scope, so every animation frame
        // re-evaluates the retention decision.
        entries.retain(|entry| {
            entry.value == target_state
                || entry
                    .alpha
                    .is_none_or(|alpha| alpha.value() > CROSSFADE_ALPHA_EPSILON)
        });
    }

    let state_for_items = state.clone();
    Box(Modifier::empty(), BoxSpec::new(), move || {
        let items: Vec<(u64, T, bool)> = state_for_items
            .inner
            .entries
            .borrow()
            .iter()
            .map(|entry| (entry.id, entry.value.clone(), entry.fade_in))
            .collect();
        for (id, value, fade_in) in items {
            let is_target = value == target_state;
            let state_for_item = state_for_items.clone();
            cranpose_core::with_key(&id, || {
                let alpha_target = if is_target { 1.0 } else { 0.0 };
                let initial_alpha = if fade_in { 0.0 } else { alpha_target };
                let alpha = animate_float_with_initial(initial_alpha, alpha_target, animation);
                if let Some(entry) = state_for_item
                    .inner
                    .entries
                    .borrow_mut()
                    .iter_mut()
                    .find(|entry| entry.id == id)
                {
                    entry.alpha = Some(alpha);
                }

                let layer_alpha = alpha.value().clamp(0.0, 1.0);
                let state_for_content = state_for_item.clone();
                Box(
                    Modifier::empty().graphics_layer_value(GraphicsLayer {
                        alpha: layer_alpha,
                        ..Default::default()
                    }),
                    BoxSpec::new(),
                    move || {
                        let mut content = state_for_content.inner.content.borrow_mut();
                        if let Some(content) = content.as_mut() {
                            content(value.clone());
                        }
                    },
                );
            });
        }
    });
}
