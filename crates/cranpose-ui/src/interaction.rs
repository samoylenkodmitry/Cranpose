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
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone)]
pub struct MutableInteractionSource {
    inner: Rc<MutableInteractionSourceInner>,
}

struct MutableInteractionSourceInner {
    id: u64,
    active_presses: RefCell<HashSet<u64>>,
    pressed: OwnedMutableState<bool>,
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
        static NEXT_SOURCE_ID: AtomicU64 = AtomicU64::new(1);
        Self {
            inner: Rc::new(MutableInteractionSourceInner {
                id: NEXT_SOURCE_ID.fetch_add(1, Ordering::Relaxed),
                active_presses: RefCell::new(HashSet::new()),
                pressed: OwnedMutableState::with_runtime(false, runtime),
            }),
        }
    }

    pub fn id(&self) -> u64 {
        self.inner.id
    }

    pub fn press(&self, press_position: Point) -> PressInteractionPress {
        static NEXT_PRESS_ID: AtomicU64 = AtomicU64::new(1);
        let press = PressInteractionPress {
            id: NEXT_PRESS_ID.fetch_add(1, Ordering::Relaxed),
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

    pub fn emit(&self, interaction: Interaction) {
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
    fn interaction_source_tracks_active_press_state() {
        let composition = Composition::new(MemoryApplier::new());
        let source = MutableInteractionSource::with_runtime(composition.runtime_handle());
        let pressed = source.collectIsPressedAsState();

        assert!(!pressed.get());

        let first = source.press(Point { x: 1.0, y: 2.0 });
        assert!(pressed.get());

        let second = source.press(Point { x: 3.0, y: 4.0 });
        source.release(first);
        assert!(pressed.get());

        source.cancel(second);
        assert!(!pressed.get());
    }
}
