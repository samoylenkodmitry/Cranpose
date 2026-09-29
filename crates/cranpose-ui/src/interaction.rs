#![expect(non_snake_case)]

use std::{
    cell::RefCell,
    collections::HashSet,
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_core::{
    MutableState, OwnedMutableState, RuntimeHandle, State, remember, with_current_composer,
};
use cranpose_foundation::{
    DelegatableNode, InvalidationKind, ModifierNode, ModifierNodeContext, ModifierNodeElement,
    NodeCapabilities, NodeState, PointerInputNode,
};

use crate::{
    composable,
    modifier::{Modifier, Point, PointerEvent, PointerEventKind, inspector_metadata},
};

#[derive(Clone, Copy)]
pub struct MutableInteractionSource {
    inner: MutableState<Rc<MutableInteractionSourceInner>>,
}

struct MutableInteractionSourceInner {
    next_id: RefCell<u64>,
    active_presses: RefCell<HashSet<u64>>,
    active_hovers: RefCell<HashSet<u64>>,
    pressed: OwnedMutableState<bool>,
    hovered: OwnedMutableState<bool>,
    last_interaction: OwnedMutableState<Option<Interaction>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Interaction {
    Press(PressInteraction),
    Hover(HoverInteraction),
}

/// The pointer resting over a node: Compose's `HoverInteraction`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HoverInteraction {
    Enter(HoverInteractionEnter),
    Exit(HoverInteractionExit),
}

/// A pointer came to rest over a node.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoverInteractionEnter {
    id: u64,
}

