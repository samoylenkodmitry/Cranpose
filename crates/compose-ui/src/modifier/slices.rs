use std::fmt;
use std::rc::Rc;

use compose_foundation::{ModifierNodeChain, NodeCapabilities, PointerEvent};

use crate::draw::DrawCommand;
use crate::modifier::Modifier;
use crate::modifier_nodes::{ClickableNode, ClipToBoundsNode, DrawCommandNode};

use super::{ModifierChainHandle, Point};

/// Snapshot of modifier node slices that impact draw and pointer subsystems.
#[derive(Default)]
pub struct ModifierNodeSlices {
    draw_commands: Vec<DrawCommand>,
    pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    click_handlers: Vec<Rc<dyn Fn(Point)>>,
    clip_to_bounds: bool,
    chain_guard: Option<Rc<ChainGuard>>,
}

struct ChainGuard {
    _handle: ModifierChainHandle,
}

impl Clone for ModifierNodeSlices {
    fn clone(&self) -> Self {
        Self {
            draw_commands: self.draw_commands.clone(),
            pointer_inputs: self.pointer_inputs.clone(),
            click_handlers: self.click_handlers.clone(),
            clip_to_bounds: self.clip_to_bounds,
            chain_guard: self.chain_guard.clone(),
        }
    }
}

impl ModifierNodeSlices {
    pub fn draw_commands(&self) -> &[DrawCommand] {
        &self.draw_commands
    }

    pub fn pointer_inputs(&self) -> &[Rc<dyn Fn(PointerEvent)>] {
        &self.pointer_inputs
    }

    pub fn click_handlers(&self) -> &[Rc<dyn Fn(Point)>] {
        &self.click_handlers
    }

    pub fn clip_to_bounds(&self) -> bool {
        self.clip_to_bounds
    }

    fn extend(&mut self, other: ModifierNodeSlices) {
        self.draw_commands.extend(other.draw_commands);
        self.pointer_inputs.extend(other.pointer_inputs.into_iter());
        self.click_handlers.extend(other.click_handlers.into_iter());
        self.clip_to_bounds |= other.clip_to_bounds;
        if self.chain_guard.is_none() {
            self.chain_guard = other.chain_guard;
        }
    }

    pub fn with_chain_guard(mut self, handle: ModifierChainHandle) -> Self {
        self.chain_guard = Some(Rc::new(ChainGuard { _handle: handle }));
        self
    }
}

impl fmt::Debug for ModifierNodeSlices {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModifierNodeSlices")
            .field("draw_commands", &self.draw_commands.len())
            .field("pointer_inputs", &self.pointer_inputs.len())
            .field("click_handlers", &self.click_handlers.len())
            .field("clip_to_bounds", &self.clip_to_bounds)
            .finish()
    }
}

/// Collects modifier node slices directly from a reconciled [`ModifierNodeChain`].
pub fn collect_modifier_slices(chain: &ModifierNodeChain) -> ModifierNodeSlices {
    let mut slices = ModifierNodeSlices::default();

    chain.for_each_node_with_capability(NodeCapabilities::POINTER_INPUT, |_ref, node| {
        if let Some(handler) = node
            .as_pointer_input_node()
            .and_then(|n| n.pointer_input_handler())
        {
            slices.pointer_inputs.push(handler);
        }
    });

    chain.for_each_node_with_capability(NodeCapabilities::DRAW, |_ref, node| {
        let any = node.as_any();
        if let Some(commands) = any.downcast_ref::<DrawCommandNode>() {
            slices
                .draw_commands
                .extend(commands.commands().iter().cloned());
        }
        if any.is::<ClipToBoundsNode>() {
            slices.clip_to_bounds = true;
        }
    });

    slices
}

/// Collects modifier node slices by instantiating a temporary node chain from a [`Modifier`].
pub fn collect_slices_from_modifier(modifier: &Modifier) -> ModifierNodeSlices {
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(modifier);
    collect_modifier_slices(handle.chain()).with_chain_guard(handle)
}
