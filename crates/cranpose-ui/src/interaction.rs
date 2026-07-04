#![allow(non_snake_case)]

use crate::composable;
use crate::modifier::{inspector_metadata, Modifier, Point, PointerEvent, PointerEventKind};
use cranpose_core::{remember, with_current_composer, OwnedMutableState, RuntimeHandle, State};
use cranpose_foundation::{
    DelegatableNode, InvalidationKind, ModifierNode, ModifierNodeContext, ModifierNodeElement,
    NodeCapabilities, NodeState, PointerInputNode,
};
use std::cell::RefCell;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

#[derive(Clone)]
pub struct MutableInteractionSource {
    inner: Rc<MutableInteractionSourceInner>,
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
            inner: Rc::new(MutableInteractionSourceInner {
                next_press_id: RefCell::new(1),
                active_presses: RefCell::new(HashSet::new()),
                pressed: OwnedMutableState::with_runtime(false, runtime.clone()),
                last_interaction: OwnedMutableState::with_runtime(None, runtime),
            }),
        }
    }

    pub fn id(&self) -> u64 {
        Rc::as_ptr(&self.inner) as usize as u64
    }

    pub fn press(&self, press_position: Point) -> PressInteractionPress {
        let id = {
            let mut next_press_id = self.inner.next_press_id.borrow_mut();
            let id = *next_press_id;
            *next_press_id = next_press_id.saturating_add(1);
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
        self.inner.last_interaction.set(Some(interaction));
        let is_pressed = {
            let mut active_presses = self.inner.active_presses.borrow_mut();
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

        if self.inner.pressed.get_non_reactive() != is_pressed {
            self.inner.pressed.set(is_pressed);
        }
    }

    pub fn collectIsPressedAsState(&self) -> State<bool> {
        self.inner.pressed.as_state()
    }

    pub fn collectLastInteractionAsState(&self) -> State<Option<Interaction>> {
        self.inner.last_interaction.as_state()
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
        self.id() == other.id()
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
    remember(move || MutableInteractionSource::with_runtime(runtime.clone()))
        .with(|source| source.clone())
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
        PressInteractionNode::new(self.interaction_source.clone())
    }

    fn update(&self, node: &mut Self::Node) {
        node.update(self.interaction_source.clone());
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
        let cached_handler = Self::create_handler(interaction_source.clone(), active_press.clone());
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
            Self::create_handler(self.interaction_source.clone(), self.active_press.clone());
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
    use super::*;
    use cranpose_core::{Composition, MemoryApplier};

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
        let first_clone = first.clone();
        let second = MutableInteractionSource::with_runtime(composition.runtime_handle());

        assert_eq!(first.id(), first_clone.id());
        assert_ne!(first.id(), second.id());
        assert_eq!(first.press(Point { x: 0.0, y: 0.0 }).id(), 1);
        assert_eq!(first.press(Point { x: 1.0, y: 1.0 }).id(), 2);
        assert_eq!(second.press(Point { x: 0.0, y: 0.0 }).id(), 1);
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