/// The pointer that [`Self::enter`] reported left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoverInteractionExit {
    pub enter: HoverInteractionEnter,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PressInteraction {
    Press(PressInteractionPress),
    Release(PressInteractionRelease),
    Cancel(PressInteractionCancel),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PressInteractionPress {
    id: u64,
    pub press_position: Point,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PressInteractionRelease {
    pub press: PressInteractionPress,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PressInteractionCancel {
    pub press: PressInteractionPress,
}

impl MutableInteractionSource {
    pub fn new() -> Self {
        let runtime = with_current_composer(cranpose_core::Composer::runtime_handle);
        Self::with_runtime(runtime)
    }

    pub fn with_runtime(runtime: RuntimeHandle) -> Self {
        Self {
            inner: MutableState::with_runtime(
                Rc::new(MutableInteractionSourceInner {
                    next_id: RefCell::new(1),
                    active_presses: RefCell::new(HashSet::new()),
                    active_hovers: RefCell::new(HashSet::new()),
                    pressed: OwnedMutableState::with_runtime(false, runtime.clone()),
                    hovered: OwnedMutableState::with_runtime(false, runtime.clone()),
                    last_interaction: OwnedMutableState::with_runtime(None, runtime.clone()),
                }),
                runtime,
            ),
        }
    }

    fn inner(&self) -> Rc<MutableInteractionSourceInner> {
        self.inner.get_non_reactive()
    }

    pub fn id(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.inner.runtime_state_id().hash(&mut hasher);
        hasher.finish()
    }

    /// The next id this source hands an interaction.
    fn next_id(&self) -> u64 {
        let inner = self.inner();
        let mut next_id = inner.next_id.borrow_mut();
        let id = *next_id;
        *next_id = id.saturating_add(1);
        id
    }

    pub fn press(&self, press_position: Point) -> PressInteractionPress {
        let press = PressInteractionPress {
            id: self.next_id(),
            press_position,
        };
        self.emit(Interaction::Press(PressInteraction::Press(press)));
        press
    }

    pub fn release(&self, press: PressInteractionPress) {
        self.emit(Interaction::Press(PressInteraction::Release(
            PressInteractionRelease { press },
        )));
    }

    pub fn cancel(&self, press: PressInteractionPress) {
        self.emit(Interaction::Press(PressInteraction::Cancel(
            PressInteractionCancel { press },
        )));
    }

    /// Reports that a pointer came to rest over a node, Compose's
    /// `emit(HoverInteraction.Enter())`, and returns the enter to end it
    /// with.
    pub fn enter_hover(&self) -> HoverInteractionEnter {
        let enter = HoverInteractionEnter { id: self.next_id() };
        self.emit(Interaction::Hover(HoverInteraction::Enter(enter)));
        enter
    }

    /// Reports that the pointer `enter` reported left.
    pub fn exit_hover(&self, enter: HoverInteractionEnter) {
        self.emit(Interaction::Hover(HoverInteraction::Exit(
            HoverInteractionExit { enter },
        )));
    }

    pub fn emit(&self, interaction: Interaction) {
        let inner = self.inner();
        inner.last_interaction.set(Some(interaction));
        let (active, state, id, starts) = match interaction {
            Interaction::Press(press) => {
                let (id, starts) = match press {
                    PressInteraction::Press(press) => (press.id, true),
                    PressInteraction::Release(release) => (release.press.id, false),
                    PressInteraction::Cancel(cancel) => (cancel.press.id, false),
                };
                (&inner.active_presses, &inner.pressed, id, starts)
            }
            Interaction::Hover(hover) => {
                let (id, starts) = match hover {
                    HoverInteraction::Enter(enter) => (enter.id, true),
                    HoverInteraction::Exit(exit) => (exit.enter.id, false),
                };
                (&inner.active_hovers, &inner.hovered, id, starts)
            }
        };
        let any = {
            let mut active = active.borrow_mut();
            if starts {
                active.insert(id);
            } else {
                active.remove(&id);
            }
            !active.is_empty()
        };
        if state.get_non_reactive() != any {
            state.set(any);
        }
    }

    /// Returns whether the interaction source is currently pressed as a
    /// reactive [`State`].
    ///
    /// Mirrors Jetpack Compose: `InteractionSource.collectIsPressedAsState()`.
    ///
    /// The value flips to `true` when a `PressInteraction::Press` is emitted
    /// and back to `false` once every active press has seen a matching
    /// `PressInteraction::Release` or `PressInteraction::Cancel`. Reading the
    /// returned state inside a composable subscribes the enclosing recompose
    /// scope, so the composable recomposes whenever the pressed state changes.
    pub fn collectIsPressedAsState(&self) -> State<bool> {
        self.inner().pressed.as_state()
    }

    /// Returns whether a pointer rests over a node of this source as a
    /// reactive [`State`]: `true` from a `HoverInteraction::Enter` until
    /// every enter has seen its `HoverInteraction::Exit`.
    ///
    /// Mirrors Jetpack Compose: `InteractionSource.collectIsHoveredAsState()`.
    pub fn collectIsHoveredAsState(&self) -> State<bool> {
        self.inner().hovered.as_state()
    }

    pub fn collectLastInteractionAsState(&self) -> State<Option<Interaction>> {
        self.inner().last_interaction.as_state()
    }
}

impl PressInteractionPress {
    pub fn id(&self) -> u64 {
        self.id
    }
}

impl HoverInteractionEnter {
    pub fn id(&self) -> u64 {
        self.id
    }
}

impl std::fmt::Debug for MutableInteractionSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MutableInteractionSource")
            .field("id", &self.id())
            .finish()
    }
}

impl PartialEq for MutableInteractionSource {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl Eq for MutableInteractionSource {}

impl Default for MutableInteractionSource {
    fn default() -> Self {
        Self::new()
    }
}

#[composable]
pub fn rememberMutableInteractionSource() -> MutableInteractionSource {
    let runtime = with_current_composer(|composer| composer.runtime_handle());
    remember(move || MutableInteractionSource::with_runtime(runtime)).with(|source| *source)
}

/// Free-function form of
/// [`MutableInteractionSource::collectIsPressedAsState`].
///
/// Mirrors Jetpack Compose: `InteractionSource.collectIsPressedAsState()`.
///
/// Returns a reactive [`State`] derived from the press interactions emitted
/// by `interaction_source`: `true` between `PressInteraction::Press` and the
/// matching `PressInteraction::Release`/`PressInteraction::Cancel`.
pub fn collect_is_pressed_as_state(interaction_source: &MutableInteractionSource) -> State<bool> {
    interaction_source.collectIsPressedAsState()
}

/// Free-function form of
/// [`MutableInteractionSource::collectIsHoveredAsState`].
///
/// Mirrors Jetpack Compose: `InteractionSource.collectIsHoveredAsState()`.
pub fn collect_is_hovered_as_state(interaction_source: &MutableInteractionSource) -> State<bool> {
    interaction_source.collectIsHoveredAsState()
}

impl Modifier {
    pub fn press_interaction_source(self, interaction_source: MutableInteractionSource) -> Self {
        self.interaction_node("pressInteractionSource", interaction_source, PressTracker)
    }

    /// Reports the pointer resting over this node to `interaction_source` as
    /// a `HoverInteraction::Enter`, and its leaving as the matching
    /// `HoverInteraction::Exit`, while `enabled`. Disabling the node or
    /// removing it ends a hover it reported. Compose's
    /// `Modifier.hoverable(interactionSource, enabled)`.
    ///
    /// Example: `let hovered = source.collectIsHoveredAsState();` and
    /// `Modifier::empty().hoverable(source, true)` on the row it tints.
    pub fn hoverable(self, interaction_source: MutableInteractionSource, enabled: bool) -> Self {
        self.interaction_node("hoverable", interaction_source, HoverTracker { enabled })
    }

    fn interaction_node<T: InteractionTracker>(
        self,
        name: &'static str,
        interaction_source: MutableInteractionSource,
        tracker: T,
    ) -> Self {
        let source_id = interaction_source.id();
        let modifier = Self::with_element(InteractionElement {
            interaction_source,
            tracker,
        })
        .with_inspector_metadata(inspector_metadata(name, move |info| {
            info.add_property("sourceId", source_id.to_string());
        }));
        self.then(modifier)
    }
}

/// What a pointer node reports to an interaction source: which pointer
/// events start and end the interaction it tracks, and how to end one
/// early.
trait InteractionTracker: Copy + PartialEq + std::fmt::Debug + Hash + 'static {
    /// The interaction the node holds while it lasts.
    type Active: Copy + 'static;

    /// Feeds the primary pointer's `event` to `source`, starting or ending
    /// the interaction `active` holds.
    fn handle(
        self,
        source: MutableInteractionSource,
        active: &RefCell<Option<Self::Active>>,
        event: &PointerEvent,
    );

    /// Ends `active` early: the node left, or stopped tracking.
    fn end(source: MutableInteractionSource, active: Self::Active);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct PressTracker;

impl InteractionTracker for PressTracker {
    type Active = PressInteractionPress;

    fn handle(
        self,
        source: MutableInteractionSource,
        active: &RefCell<Option<PressInteractionPress>>,
        event: &PointerEvent,
    ) {
        if event.is_consumed() {
            if let Some(press) = active.borrow_mut().take() {
                source.cancel(press);
            }
            return;
        }
        match event.kind {
            PointerEventKind::Down => {
                if active.borrow().is_none() {
                    *active.borrow_mut() = Some(source.press(event.position));
                }
            }
            PointerEventKind::Up => {
                if let Some(press) = active.borrow_mut().take() {
                    source.release(press);
                }
            }
            PointerEventKind::Cancel => {
                if let Some(press) = active.borrow_mut().take() {
                    source.cancel(press);
                }
            }
            PointerEventKind::Move
            | PointerEventKind::Scroll
            | PointerEventKind::Zoom
            | PointerEventKind::RotaryScrollPre
            | PointerEventKind::RotaryScroll
            | PointerEventKind::Enter
            | PointerEventKind::Exit => {}
        }
    }

    fn end(source: MutableInteractionSource, active: PressInteractionPress) {
        source.cancel(active);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct HoverTracker {
    enabled: bool,
}

impl InteractionTracker for HoverTracker {
    type Active = HoverInteractionEnter;

    fn handle(
        self,
        source: MutableInteractionSource,
        active: &RefCell<Option<HoverInteractionEnter>>,
        event: &PointerEvent,
    ) {
        match event.kind {
            PointerEventKind::Enter | PointerEventKind::Move
                if self.enabled && active.borrow().is_none() =>
            {
                *active.borrow_mut() = Some(source.enter_hover());
            }
            PointerEventKind::Exit => {
                if let Some(enter) = active.borrow_mut().take() {
                    source.exit_hover(enter);
                }
            }
            _ => {}
        }
    }

    fn end(source: MutableInteractionSource, active: HoverInteractionEnter) {
        source.exit_hover(active);
    }
}

#[derive(Clone)]
struct InteractionElement<T> {
    interaction_source: MutableInteractionSource,
    tracker: T,
}

impl<T: InteractionTracker> std::fmt::Debug for InteractionElement<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InteractionElement")
            .field("source_id", &self.interaction_source.id())
            .field("tracker", &self.tracker)
            .finish()
    }
}

impl<T: InteractionTracker> PartialEq for InteractionElement<T> {
    fn eq(&self, other: &Self) -> bool {
        self.interaction_source == other.interaction_source && self.tracker == other.tracker
    }
}

impl<T: InteractionTracker> Eq for InteractionElement<T> {}

impl<T: InteractionTracker> Hash for InteractionElement<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.tracker.hash(state);
        self.interaction_source.id().hash(state);
    }
}

