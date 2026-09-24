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
    next_press_id: RefCell<u64>,
    active_presses: RefCell<HashSet<u64>>,
    pressed: OwnedMutableState<bool>,
    last_interaction: OwnedMutableState<Option<Interaction>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Interaction {
    Press(PressInteraction),
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
                    next_press_id: RefCell::new(1),
                    active_presses: RefCell::new(HashSet::new()),
                    pressed: OwnedMutableState::with_runtime(false, runtime.clone()),
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

    pub fn press(&self, press_position: Point) -> PressInteractionPress {
        let inner = self.inner();
        let id = {
            let mut next_press_id = inner.next_press_id.borrow_mut();
            let id = *next_press_id;
            *next_press_id = id.saturating_add(1);
            id
        };
        let press = PressInteractionPress { id, press_position };
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

    pub fn emit(&self, interaction: Interaction) {
        let inner = self.inner();
        inner.last_interaction.set(Some(interaction));
        let is_pressed = {
            let mut active_presses = inner.active_presses.borrow_mut();
            match interaction {
                Interaction::Press(PressInteraction::Press(press)) => {
                    active_presses.insert(press.id);
                }
                Interaction::Press(PressInteraction::Release(release)) => {
                    active_presses.remove(&release.press.id);
                }
                Interaction::Press(PressInteraction::Cancel(cancel)) => {
                    active_presses.remove(&cancel.press.id);
                }
            }
            !active_presses.is_empty()
        };

        if inner.pressed.get_non_reactive() != is_pressed {
            inner.pressed.set(is_pressed);
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

    pub fn collectLastInteractionAsState(&self) -> State<Option<Interaction>> {
        self.inner().last_interaction.as_state()
    }
}

impl PressInteractionPress {
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

impl Modifier {
    pub fn press_interaction_source(self, interaction_source: MutableInteractionSource) -> Self {
        let source_id = interaction_source.id();
        let modifier = Self::with_element(PressInteractionElement::new(interaction_source))
            .with_inspector_metadata(inspector_metadata("pressInteractionSource", move |info| {
                info.add_property("sourceId", source_id.to_string());
            }));
        self.then(modifier)
    }
}

#[derive(Clone)]
struct PressInteractionElement {
    interaction_source: MutableInteractionSource,
}

impl PressInteractionElement {
    fn new(interaction_source: MutableInteractionSource) -> Self {
        Self { interaction_source }
    }
}

impl std::fmt::Debug for PressInteractionElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PressInteractionElement")
            .field("source_id", &self.interaction_source.id())
            .finish()
    }
}

impl PartialEq for PressInteractionElement {
    fn eq(&self, other: &Self) -> bool {
        self.interaction_source == other.interaction_source
    }
}

impl Eq for PressInteractionElement {}

impl Hash for PressInteractionElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "pressInteractionSource".hash(state);
        self.interaction_source.id().hash(state);
    }
}

impl ModifierNodeElement for PressInteractionElement {
    type Node = PressInteractionNode;

    fn create(&self) -> Self::Node {
        PressInteractionNode::new(self.interaction_source)
    }

    fn update(&self, node: &mut Self::Node) {
        node.update(self.interaction_source);
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::POINTER_INPUT
    }
}

struct PressInteractionNode {
    interaction_source: MutableInteractionSource,
    active_press: Rc<RefCell<Option<PressInteractionPress>>>,
    cached_handler: Rc<dyn Fn(PointerEvent)>,
    state: NodeState,
}

impl PressInteractionNode {
    fn new(interaction_source: MutableInteractionSource) -> Self {
        let active_press = Rc::new(RefCell::new(None));
        let cached_handler = Self::create_handler(interaction_source, active_press.clone());
        Self {
            interaction_source,
            active_press,
            cached_handler,
            state: NodeState::new(),
        }
    }

    fn update(&mut self, interaction_source: MutableInteractionSource) {
        if self.interaction_source == interaction_source {
            return;
        }
        if let Some(press) = self.active_press.borrow_mut().take() {
            self.interaction_source.cancel(press);
        }
        self.interaction_source = interaction_source;
        self.cached_handler =
            Self::create_handler(self.interaction_source, self.active_press.clone());
    }

    fn create_handler(
        interaction_source: MutableInteractionSource,
        active_press: Rc<RefCell<Option<PressInteractionPress>>>,
    ) -> Rc<dyn Fn(PointerEvent)> {
        Rc::new(move |event: PointerEvent| {
            if event.id != 0 {
                return;
            }

            if event.is_consumed() {
                if let Some(press) = active_press.borrow_mut().take() {
                    interaction_source.cancel(press);
                }
                return;
            }

            match event.kind {
                PointerEventKind::Down => {
                    if active_press.borrow().is_none() {
                        let press = interaction_source.press(event.position);
                        *active_press.borrow_mut() = Some(press);
                    }
                }
                PointerEventKind::Up => {
                    if let Some(press) = active_press.borrow_mut().take() {
                        interaction_source.release(press);
                    }
                }
                PointerEventKind::Cancel => {
                    if let Some(press) = active_press.borrow_mut().take() {
                        interaction_source.cancel(press);
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
        })
    }
}

impl std::fmt::Debug for PressInteractionNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PressInteractionNode")
            .field("source_id", &self.interaction_source.id())
            .finish()
    }
}

impl DelegatableNode for PressInteractionNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for PressInteractionNode {
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
        if let Some(press) = self.active_press.borrow_mut().take() {
            self.interaction_source.cancel(press);
        }
    }
}

impl PointerInputNode for PressInteractionNode {
    fn pointer_input_handler(&self) -> Option<Rc<dyn Fn(PointerEvent)>> {
        Some(self.cached_handler.clone())
    }
}

#[cfg(test)]
#[path = "tests/interaction_tests.rs"]
mod tests;
