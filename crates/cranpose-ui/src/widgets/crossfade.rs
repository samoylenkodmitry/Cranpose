//! Crossfade composable
//!
//! Mirrors Jetpack Compose's `Crossfade` from
//! `androidx.compose.animation.Crossfade`.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_animation::{Animatable, AnimationType};
use cranpose_core::{State, remember, with_current_composer};

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
    id: u64,
    value: T,
    alpha: Option<State<f32>>,
    fade_in: bool,
}

type CrossfadeContentFn<T> = std::boxed::Box<dyn FnMut(T)>;

struct CrossfadeStateInner<T> {
    entries: RefCell<Vec<CrossfadeEntry<T>>>,
    next_id: Cell<u64>,
    content: RefCell<Option<CrossfadeContentFn<T>>>,
}

/// The alpha of one crossfade content: `seeked` while a gesture holds it,
/// else animated toward `target` from wherever it stands, which a content
/// shown for the first time takes as `initial`.
fn content_alpha(
    initial: f32,
    target: f32,
    seeked: Option<f32>,
    animation: AnimationType,
) -> State<f32> {
    let runtime = with_current_composer(cranpose_core::Composer::runtime_handle);
    let alpha = remember(|| Animatable::new_with_animation(initial, animation, runtime));
    alpha.update(|alpha| match seeked {
        Some(seeked) => {
            if alpha.is_running() || (alpha.target() - seeked).abs() > f32::EPSILON {
                alpha.snapTo(seeked);
            }
        }
        None => {
            if (alpha.target() - target).abs() > f32::EPSILON || alpha.animation_type() != animation
            {
                alpha.animateTo(target, animation);
            }
        }
    });
    alpha.with(Animatable::state)
}

/// How far a gesture moved a [`SeekableCrossfade`] from its target state
/// toward another state.
#[derive(Clone, Debug, PartialEq)]
pub struct CrossfadeSeek<T> {
    /// The state the gesture moves toward.
    pub toward: T,
    /// From 0, the target state shown alone, to 1, `toward` shown alone.
    pub fraction: f32,
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

impl<T: PartialEq + Clone> CrossfadeStateHandle<T> {
    fn new_entry(&self, value: T, fade_in: bool) -> CrossfadeEntry<T> {
        CrossfadeEntry {
            id: self.allocate_id(),
            value,
            alpha: None,
            fade_in,
        }
    }

    /// Adds an entry for the target and for the state a seek moves toward,
    /// beneath the others, and drops the entries that finished fading out.
    fn sync_entries(&self, target: &T, seek: Option<&CrossfadeSeek<T>>) {
        let mut entries = self.inner.entries.borrow_mut();
        if !entries.iter().any(|entry| entry.value == *target) {
            let fade_in = !entries.is_empty();
            entries.push(self.new_entry(target.clone(), fade_in));
        }
        if let Some(seek) = seek
            && !entries.iter().any(|entry| entry.value == seek.toward)
        {
            entries.insert(0, self.new_entry(seek.toward.clone(), true));
        }
        entries.retain(|entry| {
            entry.value == *target
                || seek.is_some_and(|seek| seek.toward == entry.value)
                || entry
                    .alpha
                    .is_none_or(|alpha| alpha.value() > CROSSFADE_ALPHA_EPSILON)
        });
    }
}

/// The alpha a seek holds the content of `value` at: the rest of the fraction
/// for the target, the fraction for the state it moves toward.
fn seeked_alpha<T: PartialEq>(
    seek: Option<&CrossfadeSeek<T>>,
    value: &T,
    is_target: bool,
) -> Option<f32> {
    let seek = seek?;
    let fraction = seek.fraction.clamp(0.0, 1.0);
    if is_target {
        Some(1.0 - fraction)
    } else {
        (seek.toward == *value).then_some(fraction)
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
    SeekableCrossfade(target_state, None, animation, content);
}

/// [`Crossfade`] that a gesture can move part of the way, as Compose's
/// `SeekableTransitionState` moves a transition. While `seek` is set, the
/// content for `seek.toward` shows beneath the target's at
/// `seek.fraction`, and the target's at the rest. When the gesture ends, the
/// target either becomes `seek.toward` or stays, and both contents animate
/// on from where the gesture left them, with `animation`.
#[composable(no_skip)]
pub fn SeekableCrossfade<T, F>(
    target_state: T,
    seek: Option<CrossfadeSeek<T>>,
    animation: AnimationType,
    content: F,
) where
    T: Clone + PartialEq + 'static,
    F: FnMut(T) + 'static,
{
    let handle = remember(|| CrossfadeStateHandle::<T>::new()).with(CrossfadeStateHandle::clone);
    *handle.inner.content.borrow_mut() = Some(std::boxed::Box::new(content));
    let seek = seek.filter(|seek| seek.toward != target_state);
    CrossfadeContents(handle, target_state, seek, animation);
}

/// Reactive part of [`Crossfade`]: reads the per-entry alpha states so each
/// animation frame recomposes it, and drops entries whose fade-out finished.
/// It cannot skip: the content it runs is the closure [`Crossfade`] just put
/// in the handle, which none of these three parameters describes.
#[composable(no_skip)]
fn CrossfadeContents<T>(
    state: CrossfadeStateHandle<T>,
    target_state: T,
    seek: Option<CrossfadeSeek<T>>,
    animation: AnimationType,
) where
    T: Clone + PartialEq + 'static,
{
    state.sync_entries(&target_state, seek.as_ref());

    Box(Modifier::empty(), BoxSpec::new(), move || {
        let items: Vec<(u64, T, bool)> = state
            .inner
            .entries
            .borrow()
            .iter()
            .map(|entry| (entry.id, entry.value.clone(), entry.fade_in))
            .collect();
        for (id, value, fade_in) in items {
            let is_target = value == target_state;
            let seeked = seeked_alpha(seek.as_ref(), &value, is_target);
            let state_for_item = state.clone();
            cranpose_core::with_key(&id, || {
                let alpha_target = if is_target { 1.0 } else { 0.0 };
                let initial_alpha = if fade_in { 0.0 } else { alpha_target };
                let alpha = content_alpha(initial_alpha, alpha_target, seeked, animation);
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
