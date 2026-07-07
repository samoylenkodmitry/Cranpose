use std::fmt;
use std::mem::size_of;
use std::rc::Rc;

use cranpose_foundation::{ModifierNodeChain, NodeCapabilities, PointerEvent};
use cranpose_ui_graphics::{ColorFilter, GraphicsLayer, RenderEffect, RoundedCornerShape};

use super::{ModifierChainHandle, Point};
use crate::draw::DrawCommand;
use crate::modifier::scroll::{MotionContextAnimatedNode, TranslatedContentContextNode};
use crate::modifier::Modifier;
use crate::modifier_nodes::{
    BackgroundNode, ClipToBoundsNode, CornerShapeNode, DrawCommandNode, GraphicsLayerNode,
    PaddingNode,
};
use crate::text::{TextLayoutOptions, TextStyle};
use crate::text_field_modifier_node::{TextFieldModifierNode, TextPanResolver};
use crate::text_modifier_node::{TextModifierNode, TextPreparedLayoutHandle};
use cranpose_ui_graphics::EdgeInsets;

/// Snapshot of modifier node slices that impact draw and pointer subsystems.
#[derive(Default)]
pub struct ModifierNodeSlices {
    draw_commands: Vec<DrawCommand>,
    pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    click_handlers: Vec<Rc<dyn Fn(Point)>>,
    clip_to_bounds: bool,
    motion_context_animated: bool,
    translated_content_context: bool,
    translated_content_context_identity: Option<usize>,
    translated_content_offset_reader: Option<Rc<dyn Fn() -> Point>>,
    text_content: Option<Rc<crate::text::AnnotatedString>>,
    text_style: Option<TextStyle>,
    text_layout_options: Option<TextLayoutOptions>,
    prepared_text_layout: Option<TextPreparedLayoutHandle>,
    text_pan: Option<TextPanResolver>,
    /// Write target for a text field's composited window origin. The layout
    /// `place` pass writes the field node's true on-screen top-left here (window
    /// coordinates, resolved through ancestor scroll placement + graphics-layer
    /// translation) so the field's finger selection handles follow the field as
    /// a `LazyColumn`/`vertical_scroll` scrolls. `None` for non-text-field nodes.
    text_field_window_origin: Option<Rc<std::cell::Cell<Point>>>,
    graphics_layer: Option<GraphicsLayer>,
    graphics_layer_resolver: Option<Rc<dyn Fn() -> GraphicsLayer>>,
    corner_shape: Option<RoundedCornerShape>,
    chain_guard: Option<Rc<ChainGuard>>,
}