impl<T: InteractionTracker> ModifierNodeElement for InteractionElement<T> {
    type Node = InteractionNode<T>;

    fn create(&self) -> Self::Node {
        let active = Rc::new(RefCell::new(None));
        InteractionNode {
            interaction_source: self.interaction_source,
            tracker: self.tracker,
            cached_handler: InteractionNode::handler(
                self.interaction_source,
                self.tracker,
                active.clone(),
            ),
            active,
            state: NodeState::new(),
        }
    }

    fn update(&self, node: &mut Self::Node) {
        node.update(self.interaction_source, self.tracker);
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::POINTER_INPUT
    }
}

struct InteractionNode<T: InteractionTracker> {
    interaction_source: MutableInteractionSource,
    tracker: T,
    active: Rc<RefCell<Option<T::Active>>>,
    cached_handler: Rc<dyn Fn(PointerEvent)>,
    state: NodeState,
}

impl<T: InteractionTracker> InteractionNode<T> {
    fn update(&mut self, interaction_source: MutableInteractionSource, tracker: T) {
        if self.interaction_source == interaction_source && self.tracker == tracker {
            return;
        }
        self.end_active();
        self.interaction_source = interaction_source;
        self.tracker = tracker;
        self.cached_handler = Self::handler(interaction_source, tracker, self.active.clone());
    }

