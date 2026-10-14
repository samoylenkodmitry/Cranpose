//! `AnimatedContent`: content that changes with a transition.
//!
//! Mirrors Jetpack Compose's `AnimatedContent` from
//! `androidx.compose.animation`. Each content the container shows has its
//! own progress, from `0.0` (gone) to `1.0` (at rest), which its layer reads
//! while it draws: a running transition moves layers and recomposes nothing.
//! Content that left is removed on the frame its exit ends, and a
//! [`SizeTransform`] animates the container's size in layout.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_animation::{Animatable, AnimationType};
use cranpose_core::{
    MutableState, NodeId, RuntimeHandle, internal::FrameCallbackRegistration, remember,
    with_current_composer, with_key,
};
use cranpose_ui_graphics::{Size, WindowCoordinates};
use cranpose_ui_layout::{
    Constraints, Measurable, MeasurePolicy, MeasureResult, MeasureScope, Placement,
};

use crate::{
    composable,
    modifier::{GraphicsLayer, Modifier},
    widgets::{
        animated_visibility::{EnterTransition, ExitTransition, TransitionEffects},
        box_widget::{Box, BoxSpec},
        layout::Layout,
        shared_transition::{ProvideSharedEntry, SharedEntry, in_shared_transition},
    },
};

/// Progress below which leaving content is gone and leaves the composition.
const GONE: f32 = 0.001;

/// How an [`AnimatedContent`] changes from one content to the next.
///
/// Mirrors Jetpack Compose's `ContentTransform`: build one with
/// `enter.together_with(exit)` and add a size animation with
/// [`using`](Self::using).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContentTransform {
    /// Transition that plays on the content that comes in.
    pub enter: EnterTransition,
    /// Transition that plays on the content that goes out.
    pub exit: ExitTransition,
    /// How the container's size follows the content's; `None` keeps the
    /// container as large as the largest content during the change.
    pub size_transform: Option<SizeTransform>,
}

impl ContentTransform {
    /// Animates the container's size with `size_transform`. Compose's
    /// `transform using SizeTransform()`.
    pub fn using(mut self, size_transform: SizeTransform) -> Self {
        self.size_transform = Some(size_transform);
        self
    }
}

/// Animates an [`AnimatedContent`]'s size from the content that leaves to
/// the content that comes. Compose's `SizeTransform`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SizeTransform {
    /// Whether content outside the animating size is clipped.
    pub clip: bool,
    /// Animation that drives the size change.
    pub animation: AnimationType,
}

impl Default for SizeTransform {
    fn default() -> Self {
        Self {
            clip: true,
            animation: AnimationType::default(),
        }
    }
}

/// A gesture part of the way from the current content toward another, as
/// predictive back is: the content `toward` shows at `fraction` and the
/// current content at `1.0 - fraction`, each with the transition from the
/// current content to `toward`.
#[derive(Clone, Debug, PartialEq)]
pub struct ContentSeek<T> {
    /// The content that the gesture heads toward.
    pub toward: T,
    /// How far the gesture has come toward `toward`, from 0.0 (still at the current content) to 1.0
    /// (at `toward`).
    pub fraction: f32,
}

/// How far one content is in the container, and how it shows on the way.
pub(crate) struct EntryMotion {
    progress: RefCell<Animatable<f32>>,
    size: Rc<Cell<Size>>,
    effects: Cell<TransitionEffects>,
    animation: Cell<AnimationType>,
    /// Whether the content goes, rather than comes or rests.
    pub(crate) leaving: Cell<bool>,
    /// Where the content's box is, its layer included, inside a shared
    /// transition layout.
    coordinates: Rc<Cell<WindowCoordinates>>,
}

impl EntryMotion {
    pub(crate) fn progress(&self) -> f32 {
        self.progress.borrow().state().value()
    }

    fn layer(&self) -> GraphicsLayer {
        self.effects.get().layer(self.progress(), self.size.get())
    }

    /// Animates toward `target` unless it already runs there or rests there.
    /// Composition calls it, and reads nothing it would recompose by.
    fn animate_to(&self, target: f32) {
        let mut progress = self.progress.borrow_mut();
        let settled = if progress.is_running() {
            progress.target()
        } else {
            progress.state().get_non_reactive()
        };
        if (settled - target).abs() > f32::EPSILON {
            progress.animateTo(target, self.animation.get());
        }
    }

    fn snap_to(&self, target: f32) {
        self.progress.borrow_mut().snapTo(target);
    }

