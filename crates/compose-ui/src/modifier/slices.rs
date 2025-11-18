use std::fmt;
use std::rc::Rc;

use compose_foundation::{ModifierNodeChain, NodeCapabilities, PointerEvent};
use compose_ui_graphics::GraphicsLayer;

use crate::draw::DrawCommand;
use crate::modifier::{Brush, Modifier};
use crate::modifier_nodes::{BackgroundNode, ClickableNode, ClipToBoundsNode, CornerShapeNode, DrawCommandNode, GraphicsLayerNode};
use crate::text_modifier_node::TextModifierNode;
use std::cell::RefCell;

use super::{ModifierChainHandle, Point};

/// Snapshot of modifier node slices that impact draw and pointer subsystems.
#[derive(Default)]
pub struct ModifierNodeSlices {
    draw_commands: Vec<DrawCommand>,
    pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    click_handlers: Vec<Rc<dyn Fn(Point)>>,
    clip_to_bounds: bool,
    text_content: Option<String>,
    graphics_layer: Option<GraphicsLayer>,
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
            text_content: self.text_content.clone(),
            graphics_layer: self.graphics_layer,
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

    pub fn text_content(&self) -> Option<&str> {
        self.text_content.as_deref()
    }

    pub fn graphics_layer(&self) -> Option<GraphicsLayer> {
        self.graphics_layer
    }

    fn extend(&mut self, other: ModifierNodeSlices) {
        self.draw_commands.extend(other.draw_commands);
        self.pointer_inputs.extend(other.pointer_inputs.into_iter());
        self.click_handlers.extend(other.click_handlers.into_iter());
        self.clip_to_bounds |= other.clip_to_bounds;
        // Take the last text content (rightmost modifier wins)
        if other.text_content.is_some() {
            self.text_content = other.text_content;
        }
        // Take the last graphics layer (rightmost modifier wins)
        if other.graphics_layer.is_some() {
            self.graphics_layer = other.graphics_layer;
        }
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
            .field("text_content", &self.text_content)
            .field("graphics_layer", &self.graphics_layer)
            .finish()
    }
}

/// Collects modifier node slices directly from a reconciled [`ModifierNodeChain`].
pub fn collect_modifier_slices(chain: &ModifierNodeChain) -> ModifierNodeSlices {
    let mut slices = ModifierNodeSlices::default();

    chain.for_each_node_with_capability(NodeCapabilities::POINTER_INPUT, |_ref, node| {
        let any = node.as_any();

        // Collect click handlers from ClickableNode
        if let Some(clickable) = any.downcast_ref::<ClickableNode>() {
            slices.click_handlers.push(clickable.handler());
            // Skip adding to pointer_inputs to avoid duplicate invocation
            return;
        }

        // Collect general pointer input handlers (non-clickable)
        if let Some(handler) = node
            .as_pointer_input_node()
            .and_then(|n| n.pointer_input_handler())
        {
            slices.pointer_inputs.push(handler);
        }
    });

    // Track background and shape to combine them in draw commands
    let background_color = RefCell::new(None);
    let corner_shape = RefCell::new(None);

    chain.for_each_node_with_capability(NodeCapabilities::DRAW, |_ref, node| {
        let any = node.as_any();

        // Collect background color from BackgroundNode
        if let Some(bg_node) = any.downcast_ref::<BackgroundNode>() {
            *background_color.borrow_mut() = Some(bg_node.color());
            // Note: BackgroundNode can have an optional shape, but we primarily track
            // shape via CornerShapeNode for flexibility
            if bg_node.shape().is_some() {
                *corner_shape.borrow_mut() = bg_node.shape();
            }
        }

        // Collect corner shape from CornerShapeNode
        if let Some(shape_node) = any.downcast_ref::<CornerShapeNode>() {
            *corner_shape.borrow_mut() = Some(shape_node.shape());
        }

        // Collect draw commands from DrawCommandNode
        if let Some(commands) = any.downcast_ref::<DrawCommandNode>() {
            slices
                .draw_commands
                .extend(commands.commands().iter().cloned());
        }

        // Collect graphics layer from GraphicsLayerNode
        if let Some(layer_node) = any.downcast_ref::<GraphicsLayerNode>() {
            slices.graphics_layer = Some(layer_node.layer());
        }

        if any.is::<ClipToBoundsNode>() {
            slices.clip_to_bounds = true;
        }
    });

    // Collect text content from TextModifierNode (LAYOUT capability, not DRAW)
    chain.for_each_node_with_capability(NodeCapabilities::LAYOUT, |_ref, node| {
        let any = node.as_any();
        if let Some(text_node) = any.downcast_ref::<TextModifierNode>() {
            // Rightmost text modifier wins
            slices.text_content = Some(text_node.text().to_string());
        }
    });

    // Convert background + shape into a draw command
    if let Some(color) = background_color.into_inner() {
        let shape = corner_shape.into_inner();

        let draw_cmd = Rc::new(move |size: crate::modifier::Size| {
            use compose_ui_graphics::DrawPrimitive;
            use crate::modifier::{Brush, Rect};

            let brush = Brush::solid(color);
            let rect = Rect {
                x: 0.0,
                y: 0.0,
                width: size.width,
                height: size.height,
            };

            if let Some(shape) = shape {
                let radii = shape.resolve(size.width, size.height);
                vec![DrawPrimitive::RoundRect { rect, brush, radii }]
            } else {
                vec![DrawPrimitive::Rect { rect, brush }]
            }
        });

        slices.draw_commands.insert(0, DrawCommand::Behind(draw_cmd));
    }

    slices
}

/// Collects modifier node slices by instantiating a temporary node chain from a [`Modifier`].
pub fn collect_slices_from_modifier(modifier: &Modifier) -> ModifierNodeSlices {
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(modifier);
    collect_modifier_slices(handle.chain()).with_chain_guard(handle)
}