    fn end_active(&self) {
        if let Some(active) = self.active.borrow_mut().take() {
            T::end(self.interaction_source, active);
        }
    }

    fn handler(
        interaction_source: MutableInteractionSource,
        tracker: T,
        active: Rc<RefCell<Option<T::Active>>>,
    ) -> Rc<dyn Fn(PointerEvent)> {
        Rc::new(move |event: PointerEvent| {
            if event.id == 0 {
                tracker.handle(interaction_source, &active, &event);
            }
        })
    }
}

impl<T: InteractionTracker> std::fmt::Debug for InteractionNode<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InteractionNode")
            .field("source_id", &self.interaction_source.id())
            .field("tracker", &self.tracker)
            .finish()
    }
}

impl<T: InteractionTracker> DelegatableNode for InteractionNode<T> {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl<T: InteractionTracker> ModifierNode for InteractionNode<T> {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        context.invalidate(InvalidationKind::PointerInput);
    }

    fn as_pointer_input_node(&self) -> Option<&dyn PointerInputNode> {
        Some(self)
    }

    fn as_pointer_input_node_mut(&mut self) -> Option<&mut dyn PointerInputNode> {
        Some(self)
    }

    fn on_detach(&mut self) {
        self.end_active();
    }
}

impl<T: InteractionTracker> PointerInputNode for InteractionNode<T> {
    fn pointer_input_handler(&self) -> Option<Rc<dyn Fn(PointerEvent)>> {
        Some(self.cached_handler.clone())
    }
}

#[cfg(test)]
#[path = "tests/interaction_tests.rs"]
mod tests;