struct ChainGuard {
    _handle: ModifierChainHandle,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModifierNodeSlicesDebugStats {
    pub draw_command_count: usize,
    pub draw_command_capacity: usize,
    pub pointer_input_count: usize,
    pub pointer_input_capacity: usize,
    pub click_handler_count: usize,
    pub click_handler_capacity: usize,
    pub has_text_content: bool,
    pub has_text_style: bool,
    pub has_text_layout_options: bool,
    pub has_prepared_text_layout: bool,
    pub has_graphics_layer: bool,
    pub has_graphics_layer_resolver: bool,
    pub heap_bytes: usize,
}

impl Clone for ModifierNodeSlices {
    fn clone(&self) -> Self {
        Self {
            draw_commands: self.draw_commands.clone(),
            pointer_inputs: self.pointer_inputs.clone(),
            click_handlers: self.click_handlers.clone(),
            clip_to_bounds: self.clip_to_bounds,
            motion_context_animated: self.motion_context_animated,
            translated_content_context: self.translated_content_context,
            translated_content_context_identity: self.translated_content_context_identity,
            translated_content_offset_reader: self.translated_content_offset_reader.clone(),
            text_content: self.text_content.clone(),
            text_style: self.text_style.clone(),
            text_layout_options: self.text_layout_options,
            prepared_text_layout: self.prepared_text_layout.clone(),
            text_pan: self.text_pan.clone(),
            text_field_window_origin: self.text_field_window_origin.clone(),
            graphics_layer: self.graphics_layer.clone(),
            graphics_layer_resolver: self.graphics_layer_resolver.clone(),
            corner_shape: self.corner_shape,
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

    pub fn motion_context_animated(&self) -> bool {
        self.motion_context_animated
    }

    pub fn translated_content_context(&self) -> bool {
        self.translated_content_context
    }

    pub fn translated_content_context_identity(&self) -> Option<usize> {
        self.translated_content_context_identity
    }

    pub fn translated_content_offset(&self) -> Option<Point> {
        self.translated_content_offset_reader
            .as_ref()
            .map(|reader| reader())
    }

    pub fn text_content(&self) -> Option<&str> {
        self.text_content.as_ref().map(|a| a.text.as_str())
    }

    pub fn annotated_text(&self) -> Option<&crate::text::AnnotatedString> {
        self.text_content.as_deref()
    }

    pub fn annotated_string(&self) -> Option<crate::text::AnnotatedString> {
        self.annotated_text().cloned()
    }

    pub fn text_style(&self) -> Option<&TextStyle> {
        self.text_style.as_ref()
    }

    pub fn text_layout_options(&self) -> Option<TextLayoutOptions> {
        self.text_layout_options
    }

    /// Returns the horizontal pan resolver for single-line text fields.
    ///
    /// The resolver takes the content viewport width (px) and returns the
    /// horizontal scroll offset that keeps the cursor visible. Renderers
    /// subtract this offset from the text origin so the glyphs pan together
    /// with the cursor and selection.
    pub fn text_pan_resolver(&self) -> Option<TextPanResolver> {
        self.text_pan.clone()
    }

    /// The write target for a text field's composited window origin, if this
    /// node is a `BasicTextField`. The layout pass writes the field's true
    /// on-screen top-left here so its finger selection handles track the field
    /// across scroll. See [`ModifierNodeSlices::text_field_window_origin`].
    pub fn text_field_window_origin(&self) -> Option<Rc<std::cell::Cell<Point>>> {
        self.text_field_window_origin.clone()
    }

    pub fn prepare_text_layout(
        &self,
        max_width: Option<f32>,
    ) -> Option<crate::text::PreparedTextLayout> {
        if let Some(handle) = &self.prepared_text_layout {
            return Some(handle.prepare(max_width));
        }

        let text = self.annotated_text()?;
        let style = self.text_style.clone().unwrap_or_default();
        Some(crate::text::prepare_text_layout(
            text,
            &style,
            self.text_layout_options.unwrap_or_default(),
            max_width,
        ))
    }

    pub fn graphics_layer(&self) -> Option<GraphicsLayer> {
        if let Some(resolve) = &self.graphics_layer_resolver {
            Some(resolve())
        } else {
            self.graphics_layer.clone()
        }
    }

    pub fn corner_shape(&self) -> Option<RoundedCornerShape> {
        self.corner_shape
    }

    fn push_graphics_layer(
        &mut self,
        layer: GraphicsLayer,
        resolver: Option<Rc<dyn Fn() -> GraphicsLayer>>,
    ) {
        let existing_snapshot = self.graphics_layer.clone();
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

    pub fn debug_stats(&self) -> ModifierNodeSlicesDebugStats {
        let draw_command_bytes = self.draw_commands.capacity() * size_of::<DrawCommand>();
        let pointer_input_bytes =
            self.pointer_inputs.capacity() * size_of::<Rc<dyn Fn(PointerEvent)>>();
        let click_handler_bytes = self.click_handlers.capacity() * size_of::<Rc<dyn Fn(Point)>>();
        ModifierNodeSlicesDebugStats {
            draw_command_count: self.draw_commands.len(),
            draw_command_capacity: self.draw_commands.capacity(),
            pointer_input_count: self.pointer_inputs.len(),
            pointer_input_capacity: self.pointer_inputs.capacity(),
            click_handler_count: self.click_handlers.len(),
            click_handler_capacity: self.click_handlers.capacity(),
            has_text_content: self.text_content.is_some(),
            has_text_style: self.text_style.is_some(),
            has_text_layout_options: self.text_layout_options.is_some(),
            has_prepared_text_layout: self.prepared_text_layout.is_some(),
            has_graphics_layer: self.graphics_layer.is_some(),
            has_graphics_layer_resolver: self.graphics_layer_resolver.is_some(),
            heap_bytes: draw_command_bytes + pointer_input_bytes + click_handler_bytes,
        }
    }

    /// Resets the slice collection for reuse, retaining vector capacity.
    pub fn clear(&mut self) {
        self.draw_commands.clear();
        self.pointer_inputs.clear();
        self.click_handlers.clear();
        self.clip_to_bounds = false;
        self.motion_context_animated = false;
        self.translated_content_context = false;
        self.translated_content_context_identity = None;
        self.translated_content_offset_reader = None;
        self.text_content = None;
        self.text_style = None;
        self.text_layout_options = None;
        self.prepared_text_layout = None;
        self.text_pan = None;
        self.graphics_layer = None;
        self.graphics_layer_resolver = None;
        self.corner_shape = None;
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
            .field("motion_context_animated", &self.motion_context_animated)
            .field(
                "translated_content_context",
                &self.translated_content_context,
            )
            .field(
                "translated_content_context_identity",
                &self.translated_content_context_identity,
            )
            .field(
                "translated_content_offset",
                &self.translated_content_offset(),
            )
            .field("text_content", &self.text_content)
            .field("text_style", &self.text_style)
            .field("text_layout_options", &self.text_layout_options)
            .field("prepared_text_layout", &self.prepared_text_layout.is_some())
            .field("graphics_layer", &self.graphics_layer)
            .field(
                "graphics_layer_resolver",
                &self.graphics_layer_resolver.is_some(),
            )
            .field("corner_shape", &self.corner_shape)
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
///
/// Single-pass: iterates the chain once instead of 4 separate capability-filtered
/// traversals, reducing per-node `RefCell::borrow()` overhead.
pub fn collect_modifier_slices_into(chain: &ModifierNodeChain, slices: &mut ModifierNodeSlices) {
    slices.clear();

    let caps = chain.capabilities();
    let has_pointer = caps.intersects(NodeCapabilities::POINTER_INPUT);
    let has_draw = caps.intersects(NodeCapabilities::DRAW);
    let has_layout = caps.intersects(NodeCapabilities::LAYOUT);

    if !has_pointer && !has_draw && !has_layout {
        return;
    }

    let mut background_color = None;
    let mut background_insert_index = None::<usize>;
    let mut corner_shape = None;
    let mut padding = EdgeInsets::default();

    for node_ref in chain.head_to_tail() {
        let node_caps = node_ref.kind_set();

        node_ref.with_node(|node| {
            let any = node.as_any();

            // POINTER_INPUT collection
            if has_pointer && node_caps.intersects(NodeCapabilities::POINTER_INPUT) {
                if let Some(handler) = node
                    .as_pointer_input_node()
                    .and_then(|n| n.pointer_input_handler())
                {
                    slices.pointer_inputs.push(handler);
                }
            }

            // DRAW collection
            if has_draw && node_caps.intersects(NodeCapabilities::DRAW) {
                if let Some(bg_node) = any.downcast_ref::<BackgroundNode>() {
                    background_color = Some(bg_node.color());
                    background_insert_index = Some(slices.draw_commands.len());
                    if bg_node.shape().is_some() {
                        corner_shape = bg_node.shape();
                    }
                }

                if let Some(shape_node) = any.downcast_ref::<CornerShapeNode>() {
                    corner_shape = Some(shape_node.shape());
                }

                if let Some(commands) = any.downcast_ref::<DrawCommandNode>() {
                    slices.draw_commands.extend(commands.observed_commands());
                }

                if let Some(draw_node) = node.as_draw_node() {
                    if let Some(closure) = draw_node.create_draw_closure() {
                        slices.draw_commands.push(DrawCommand::Overlay(closure));
                    } else {
                        use cranpose_ui_graphics::{DrawScope as _, DrawScopeDefault};
                        let mut scope = DrawScopeDefault::new(crate::modifier::Size {
                            width: 0.0,
                            height: 0.0,
                        });
                        draw_node.draw(&mut scope);
                        let primitives = scope.into_primitives();
                        if !primitives.is_empty() {
                            let draw_cmd =
                                Rc::new(move |_size: crate::modifier::Size| primitives.clone());
                            slices.draw_commands.push(DrawCommand::Overlay(draw_cmd));
                        }
                    }
                }

                if let Some(layer_node) = any.downcast_ref::<GraphicsLayerNode>() {
                    slices.push_graphics_layer(
                        layer_node.layer_snapshot(),
                        layer_node.layer_resolver(),
                    );
                }

                if any.is::<ClipToBoundsNode>() {
                    slices.clip_to_bounds = true;
                }
            }

            // LAYOUT collection (padding + text)
            if has_layout && node_caps.intersects(NodeCapabilities::LAYOUT) {
                if let Some(padding_node) = any.downcast_ref::<PaddingNode>() {
                    let p = padding_node.padding();
                    padding.left += p.left;
                    padding.top += p.top;
                    padding.right += p.right;
                    padding.bottom += p.bottom;
                }

                if let Some(motion_context_node) = any.downcast_ref::<MotionContextAnimatedNode>() {
                    slices.motion_context_animated = motion_context_node.is_active();
                }

                if let Some(translated_content_node) =
                    any.downcast_ref::<TranslatedContentContextNode>()
                {
                    slices.translated_content_context = translated_content_node.is_active();
                    slices.translated_content_context_identity =
                        Some(translated_content_node.identity());
                    slices.translated_content_offset_reader =
                        translated_content_node.content_offset_reader();
                }

                if let Some(text_node) = any.downcast_ref::<TextModifierNode>() {
                    slices.text_content = Some(text_node.annotated_text());
                    slices.text_style = Some(text_node.style().clone());
                    slices.text_layout_options = Some(text_node.options());
                    slices.prepared_text_layout = Some(text_node.prepared_layout_handle());
                }

                if let Some(text_field_node) = any.downcast_ref::<TextFieldModifierNode>() {
                    let text = text_field_node.text();
                    slices.text_content = Some(Rc::new(crate::text::AnnotatedString::from(text)));
                    slices.text_style = Some(text_field_node.style().clone());
                    slices.text_layout_options = Some(TextLayoutOptions::default());
                    slices.prepared_text_layout = None;
                    slices.text_pan = text_field_node.text_pan_resolver();
                    // Give the layout pass a handle to write the field's true
                    // composited window origin into, so its finger selection
                    // handles follow the field as an ancestor list scrolls.
                    slices.text_field_window_origin = Some(text_field_node.window_origin_sink());

                    text_field_node.set_content_offset(padding.left);
                    text_field_node.set_content_y_offset(padding.top);
                }
            }
        });
    }

    slices.corner_shape = corner_shape;

    // Convert background + shape into a draw command
    if let Some(color) = background_color {
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

            if let Some(shape) = corner_shape {
                let radii = shape.resolve(size.width, size.height);
                vec![DrawPrimitive::RoundRect { rect, brush, radii }]
            } else {
                vec![DrawPrimitive::Rect { rect, brush }]
            }
        });

        let insert_index = background_insert_index
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