    /// Shown in full with nothing running: the content a change leaves
    /// takes the change's exit transition only from here.
    fn at_rest(&self) -> bool {
        let progress = self.progress.borrow();
        !progress.is_running() && progress.state().get_non_reactive() >= 1.0 - GONE
    }

    fn gone(&self) -> bool {
        let progress = self.progress.borrow();
        !progress.is_running() && progress.state().get_non_reactive() <= GONE
    }
}

struct Entry<T> {
    id: u64,
    value: T,
    motion: Rc<EntryMotion>,
}

type ContentFn<T> = std::boxed::Box<dyn FnMut(T)>;

struct ContentState<T> {
    runtime: RuntimeHandle,
    /// Leaving content first, the current content last, on top.
    entries: RefCell<Vec<Entry<T>>>,
    next_id: Cell<u64>,
    last_target: RefCell<Option<T>>,
    content: RefCell<Option<ContentFn<T>>>,
    size_transform: Cell<Option<SizeTransform>>,
    size: RefCell<Option<Animatable<Size>>>,
    /// Where the container is, inside a shared transition layout.
    coordinates: Rc<Cell<WindowCoordinates>>,
    /// The container's size from its last measure, where a size animation
    /// starts.
    last_size: Cell<Size>,
    node: Cell<Option<NodeId>>,
    /// Bumped when leaving content is removed, so the container recomposes
    /// without it.
    removed: MutableState<u64>,
    watcher: RefCell<Option<FrameCallbackRegistration>>,
}

impl<T: Clone + PartialEq + 'static> ContentState<T> {
    fn new(runtime: RuntimeHandle) -> Self {
        Self {
            removed: MutableState::with_runtime(0, runtime.clone()),
            runtime,
            entries: RefCell::new(Vec::new()),
            next_id: Cell::new(0),
            last_target: RefCell::new(None),
            content: RefCell::new(None),
            size_transform: Cell::new(None),
            size: RefCell::new(None),
            last_size: Cell::new(Size::ZERO),
            coordinates: Rc::default(),
            node: Cell::new(None),
            watcher: RefCell::new(None),
        }
    }

    fn push(&self, value: T, progress: f32, effects: TransitionEffects, animation: AnimationType) {
        let id = self.next_id.get();
        self.next_id.set(id.wrapping_add(1));
        self.entries.borrow_mut().push(Entry {
            id,
            value,
            motion: Rc::new(EntryMotion {
                progress: RefCell::new(Animatable::new(progress, self.runtime.clone())),
                size: Rc::new(Cell::new(Size::ZERO)),
                effects: Cell::new(effects),
                animation: Cell::new(animation),
                leaving: Cell::new(false),
                coordinates: Rc::default(),
            }),
        });
    }

    /// Brings the entries to `target`: it comes in with the transition from
    /// the previous target, everything else leaves, and a seek holds both
    /// ends where the gesture is.
    fn sync(
        &self,
        target: &T,
        seek: Option<&ContentSeek<T>>,
        transition: &dyn Fn(&T, &T) -> ContentTransform,
    ) {
        let previous = self.last_target.replace(Some(target.clone()));
        let change = previous
            .filter(|previous| previous != target)
            .map(|previous| transition(&previous, target));
        if let Some(change) = change {
            self.size_transform.set(change.size_transform);
        }
        let first = self.entries.borrow().is_empty();
        if !self
            .entries
            .borrow()
            .iter()
            .any(|entry| entry.value == *target)
        {
            let enter = change.map_or_else(EnterTransition::none, |change| change.enter);
            let progress = if first { 1.0 } else { 0.0 };
            self.push(target.clone(), progress, enter.effects(), enter.animation());
        }
        let seek = seek.filter(|seek| seek.toward != *target);
        let seeking = seek.map(|seek| (seek, transition(target, &seek.toward)));
        if let Some((seek, _)) = seeking
            && !self
                .entries
                .borrow()
                .iter()
                .any(|entry| entry.value == seek.toward)
        {
            self.push(
                seek.toward.clone(),
                0.0,
                TransitionEffects::default(),
                AnimationType::default(),
            );
        }
        let mut entries = self.entries.borrow_mut();
        for entry in entries.iter_mut() {
            let motion = &entry.motion;
            if entry.value == *target {
                // New content took its enter transition when it was added;
                // content that turns back runs its own transition backward.
                // A gesture that seeks away from it makes it the content that
                // goes, for the shared elements in it.
                entry.motion.leaving.set(seeking.is_some());
                match seeking {
                    Some((seek, seek_change)) => {
                        motion.effects.set(seek_change.exit.effects());
                        motion.snap_to(1.0 - seek.fraction.clamp(0.0, 1.0));
                    }
                    None => motion.animate_to(1.0),
                }
            } else if let Some((seek, seek_change)) =
                seeking.filter(|(seek, _)| seek.toward == entry.value)
            {
                entry.motion.leaving.set(false);
                motion.effects.set(seek_change.enter.effects());
                motion.animation.set(seek_change.enter.animation());
                motion.snap_to(seek.fraction.clamp(0.0, 1.0));
            } else {
                if !entry.motion.leaving.replace(true)
                    && let Some(change) = change.filter(|_| motion.at_rest())
                {
                    motion.effects.set(change.exit.effects());
                    motion.animation.set(change.exit.animation());
                }
                motion.animate_to(0.0);
            }
        }
        // The current content draws on top.
        if let Some(index) = entries.iter().position(|entry| entry.value == *target) {
            let current = entries.remove(index);
            entries.push(current);
        }
    }

    fn animating(&self) -> bool {
        self.entries
            .borrow()
            .iter()
            .any(|entry| entry.motion.leaving.get())
            || self
                .size
                .borrow()
                .as_ref()
                .is_some_and(Animatable::is_running)
    }
}

