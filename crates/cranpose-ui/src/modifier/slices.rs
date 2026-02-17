use std::fmt;
use std::rc::Rc;

use cranpose_foundation::{ModifierNodeChain, NodeCapabilities, PointerEvent};
use cranpose_ui_graphics::{ColorFilter, GraphicsLayer, RenderEffect};

use crate::draw::DrawCommand;
use crate::modifier::Modifier;
use crate::modifier_nodes::{
    BackgroundNode, ClipToBoundsNode, CornerShapeNode, DrawCommandNode, GraphicsLayerNode,
    PaddingNode,
};
use crate::text::{TextLayoutOptions, TextStyle};
use crate::text_field_modifier_node::TextFieldModifierNode;
use crate::text_modifier_node::TextModifierNode;
use cranpose_ui_graphics::EdgeInsets;
use std::cell::RefCell;

use super::{ModifierChainHandle, Point};

/// Snapshot of modifier node slices that impact draw and pointer subsystems.
#[derive(Default)]
pub struct ModifierNodeSlices {
    draw_commands: Vec<DrawCommand>,
    pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    click_handlers: Vec<Rc<dyn Fn(Point)>>,
    clip_to_bounds: bool,
    text_content: Option<Rc<str>>,
    text_style: Option<TextStyle>,
    text_layout_options: Option<TextLayoutOptions>,
    graphics_layer: Option<GraphicsLayer>,
    graphics_layer_resolver: Option<Rc<dyn Fn() -> GraphicsLayer>>,
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
            text_style: self.text_style.clone(),
            text_layout_options: self.text_layout_options,
            graphics_layer: self.graphics_layer.clone(),
            graphics_layer_resolver: self.graphics_layer_resolver.clone(),
            chain_guard: self.chain_guard.clone(),
        }
    }
}

/// Compose two nested graphics layers (`base` outer, `overlay` inner) into one
/// flattened layer snapshot used by render pipelines.
///
/// Composition rules follow how nested transforms/effects behave visually:
/// - Multiplicative: `alpha`, `scale`, `scale_x`, `scale_y`
/// - Additive: `rotation_*`, `translation_*`
/// - Boolean OR: `clip`
/// - Overlay-wins when explicitly set: camera distance, transform origin,
///   shadow elevation/colors, shape, compositing strategy, blend mode
/// - Filters/effects are composed in draw order:
///   - color filters are multiplied where possible
///   - render effects chain inner-first then outer (`inner.then(outer)`)
/// - Backdrop effect keeps the most local explicit backdrop effect because
///   backdrop sampling cannot be safely flattened as a deterministic chain.
fn merge_graphics_layers(base: GraphicsLayer, overlay: GraphicsLayer) -> GraphicsLayer {
    GraphicsLayer {
        alpha: (base.alpha * overlay.alpha).clamp(0.0, 1.0),
        scale: base.scale * overlay.scale,
        scale_x: base.scale_x * overlay.scale_x,
        scale_y: base.scale_y * overlay.scale_y,
        rotation_x: base.rotation_x + overlay.rotation_x,
        rotation_y: base.rotation_y + overlay.rotation_y,
        rotation_z: base.rotation_z + overlay.rotation_z,
        camera_distance: overlay.camera_distance,
        transform_origin: overlay.transform_origin,
        translation_x: base.translation_x + overlay.translation_x,
        translation_y: base.translation_y + overlay.translation_y,
        shadow_elevation: overlay.shadow_elevation,
        ambient_shadow_color: overlay.ambient_shadow_color,
        spot_shadow_color: overlay.spot_shadow_color,
        shape: overlay.shape,
        clip: base.clip || overlay.clip,
        compositing_strategy: overlay.compositing_strategy,
        blend_mode: overlay.blend_mode,
        color_filter: compose_color_filters(base.color_filter, overlay.color_filter),
        // Modifiers are traversed outer -> inner. Layer effects therefore compose
        // inner-first, then outer, matching nested layer rendering semantics.
        render_effect: compose_render_effects(base.render_effect, overlay.render_effect),
        // Backdrop effects cannot be represented as a deterministic chain on a
        // flattened single layer, so keep the most local explicit backdrop.
        backdrop_effect: overlay.backdrop_effect.or(base.backdrop_effect),
    }
}

