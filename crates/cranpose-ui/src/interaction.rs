#![allow(non_snake_case)]

use std::{
    cell::RefCell,
    collections::HashSet,
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_core::{
    remember, with_current_composer, MutableState, OwnedMutableState, RuntimeHandle, State,
};
use cranpose_foundation::{
    DelegatableNode, InvalidationKind, ModifierNode, ModifierNodeContext, ModifierNodeElement,
    NodeCapabilities, NodeState, PointerInputNode,
};

use crate::{
    composable,
    modifier::{inspector_metadata, Modifier, Point, PointerEvent, PointerEventKind},
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
        let runtime = with_current_composer(|composer| composer.runtime_handle());
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
    remember(move || MutableInteractionSource::with_runtime(runtime.clone())).with(|source| *source)
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
            // Press interactions track the primary pointer only.
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
mod tests {
    use cranpose_core::{Composition, MemoryApplier};

    use super::*;

    #[test]
    fn interaction_ids_do_not_use_process_global_counters() {
        let source = include_str!("interaction.rs");
        let source_counter = ["static ", "NEXT_SOURCE_ID"].concat();
        let press_counter = ["static ", "NEXT_PRESS_ID"].concat();

        assert!(
            !source.contains(&source_counter) && !source.contains(&press_counter),
            "interaction source and press ids must be owned by the interaction source instance"
        );
    }

    #[test]
    fn interaction_source_tracks_active_press_state() {
        let composition = Composition::new(MemoryApplier::new());
        let source = MutableInteractionSource::with_runtime(composition.runtime_handle());
        let pressed = source.collectIsPressedAsState();

        assert!(!pressed.get());

        let first = source.press(Point { x: 1.0, y: 2.0 });
        assert!(pressed.get());

        let second = source.press(Point { x: 3.0, y: 4.0 });
        assert_ne!(first.id(), second.id());
        source.release(first);
        assert!(pressed.get());

        source.cancel(second);
        assert!(!pressed.get());
    }

    #[test]
    fn interaction_source_ids_are_instance_owned() {
        let composition = Composition::new(MemoryApplier::new());
        let first = MutableInteractionSource::with_runtime(composition.runtime_handle());
        let first_clone = first;
        let second = MutableInteractionSource::with_runtime(composition.runtime_handle());

        assert_eq!(first.id(), first_clone.id());
        assert_ne!(first.id(), second.id());
        assert_eq!(first.press(Point { x: 0.0, y: 0.0 }).id(), 1);
        assert_eq!(first.press(Point { x: 1.0, y: 1.0 }).id(), 2);
        assert_eq!(second.press(Point { x: 0.0, y: 0.0 }).id(), 1);
    }

    #[test]
    fn collect_is_pressed_as_state_tracks_press_emissions() {
        let composition = Composition::new(MemoryApplier::new());
        let source = MutableInteractionSource::with_runtime(composition.runtime_handle());
        let pressed = collect_is_pressed_as_state(&source);

        assert!(!pressed.get());

        let press = PressInteractionPress {
            id: 7,
            press_position: Point { x: 4.0, y: 5.0 },
        };
        source.emit(Interaction::Press(PressInteraction::Press(press)));
        assert!(pressed.get(), "pressed after a Press emission");

        source.emit(Interaction::Press(PressInteraction::Release(
            PressInteractionRelease { press },
        )));
        assert!(!pressed.get(), "released after a Release emission");

        source.emit(Interaction::Press(PressInteraction::Press(press)));
        assert!(pressed.get(), "pressed again after a new Press emission");

        source.emit(Interaction::Press(PressInteraction::Cancel(
            PressInteractionCancel { press },
        )));
        assert!(!pressed.get(), "released after a Cancel emission");
    }

    #[composable]
    fn PressedReader(
        observed: Rc<RefCell<Vec<bool>>>,
        source_slot: Rc<RefCell<Option<MutableInteractionSource>>>,
    ) {
        let source = rememberMutableInteractionSource();
        source_slot.borrow_mut().replace(source);
        let pressed = collect_is_pressed_as_state(&source);
        observed.borrow_mut().push(pressed.value());
    }

    #[test]
    fn collect_is_pressed_as_state_recomposes_readers() {
        let observed = Rc::new(RefCell::new(Vec::<bool>::new()));
        let source_slot = Rc::new(RefCell::new(None::<MutableInteractionSource>));

        let mut composition = {
            let observed = Rc::clone(&observed);
            let source_slot = Rc::clone(&source_slot);
            crate::run_test_composition(move || {
                PressedReader(Rc::clone(&observed), Rc::clone(&source_slot));
            })
        };

        assert_eq!(observed.borrow().as_slice(), &[false]);

        let source = *source_slot
            .borrow()
            .as_ref()
            .expect("interaction source captured");
        let press = composition.with_app_context(|| source.press(Point { x: 1.0, y: 1.0 }));
        while composition
            .process_invalid_scopes()
            .expect("process press invalidation")
        {}
        assert_eq!(
            observed.borrow().last(),
            Some(&true),
            "press emission should recompose readers with pressed=true"
        );

        composition.with_app_context(|| source.release(press));
        while composition
            .process_invalid_scopes()
            .expect("process release invalidation")
        {}
        assert_eq!(
            observed.borrow().last(),
            Some(&false),
            "release emission should recompose readers with pressed=false"
        );
    }

    #[test]
    fn interaction_source_exposes_latest_interaction() {
        let composition = Composition::new(MemoryApplier::new());
        let source = MutableInteractionSource::with_runtime(composition.runtime_handle());
        let last_interaction = source.collectLastInteractionAsState();

        assert_eq!(last_interaction.get(), None);

        let press = source.press(Point { x: 8.0, y: 12.0 });
        assert_eq!(
            last_interaction.get(),
            Some(Interaction::Press(PressInteraction::Press(press)))
        );
        assert_eq!(press.press_position, Point { x: 8.0, y: 12.0 });

        source.release(press);
        assert_eq!(
            last_interaction.get(),
            Some(Interaction::Press(PressInteraction::Release(
                PressInteractionRelease { press }
            )))
        );
    }
}
