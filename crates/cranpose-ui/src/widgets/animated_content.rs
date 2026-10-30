//! `AnimatedContent`: content that changes with a transition.
//!
//! Mirrors Jetpack Compose's `AnimatedContent` from
//! `androidx.compose.animation`. Each content the container shows has its
//! own progress, from `0.0` (gone) to `1.0` (at rest), which its layer reads
//! while it draws: a running transition moves layers and recomposes nothing.
//! Content that left is removed on the frame its exit ends, and a
//! [`SizeTransform`] animates the container's size in layout.
//!
//! Content that a change brings composes as pausable content: past the
//! frame's composition budget it composes over the next frames, and the
//! change waits with what shows until the content is complete, so a large
//! screen does not stop the frames that come before it.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_animation::{Animatable, AnimationType};
use cranpose_core::{
    MutableState, NodeId, PausableContent, RuntimeHandle, internal::FrameCallbackRegistration,
    remember, with_current_composer, with_key, with_pausable_content,
};
use cranpose_ui_graphics::{Size, WindowCoordinates};
use cranpose_ui_layout::{
    Constraints, Measurable, MeasurePolicy, MeasureResult, MeasureScope, Placement,
};

use crate::{
    composable,
    modifier::{GraphicsLayer, Modifier},
    widgets::{
        animated_visibility::{ContainerFrame, EnterTransition, ExitTransition, TransitionEffects},
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
    /// The container that slides across its edge measure from.
    container: Rc<Cell<ContainerFrame>>,
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
        self.effects
            .get()
            .layer(self.progress(), self.size.get(), self.container.get())
    }

    // Animates toward `target` unless it already runs there or rests there;
    // an animation that takes no time puts it there at once, in the frame
    // of the change. Composition calls it, and reads nothing it would
    // recompose by.
    fn animate_to(&self, target: f32) {
        let mut progress = self.progress.borrow_mut();
        let settled = if progress.is_running() {
            progress.target()
        } else {
            progress.state().get_non_reactive()
        };
        if (settled - target).abs() <= f32::EPSILON {
            return;
        }
        match self.animation.get() {
            AnimationType::Tween(spec) if spec.duration_millis == 0 && spec.delay_millis == 0 => {
                progress.snapTo(target);
            }
            animation => progress.animateTo(target, animation),
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
    // The content a change brought, which may compose across frames; none
    // for the first content, which composes at once.
    content: Option<PausableContent>,
}

impl<T> Entry<T> {
    fn is_complete(&self) -> bool {
        self.content
            .as_ref()
            .is_none_or(PausableContent::is_complete)
    }
}

// Tells the container's measure policy which child is content that a
// change brought: it neither measures nor places it until it is complete.
#[derive(Debug, PartialEq)]
struct ChangedContent(PausableContent);

type ContentFn<T> = Box<dyn FnMut(T)>;

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
    /// starts, and its layout direction.
    frame: Rc<Cell<ContainerFrame>>,
    node: Rc<Cell<Option<NodeId>>>,
    // Bumped when leaving content is removed or content a change brought
    // completes, so the container recomposes without the one or with the
    // other.
    entries_changed: MutableState<u64>,
    watcher: RefCell<Option<FrameCallbackRegistration>>,
}

impl<T: Clone + PartialEq + 'static> ContentState<T> {
    fn new(runtime: RuntimeHandle) -> Self {
        Self {
            entries_changed: MutableState::with_runtime(0, runtime.clone()),
            runtime,
            entries: RefCell::new(Vec::new()),
            next_id: Cell::new(0),
            last_target: RefCell::new(None),
            content: RefCell::new(None),
            size_transform: Cell::new(None),
            size: RefCell::new(None),
            frame: Rc::default(),
            coordinates: Rc::default(),
            node: Rc::default(),
            watcher: RefCell::new(None),
        }
    }

    fn push(
        &self,
        value: T,
        progress: f32,
        effects: TransitionEffects,
        animation: AnimationType,
        content: Option<PausableContent>,
    ) {
        let id = self.next_id.get();
        self.next_id.set(id.wrapping_add(1));
        self.entries.borrow_mut().push(Entry {
            id,
            value,
            motion: Rc::new(EntryMotion {
                progress: RefCell::new(Animatable::new(progress, self.runtime.clone())),
                size: Rc::new(Cell::new(Size::ZERO)),
                container: Rc::clone(&self.frame),
                effects: Cell::new(effects),
                animation: Cell::new(animation),
                leaving: Cell::new(false),
                coordinates: Rc::default(),
            }),
            content,
        });
    }

    // Content for a change to compose, which may take frames: once it
    // completes, the container recomposes to start the change and measures
    // again with the content it skipped until then.
    fn changed_content(&self) -> PausableContent {
        let changed = self.entries_changed;
        let node = Rc::clone(&self.node);
        PausableContent::new(move || {
            changed.update(|count| *count = count.wrapping_add(1));
            if let Some(node) = node.get() {
                crate::schedule_measure_repass(node);
            }
        })
    }

    /// Gives the entry that shows the content of `value` that state, and
    /// answers whether there is one: a state with the key of a content shown
    /// changes it in place.
    fn show(&self, value: &T, same: &dyn Fn(&T, &T) -> bool) -> bool {
        let mut entries = self.entries.borrow_mut();
        let Some(entry) = entries.iter_mut().find(|entry| same(&entry.value, value)) else {
            return false;
        };
        if entry.value != *value {
            entry.value = value.clone();
        }
        true
    }

    /// Brings the entries to `target`: it comes in with the transition from
    /// the previous target, everything else leaves, and a seek holds both
    // ends where the gesture is. While the content of `target` is not
    // complete, everything stays as it is and the answer is `false`.
    fn sync(
        &self,
        target: &T,
        seek: Option<&ContentSeek<T>>,
        transition: &dyn Fn(&T, &T) -> ContentTransform,
        same: &dyn Fn(&T, &T) -> bool,
    ) -> bool {
        let seek = seek.filter(|seek| !same(&seek.toward, target));
        if !self.admit(target, seek, transition, same) {
            return false;
        }
        let previous = self.last_target.replace(Some(target.clone()));
        let change = previous
            .filter(|previous| !same(previous, target))
            .map(|previous| transition(&previous, target));
        if let Some(change) = change {
            self.size_transform.set(change.size_transform);
        }
        let seeking = seek.map(|seek| (seek, transition(target, &seek.toward)));
        if let Some((seek, _)) = seeking
            && !self.show(&seek.toward, same)
        {
            self.push(
                seek.toward.clone(),
                0.0,
                TransitionEffects::default(),
                AnimationType::default(),
                Some(self.changed_content()),
            );
        }
        self.aim(target, seeking, change, same);
        true
    }

    // Keeps the entries that may still show and brings one for `target`.
    // Answers whether the content of `target` is complete.
    fn admit(
        &self,
        target: &T,
        seek: Option<&ContentSeek<T>>,
        transition: &dyn Fn(&T, &T) -> ContentTransform,
        same: &dyn Fn(&T, &T) -> bool,
    ) -> bool {
        // Content that never showed leaves once no change heads for it.
        self.entries.borrow_mut().retain(|entry| {
            entry.is_complete()
                || same(&entry.value, target)
                || seek.is_some_and(|seek| same(&entry.value, &seek.toward))
        });
        let first = self.entries.borrow().is_empty();
        if !self.show(target, same) {
            let enter = self
                .last_target
                .borrow()
                .as_ref()
                .filter(|previous| !same(previous, target))
                .map_or_else(EnterTransition::none, |previous| {
                    transition(previous, target).enter
                });
            let progress = if first { 1.0 } else { 0.0 };
            let content = (!first).then(|| self.changed_content());
            self.push(
                target.clone(),
                progress,
                enter.effects(),
                enter.animation(),
                content,
            );
        }
        self.entries
            .borrow()
            .iter()
            .find(|entry| same(&entry.value, target))
            .is_some_and(Entry::is_complete)
    }

    // Sets each entry moving toward where `change` or the seek puts it:
    // the content of `target` comes in and the rest leaves.
    fn aim(
        &self,
        target: &T,
        seeking: Option<(&ContentSeek<T>, ContentTransform)>,
        change: Option<ContentTransform>,
        same: &dyn Fn(&T, &T) -> bool,
    ) {
        let mut entries = self.entries.borrow_mut();
        for entry in entries.iter_mut() {
            let motion = &entry.motion;
            if same(&entry.value, target) {
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
                seeking.filter(|(seek, _)| same(&seek.toward, &entry.value))
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
        if let Some(index) = entries.iter().position(|entry| same(&entry.value, target)) {
            let current = entries.remove(index);
            entries.push(current);
        }
    }

    // Drops content whose exit already ended, as an exit that takes no time
    // does in the pass of its change. Says whether it dropped any.
    fn drop_gone(&self) -> bool {
        let mut entries = self.entries.borrow_mut();
        let before = entries.len();
        // The current content is last; a seek all the way does not remove it.
        let current = entries.last().map(|entry| entry.id);
        entries.retain(|entry| {
            Some(entry.id) == current || !(entry.motion.leaving.get() && entry.motion.gone())
        });
        before != entries.len()
    }

    /// Notes the layout direction when a slide toward the start or the end
    /// needs it.
    fn note_direction(&self) {
        let uses_direction = self
            .entries
            .borrow()
            .iter()
            .any(|entry| entry.motion.effects.get().uses_direction());
        if uses_direction {
            let frame = self.frame.get();
            self.frame.set(ContainerFrame {
                rtl: crate::layout_direction::layout_direction().is_rtl(),
                ..frame
            });
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
        if state.drop_gone() {
            state
                .entries_changed
                .update(|count| *count = count.wrapping_add(1));
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

/// [`AnimatedContent`] where `content_key` tells contents apart: a target
/// whose key is the key of the content shown changes that content in place,
/// with the new state and without a transition — Compose's `contentKey`.
#[composable(no_skip)]
pub fn AnimatedContentWithKey<T, K, F>(
    target_state: T,
    content_key: impl Fn(&T) -> K + 'static,
    transition_spec: impl Fn(&T, &T) -> ContentTransform + 'static,
    content: F,
) where
    T: Clone + PartialEq + 'static,
    K: PartialEq,
    F: FnMut(T) + 'static,
{
    animated_content(
        target_state,
        None,
        &transition_spec,
        &|a, b| content_key(a) == content_key(b),
        content,
    );
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
    animated_content(
        target_state,
        seek,
        &transition_spec,
        &|a, b| a == b,
        content,
    );
}

/// The body of the animated content composables; `same` says whether two
/// states show the same content.
fn animated_content<T, F>(
    target_state: T,
    seek: Option<ContentSeek<T>>,
    transition_spec: &dyn Fn(&T, &T) -> ContentTransform,
    same: &dyn Fn(&T, &T) -> bool,
    content: F,
) where
    T: Clone + PartialEq + 'static,
    F: FnMut(T) + 'static,
{
    let state = remember(|| {
        let runtime = with_current_composer(cranpose_core::Composer::runtime_handle);
        StateHandle(Rc::new(ContentState::<T>::new(runtime)))
    })
    .with(StateHandle::clone);
    *state.0.content.borrow_mut() = Some(Box::new(content));
    let changed = state
        .0
        .sync(&target_state, seek.as_ref(), transition_spec, same);
    if changed {
        state.0.drop_gone();
    }
    state.0.note_direction();
    watch(&state.0);
    // Subscribes this scope to removals of content that left and to content
    // that completed.
    state.0.entries_changed.get();
    // Inside a shared transition layout the container and its contents say
    // where they are, which costs the layer fast paths; elsewhere they don't.
    let shared = in_shared_transition();
    let policy = ContentMeasurePolicy(state.clone());
    let shown = state.clone();
    let node = Layout(container_modifier(&state.0, shared), policy, move || {
        compose_entries(&shown, shared);
    });
    state.0.node.set(Some(node));
    // Content that completed as it composed changes in this pass; content it
    // replaces at once leaves in a recomposition of this frame.
    if !changed
        && state
            .0
            .sync(&target_state, seek.as_ref(), transition_spec, same)
    {
        if state.0.drop_gone() {
            cranpose_core::recompose_current_scope_again();
        }
        state.0.note_direction();
        watch(&state.0);
    }
}

// The container clips when its size transform asks, and says where it is
// inside a shared transition layout.
fn container_modifier<T>(state: &ContentState<T>, shared: bool) -> Modifier {
    let clip = state
        .size_transform
        .get()
        .is_some_and(|transform| transform.clip);
    let modifier = if clip {
        Modifier::empty().clip_to_bounds()
    } else {
        Modifier::empty()
    };
    if shared {
        modifier.report_window_coordinates(Rc::clone(&state.coordinates))
    } else {
        modifier
    }
}

// Composes the content of each entry, keyed by the entry, in the order the
// entries draw. Each entry is read in a borrow of its own, which ends
// before its content composes.
fn compose_entries<T: Clone + PartialEq + 'static>(state: &StateHandle<T>, shared: bool) {
    for index in 0.. {
        let entry = state.0.entries.borrow().get(index).map(|entry| {
            (
                entry.id,
                entry.value.clone(),
                Rc::clone(&entry.motion),
                entry.content.clone(),
            )
        });
        let Some((id, value, motion, pausable)) = entry else {
            break;
        };
        let state = state.clone();
        with_key(&id, move || {
            compose_entry(state, value, &motion, pausable, shared);
        });
    }
}

// Composes the content of one entry in a box that its transition moves.
fn compose_entry<T: Clone + PartialEq + 'static>(
    state: StateHandle<T>,
    value: T,
    motion: &Rc<EntryMotion>,
    pausable: Option<PausableContent>,
    shared: bool,
) {
    let layer = Rc::clone(motion);
    let entry = Modifier::empty().report_size(Rc::clone(&motion.size));
    let entry = match &pausable {
        Some(content) => entry.layout_data(ChangedContent(content.clone())),
        None => entry,
    };
    let entry = if shared {
        entry.report_window_coordinates(Rc::clone(&motion.coordinates))
    } else {
        entry
    };
    let shared_entry = shared.then(|| SharedEntry {
        motion: Rc::clone(motion),
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
            let compose = || match shared_entry.clone() {
                Some(entry) => ProvideSharedEntry(entry, compose),
                None => compose(),
            };
            match &pausable {
                Some(content) => with_pausable_content(content, compose),
                None => compose(),
            }
        },
    );
}

// Whether `measurable` is content a change brought that is not complete.
fn is_composing(measurable: &dyn Measurable) -> bool {
    measurable
        .parent_data()
        .layout_data::<ChangedContent>()
        .is_some_and(|content| !content.0.is_complete())
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
        for measurable in measurables
            .iter()
            .filter(|measurable| !is_composing(measurable.as_ref()))
        {
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
                    Animatable::new(state.frame.get().size, state.runtime.clone())
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
        state.frame.set(ContainerFrame {
            size,
            ..state.frame.get()
        });
        MeasureResult::new(size, placements)
    }

    fn min_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        measurables
            .iter()
            .filter(|measurable| !is_composing(measurable.as_ref()))
            .map(|measurable| measurable.min_intrinsic_width(height))
            .fold(0.0, f32::max)
    }

    fn max_intrinsic_width(&self, measurables: &[Box<dyn Measurable>], height: f32) -> f32 {
        measurables
            .iter()
            .filter(|measurable| !is_composing(measurable.as_ref()))
            .map(|measurable| measurable.max_intrinsic_width(height))
            .fold(0.0, f32::max)
    }

    fn min_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        measurables
            .iter()
            .filter(|measurable| !is_composing(measurable.as_ref()))
            .map(|measurable| measurable.min_intrinsic_height(width))
            .fold(0.0, f32::max)
    }

    fn max_intrinsic_height(&self, measurables: &[Box<dyn Measurable>], width: f32) -> f32 {
        measurables
            .iter()
            .filter(|measurable| !is_composing(measurable.as_ref()))
            .map(|measurable| measurable.max_intrinsic_height(width))
            .fold(0.0, f32::max)
    }
}