/// Removes leaving content on the frame its exit ends, and measures the
/// container again while its size animates.
fn watch<T: Clone + PartialEq + 'static>(state: &Rc<ContentState<T>>) {
    if state.watcher.borrow().is_some() || !state.animating() {
        return;
    }
    let weak = Rc::downgrade(state);
    let registration = state.runtime.frame_clock().with_frame_nanos(move |_| {
        let Some(state) = weak.upgrade() else {
            return;
        };
        state.watcher.borrow_mut().take();
        let removed = {
            let mut entries = state.entries.borrow_mut();
            let before = entries.len();
            // The current content is last; a seek all the way does not
            // remove it.
            let current = entries.last().map(|entry| entry.id);
            entries.retain(|entry| {
                Some(entry.id) == current || !(entry.motion.leaving.get() && entry.motion.gone())
            });
            before != entries.len()
        };
        if removed {
            state.removed.update(|count| *count = count.wrapping_add(1));
        }
        if state
            .size
            .borrow()
            .as_ref()
            .is_some_and(Animatable::is_running)
            && let Some(node) = state.node.get()
        {
            crate::schedule_measure_repass(node);
            crate::request_render_invalidation();
        }
        watch(&state);
    });
    *state.watcher.borrow_mut() = Some(registration);
}

struct StateHandle<T>(Rc<ContentState<T>>);