fn compose_render_effects(
    outer: Option<RenderEffect>,
    inner: Option<RenderEffect>,
) -> Option<RenderEffect> {
    match (outer, inner) {
        (None, None) => None,
        (Some(effect), None) | (None, Some(effect)) => Some(effect),
        (Some(outer_effect), Some(inner_effect)) => Some(inner_effect.then(outer_effect)),
    }
}

fn compose_color_filters(
    base: Option<ColorFilter>,
    overlay: Option<ColorFilter>,
) -> Option<ColorFilter> {
    match (base, overlay) {
        (None, None) => None,
        (Some(filter), None) | (None, Some(filter)) => Some(filter),
        (Some(filter), Some(next)) => Some(filter.compose(next)),
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

    pub fn text_content_rc(&self) -> Option<Rc<str>> {
        self.text_content.clone()
    }

    pub fn text_style(&self) -> Option<&TextStyle> {
        self.text_style.as_ref()
    }

    pub fn text_layout_options(&self) -> Option<TextLayoutOptions> {
        self.text_layout_options
    }

    pub fn graphics_layer(&self) -> Option<GraphicsLayer> {
        if let Some(resolve) = &self.graphics_layer_resolver {
            Some(resolve())
        } else {
            self.graphics_layer.clone()
        }
    }

    fn push_graphics_layer(
        &mut self,
        layer: GraphicsLayer,
        resolver: Option<Rc<dyn Fn() -> GraphicsLayer>>,
    ) {
        let existing_snapshot = self.graphics_layer();
        let next_snapshot = existing_snapshot
            .as_ref()
            .map(|current| merge_graphics_layers(current.clone(), layer.clone()))
            .unwrap_or_else(|| layer.clone());
        let existing_resolver = self.graphics_layer_resolver.clone();

        self.graphics_layer = Some(next_snapshot);
        self.graphics_layer_resolver = match (existing_resolver, resolver) {
            (None, None) => None,
            (Some(current_resolver), None) => {
                let layer = layer.clone();
                Some(Rc::new(move || {
                    merge_graphics_layers(current_resolver(), layer.clone())
                }))
            }
            (None, Some(next_resolver)) => {
                let base = existing_snapshot.unwrap_or_default();
                Some(Rc::new(move || {
                    merge_graphics_layers(base.clone(), next_resolver())
                }))
            }
            (Some(current_resolver), Some(next_resolver)) => Some(Rc::new(move || {
                merge_graphics_layers(current_resolver(), next_resolver())
            })),
        };
    }

    pub fn with_chain_guard(mut self, handle: ModifierChainHandle) -> Self {
        self.chain_guard = Some(Rc::new(ChainGuard { _handle: handle }));
        self
    }

    /// Resets the slice collection for reuse, retaining vector capacity.
    pub fn clear(&mut self) {
        self.draw_commands.clear();
        self.pointer_inputs.clear();
        self.click_handlers.clear();
        self.clip_to_bounds = false;
        self.text_content = None;
        self.text_style = None;
        self.text_layout_options = None;
        self.graphics_layer = None;
        self.graphics_layer_resolver = None;
        self.chain_guard = None;
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
            .field("text_style", &self.text_style)
            .field("text_layout_options", &self.text_layout_options)
            .field("graphics_layer", &self.graphics_layer)
            .field(
                "graphics_layer_resolver",
                &self.graphics_layer_resolver.is_some(),
            )
            .finish()
    }
}

/// Collects modifier node slices directly from a reconciled [`ModifierNodeChain`].
pub fn collect_modifier_slices(chain: &ModifierNodeChain) -> ModifierNodeSlices {
    let mut slices = ModifierNodeSlices::default();
    collect_modifier_slices_into(chain, &mut slices);
    slices
}

/// Collects modifier node slices into an existing buffer to reuse allocations.
pub fn collect_modifier_slices_into(chain: &ModifierNodeChain, slices: &mut ModifierNodeSlices) {
    slices.clear();

    chain.for_each_node_with_capability(NodeCapabilities::POINTER_INPUT, |_ref, node| {
        let _any = node.as_any();

        // ClickableNode is now handled as a standard PointerInputNode
        // to support drag cancellation and proper click semantics (Up vs Down)

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
    let background_insert_index = RefCell::new(None::<usize>);
    let corner_shape = RefCell::new(None);

    chain.for_each_node_with_capability(NodeCapabilities::DRAW, |_ref, node| {
        let any = node.as_any();

        // Collect background color from BackgroundNode
        if let Some(bg_node) = any.downcast_ref::<BackgroundNode>() {
            *background_color.borrow_mut() = Some(bg_node.color());
            *background_insert_index.borrow_mut() = Some(slices.draw_commands.len());
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

        // Use create_draw_closure() for nodes with dynamic content (cursor blink, selection)
        // This defers evaluation to render time, enabling live updates.
        // Fallback to draw() for nodes with static content.
        if let Some(draw_node) = node.as_draw_node() {
            if let Some(closure) = draw_node.create_draw_closure() {
                // Deferred closure - evaluates at render time
                slices.draw_commands.push(DrawCommand::Overlay(closure));
            } else {
                // Static draw - evaluate now
                use cranpose_ui_graphics::{DrawScope as _, DrawScopeDefault};
                let mut scope = DrawScopeDefault::new(crate::modifier::Size {
                    width: 0.0,
                    height: 0.0,
                });
                draw_node.draw(&mut scope);
                let primitives = scope.into_primitives();
                if !primitives.is_empty() {
                    let draw_cmd = Rc::new(move |_size: crate::modifier::Size| primitives.clone());
                    slices.draw_commands.push(DrawCommand::Overlay(draw_cmd));
                }
            }
        }

        // Collect graphics layer from GraphicsLayerNode
        if let Some(layer_node) = any.downcast_ref::<GraphicsLayerNode>() {
            slices.push_graphics_layer(layer_node.layer(), layer_node.layer_resolver());
        }

        if any.is::<ClipToBoundsNode>() {
            slices.clip_to_bounds = true;
        }
    });

    // Collect padding from modifier chain for cursor positioning
    let mut padding = EdgeInsets::default();
    chain.for_each_node_with_capability(NodeCapabilities::LAYOUT, |_ref, node| {
        let any = node.as_any();
        if let Some(padding_node) = any.downcast_ref::<PaddingNode>() {
            let p = padding_node.padding();
            padding.left += p.left;
            padding.top += p.top;
            padding.right += p.right;
            padding.bottom += p.bottom;
        }
    });

    // Collect text content from TextModifierNode or TextFieldModifierNode (LAYOUT capability)
    chain.for_each_node_with_capability(NodeCapabilities::LAYOUT, |_ref, node| {
        let any = node.as_any();
        if let Some(text_node) = any.downcast_ref::<TextModifierNode>() {
            // Rightmost text modifier wins
            slices.text_content = Some(text_node.text_arc());
            slices.text_style = Some(text_node.style().clone());
            slices.text_layout_options = Some(text_node.options());
        }
        // Also check for TextFieldModifierNode (editable text fields)
        if let Some(text_field_node) = any.downcast_ref::<TextFieldModifierNode>() {
            let text = text_field_node.text();
            slices.text_content = Some(Rc::from(text));
            slices.text_layout_options = Some(TextLayoutOptions::default());

            // Update content offsets for cursor positioning in collect_draw_primitives()
            text_field_node.set_content_offset(padding.left);
            text_field_node.set_content_y_offset(padding.top);

            // Cursor/selection rendering is now handled via DrawModifierNode::collect_draw_primitives()
            // in the DRAW capability loop above
        }
    });

    // Convert background + shape into a draw command
    if let Some(color) = background_color.into_inner() {
        let shape = corner_shape.into_inner();

        let draw_cmd = Rc::new(move |size: crate::modifier::Size| {
            use crate::modifier::{Brush, Rect};
            use cranpose_ui_graphics::DrawPrimitive;

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

        let insert_index = background_insert_index
            .into_inner()
            .unwrap_or(0)
            .min(slices.draw_commands.len());
        slices
            .draw_commands
            .insert(insert_index, DrawCommand::Behind(draw_cmd));
    }
}

/// Collects modifier node slices by instantiating a temporary node chain from a [`Modifier`].
pub fn collect_slices_from_modifier(modifier: &Modifier) -> ModifierNodeSlices {
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(modifier);
    collect_modifier_slices(handle.chain()).with_chain_guard(handle)
}