impl<T> Clone for StateHandle<T> {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

impl<T> PartialEq for StateHandle<T> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// Changes `content` with a transition when `target_state` changes.
///
/// `transition_spec` receives the state that goes and the state that comes,
/// and answers how they change: `fade_in().together_with(fade_out())`,
/// with `.using(SizeTransform::default())` to animate the size between
/// them. The first content shows at rest. Content that leaves stays
/// composed until its exit ends; a change back while it leaves brings it
/// back from where it was.
///
/// Mirrors Jetpack Compose:
/// `AnimatedContent(targetState, transitionSpec = { ... }) { state -> ... }`.
#[composable(no_skip)]
pub fn AnimatedContent<T, F>(
    target_state: T,
    transition_spec: impl Fn(&T, &T) -> ContentTransform + 'static,
    content: F,
) where
    T: Clone + PartialEq + 'static,
    F: FnMut(T) + 'static,
{
    SeekableAnimatedContent(target_state, None, transition_spec, content);
}

/// [`AnimatedContent`] that a gesture can hold part of the way toward
/// another state, as predictive back does: while `seek` is set, its
/// `toward` content shows at its fraction. When the gesture ends the change
/// finishes or goes back from where it stood, depending on whether the
/// target became `toward`.
#[composable(no_skip)]
pub fn SeekableAnimatedContent<T, F>(
    target_state: T,
    seek: Option<ContentSeek<T>>,
    transition_spec: impl Fn(&T, &T) -> ContentTransform + 'static,
    content: F,
) where
    T: Clone + PartialEq + 'static,
    F: FnMut(T) + 'static,
{
    let runtime = with_current_composer(|composer| composer.runtime_handle());
    let state =
        remember(|| StateHandle(Rc::new(ContentState::<T>::new(runtime)))).with(StateHandle::clone);
    *state.0.content.borrow_mut() = Some(std::boxed::Box::new(content));
    state.0.sync(&target_state, seek.as_ref(), &transition_spec);
    watch(&state.0);
    // Subscribes this scope to removals of content that left.
    state.0.removed.get();
    let clip = state
        .0
        .size_transform
        .get()
        .is_some_and(|transform| transform.clip);
    let modifier = if clip {
        Modifier::empty().clip_to_bounds()
    } else {
        Modifier::empty()
    };
    // Inside a shared transition layout the container and its contents say
    // where they are, which costs the layer fast paths; elsewhere they don't.
    let shared = in_shared_transition();
    let modifier = if shared {
        modifier.report_window_coordinates(Rc::clone(&state.0.coordinates))
    } else {
        modifier
    };
    let policy = ContentMeasurePolicy(state.clone());
    let shown = state.clone();
    let node = Layout(modifier, policy, move || {
        let state = &shown;
        let entries: Vec<(u64, T, Rc<EntryMotion>)> = state
            .0
            .entries
            .borrow()
            .iter()
            .map(|entry| (entry.id, entry.value.clone(), Rc::clone(&entry.motion)))
            .collect();
        for (id, value, motion) in entries {
            let state = state.clone();
            with_key(&id, move || {
                let layer = Rc::clone(&motion);
                let entry = Modifier::empty().report_size(Rc::clone(&motion.size));
                let entry = if shared {
                    entry.report_window_coordinates(Rc::clone(&motion.coordinates))
                } else {
                    entry
                };
                let shared_entry = shared.then(|| SharedEntry {
                    motion: Rc::clone(&motion),
                    entry: Rc::clone(&motion.coordinates),
                    container: Rc::clone(&state.0.coordinates),
                });
                Box(
                    entry.graphics_layer(move || layer.layer()),
                    BoxSpec::new(),
                    move || {
                        let compose = || {
                            if let Some(content) = state.0.content.borrow_mut().as_mut() {
                                content(value.clone());
                            }
                        };
                        match shared_entry.clone() {
                            Some(entry) => ProvideSharedEntry(entry, compose),
                            None => compose(),
                        }
                    },
                );
            });
        }
    });
    state.0.node.set(Some(node));
}

/// Places every content at the top start; the container is as large as the
/// largest content, or follows the current content's size through a
/// [`SizeTransform`].
struct ContentMeasurePolicy<T>(StateHandle<T>);

impl<T> Clone for ContentMeasurePolicy<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> PartialEq for ContentMeasurePolicy<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<T: Clone + PartialEq + 'static> MeasurePolicy for ContentMeasurePolicy<T> {
    fn measure(
        &self,
        _scope: &dyn MeasureScope,
        measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let state = &self.0.0;
        let loose = Constraints {
            min_width: 0.0,
            min_height: 0.0,
            ..constraints
        };
        let mut largest = Size::ZERO;
        let mut current = Size::ZERO;
        let mut placements = Vec::with_capacity(measurables.len());
        for measurable in measurables {
            let placeable = measurable.measure(loose);
            let size = Size::new(placeable.width(), placeable.height());
            largest = Size::new(
                largest.width.max(size.width),
                largest.height.max(size.height),
            );
            // The current content is the last.
            current = size;
            placeable.place(0.0, 0.0);
            placements.push(Placement::new(placeable.node_id(), 0.0, 0.0, 0));
        }
        let size = match state.size_transform.get() {
            Some(transform) => {
                let mut animated = state.size.borrow_mut();
                let animated = animated.get_or_insert_with(|| {
                    Animatable::new(state.last_size.get(), state.runtime.clone())
                });
                if animated.target() != current {
                    animated.animateTo(current, transform.animation);
                }
                animated.state().value()
            }
            None => {
                state.size.borrow_mut().take();
                largest
            }
        };
        let size = Size::new(
            size.width
                .clamp(constraints.min_width, constraints.max_width),
            size.height
                .clamp(constraints.min_height, constraints.max_height),
        );
        state.last_size.set(size);
        MeasureResult::new(size, placements)
    }

    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        measurables
            .iter()
            .map(|measurable| measurable.min_intrinsic_width(height))
            .fold(0.0, f32::max)
    }

    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        measurables
            .iter()
            .map(|measurable| measurable.max_intrinsic_width(height))
            .fold(0.0, f32::max)
    }

    fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        measurables
            .iter()
            .map(|measurable| measurable.min_intrinsic_height(width))
            .fold(0.0, f32::max)
    }

    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        measurables
            .iter()
            .map(|measurable| measurable.max_intrinsic_height(width))
            .fold(0.0, f32::max)
    }
}
