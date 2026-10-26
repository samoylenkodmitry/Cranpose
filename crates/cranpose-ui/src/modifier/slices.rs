use std::{borrow::Cow, cell::Ref, fmt, mem::size_of, ops::Deref, rc::Rc};

use cranpose_foundation::{PointerEvent, PointerEventKind};
use cranpose_ui_graphics::{
    ColorFilter, GraphicsLayer, LayerShape, PointerIcon, Rect, RenderEffect, RoundedCornerShape,
    Size,
};

mod snapshot;

pub(crate) use snapshot::SlicesSnapshot;
pub use snapshot::{collect_modifier_slices, collect_slices_from_modifier};

use super::{ModifierChainHandle, Point, coordinator_geometry::CoordinatorRect};
use crate::{
    draw::DrawCommand,
    text::{TextLayoutOptions, TextStyle},
    text_field_modifier_node::{TextFieldLayoutHandle, TextPanResolver},
    text_modifier_node::TextPreparedLayoutHandle,
};

/// Snapshot of modifier node slices that impact draw and pointer subsystems.
#[derive(Clone, Default)]
pub struct ModifierNodeSlices {
    draw_commands: Vec<DrawCommand>,
    layer_draw_boundary: Option<usize>,
    clip_to_bounds: bool,
    /// Where the outermost clip or graphics layer of the chain sits: its
    /// coordinator, which a later offset moves the content inside of.
    layer_coordinator: Option<CoordinatorRect>,
    text: Option<SliceText>,
    text_coordinator: Option<CoordinatorRect>,
    /// Boxed: few nodes carry a layer, and inline it took 240 bytes of
    /// every node's slices.
    graphics_layer: Option<Box<GraphicsLayer>>,
    corner_shape: Option<RoundedCornerShape>,
    /// Boxed for the same reason: inline they took 150 bytes of every
    /// node's slices, and on the gauntlet's 5,600 nodes none sets them but the
    /// 44 animated layers' resolvers.
    rare: Option<Box<RareSlices>>,
}

fn publish_pointer_input_size(rare: &RareSlices, size: Size) {
    for sink in &rare.pointer_input_sizes {
        sink.set(size);
    }
}

/// What few nodes' chains contribute: pointer input, window geometry sinks,
/// translated content and live layer resolvers.
#[derive(Clone, Default)]
struct RareSlices {
    pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    pointer_input_sizes: Vec<Rc<std::cell::Cell<Size>>>,
    pointer_icon: Option<PointerIcon>,
    motion_context_animated: bool,
    translated_content_context: bool,
    translated_content_context_identity: Option<usize>,
    translated_content_offset_reader: Option<Rc<dyn Fn() -> Point>>,
    text_window: Option<Rc<std::cell::Cell<cranpose_ui_graphics::WindowCoordinates>>>,
    viewport_window_rect: Option<Rc<dyn crate::modifier_nodes::WindowRectSink>>,
    graphics_layer_resolver: Option<Rc<dyn Fn() -> GraphicsLayer>>,
    chain_guard: Option<Rc<ChainGuard>>,
    z_index: f32,
}

impl RareSlices {
    fn clear(&mut self) {
        self.pointer_inputs.clear();
        self.pointer_input_sizes.clear();
        self.pointer_icon = None;
        self.motion_context_animated = false;
        self.translated_content_context = false;
        self.translated_content_context_identity = None;
        self.translated_content_offset_reader = None;
        self.text_window = None;
        self.viewport_window_rect = None;
        self.graphics_layer_resolver = None;
        self.chain_guard = None;
        self.z_index = 0.0;
    }
}

struct ChainGuard {
    _handle: ModifierChainHandle,
}

/// Where a draw modifier draws: in its coordinator's rect, which only moves
/// off the node's own rect when the chain has layout modifiers.
struct DrawSite {
    coordinator: CoordinatorRect,
    displaceable: bool,
    modifier_index: usize,
}

/// The text a node shows. A `Text`'s string, style and options are its
/// prepared layout's, read from there rather than copied into every
/// rebuild of the slices; a text field's are held here.
#[derive(Clone)]
enum SliceText {
    Text(TextPreparedLayoutHandle),
    Field(Rc<FieldText>),
}

/// The text, or a part of it, that a node's slices lend: a `Text`'s is
/// borrowed from its layout, which an update changes in place; a text
/// field's is held by the slices.
pub struct SliceTextRef<'a, T: ?Sized>(TextBorrow<'a, T>);

enum TextBorrow<'a, T: ?Sized> {
    Layout(Ref<'a, T>),
    Field(&'a T),
}

impl<'a, T: ?Sized> SliceTextRef<'a, T> {
    fn map<U: ?Sized>(self, part: impl FnOnce(&T) -> &U) -> SliceTextRef<'a, U> {
        SliceTextRef(match self.0 {
            TextBorrow::Layout(text) => TextBorrow::Layout(Ref::map(text, part)),
            TextBorrow::Field(text) => TextBorrow::Field(part(text)),
        })
    }
}

impl<T: ?Sized> Deref for SliceTextRef<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        match &self.0 {
            TextBorrow::Layout(text) => text,
            TextBorrow::Field(text) => text,
        }
    }
}

impl<T: ?Sized + fmt::Debug> fmt::Debug for SliceTextRef<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}

impl<T: ?Sized + fmt::Display> fmt::Display for SliceTextRef<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}

impl PartialEq<str> for SliceTextRef<'_, str> {
    fn eq(&self, other: &str) -> bool {
        **self == *other
    }
}

impl PartialEq<&str> for SliceTextRef<'_, str> {
    fn eq(&self, other: &&str) -> bool {
        **self == **other
    }
}

struct FieldText {
    content: Rc<crate::text::AnnotatedString>,
    style: TextStyle,
    layout: TextFieldLayoutHandle,
    pan: Option<TextPanResolver>,
}

/// Counts and capacities of the data that modifier node slices hold, for memory diagnostics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModifierNodeSlicesDebugStats {
    /// Number of draw commands.
    pub draw_command_count: usize,
    /// Allocated capacity of the draw command list, in commands.
    pub draw_command_capacity: usize,
    /// Number of pointer input handlers.
    pub pointer_input_count: usize,
    /// Allocated capacity of the pointer input handler list, in handlers.
    pub pointer_input_capacity: usize,
    /// Whether the node shows text.
    pub has_text_content: bool,
    /// Whether the node carries a text style.
    pub has_text_style: bool,
    /// Whether the node carries text layout options.
    pub has_text_layout_options: bool,
    /// Whether the node carries a prepared text layout.
    pub has_prepared_text_layout: bool,
    /// Whether the node carries a graphics layer.
    pub has_graphics_layer: bool,
    /// Whether the graphics layer is read through a resolver that runs when the layer is used.
    pub has_graphics_layer_resolver: bool,
    /// Heap bytes of the draw command and pointer input handler lists, counted by capacity.
    pub heap_bytes: usize,
}

fn merge_graphics_layers(base: GraphicsLayer, overlay: GraphicsLayer) -> GraphicsLayer {
    GraphicsLayer {
        alpha: (base.alpha * overlay.alpha).clamp(0.0, 1.0),
        scale: base.scale * overlay.scale,
        scale_x: base.scale_x * overlay.scale_x,
        scale_y: base.scale_y * overlay.scale_y,
        rotation_x: base.rotation_x + overlay.rotation_x,
        rotation_y: base.rotation_y + overlay.rotation_y,
        rotation_z: base.rotation_z + overlay.rotation_z,
        camera_distance: transform_frame(&base, &overlay).camera_distance,
        transform_origin: transform_frame(&base, &overlay).transform_origin,
        translation_x: base.translation_x + overlay.translation_x,
        translation_y: base.translation_y + overlay.translation_y,
        shadow_elevation: overlay.shadow_elevation,
        ambient_shadow_color: overlay.ambient_shadow_color,
        spot_shadow_color: overlay.spot_shadow_color,
        shape: merged_layer_shape(&base, &overlay),
        clip: base.clip || overlay.clip,
        compositing_strategy: overlay.compositing_strategy,
        blend_mode: overlay.blend_mode,
        color_filter: compose_color_filters(base.color_filter, overlay.color_filter),
        render_effect: compose_render_effects(base.render_effect, overlay.render_effect),
        backdrop_effect: overlay.backdrop_effect.or(base.backdrop_effect),
        space: overlay.space.or(base.space),
    }
}

/// Which of two stacked layers merged into one frames the merged transform:
/// the later layer when it rotates or scales itself, else the earlier one. A
/// layer that neither rotates nor scales, such as a glass surface under a
/// tilt, leaves its camera and pivot at their defaults, and those would
/// replace the tilt's own.
fn transform_frame<'a>(base: &'a GraphicsLayer, overlay: &'a GraphicsLayer) -> &'a GraphicsLayer {
    let transforms = |layer: &GraphicsLayer| {
        layer.rotation_x != 0.0
            || layer.rotation_y != 0.0
            || layer.rotation_z != 0.0
            || layer.scale != 1.0
            || layer.scale_x != 1.0
            || layer.scale_y != 1.0
    };
    if transforms(overlay) || !transforms(base) {
        overlay
    } else {
        base
    }
}

/// The shape of two stacked layers merged into one: the layer that clips
/// owns it, else the layer that carries the backdrop (its effect covers that
/// shape), else the later layer, whose default resets it like every other
/// parent-local field.
fn merged_layer_shape(base: &GraphicsLayer, overlay: &GraphicsLayer) -> LayerShape {
    if overlay.clip || (!base.clip && overlay.backdrop_effect.is_some()) {
        overlay.shape
    } else if base.clip || base.backdrop_effect.is_some() {
        base.shape
    } else {
        overlay.shape
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
        (Some(filter), Some(next)) => Some(filter.compose(&next)),
    }
}

impl ModifierNodeSlices {
    /// Returns the node's draw commands in the order they draw.
    pub fn draw_commands(&self) -> &[DrawCommand] {
        &self.draw_commands
    }

    /// How many leading [`draw_commands`](Self::draw_commands) come from
    /// modifiers chained before the node's graphics layer or clip. Those draw
    /// around the layer in the parent's space, outside its clip, alpha and
    /// transform, exactly as an outer `drawBehind` wraps a `graphicsLayer`.
    pub fn outer_draw_command_count(&self) -> usize {
        self.layer_draw_boundary.unwrap_or(0)
    }

    fn insert_background_draw(
        &mut self,
        insert_index: usize,
        precedes_layer: bool,
        command: DrawCommand,
    ) {
        let insert_index = insert_index.min(self.draw_commands.len());
        self.draw_commands.insert(insert_index, command);
        if let Some(boundary) = self.layer_draw_boundary.as_mut()
            && precedes_layer
        {
            *boundary += 1;
        }
    }

    fn mark_layer_draw_boundary(&mut self) {
        if self.layer_draw_boundary.is_none() {
            self.layer_draw_boundary = Some(self.draw_commands.len());
        }
    }

    /// The boxed fields when the chain set any; see [`RareSlices`].
    fn rare(&self) -> Option<&RareSlices> {
        self.rare.as_deref()
    }

    fn rare_mut(&mut self) -> &mut RareSlices {
        self.rare.get_or_insert_with(Box::default)
    }

    /// Returns the node's pointer input handlers in chain order.
    pub fn pointer_inputs(&self) -> &[Rc<dyn Fn(PointerEvent)>] {
        self.rare().map_or(&[], |rare| &rare.pointer_inputs)
    }

    /// Dispatches an event whose position is already local to this layout node.
    /// Consumed events stop propagation except for release and cancellation,
    /// which reach every handler so each can finish its active interaction.
    pub fn dispatch_pointer_event(&self, event: PointerEvent) {
        let terminal = matches!(event.kind, PointerEventKind::Up | PointerEventKind::Cancel);
        for handler in self.pointer_inputs() {
            if event.is_consumed() && !terminal {
                break;
            }
            handler(event.clone());
        }
    }

    /// The write targets for this node's resolved size, one per pointer-input
    /// node that exposes a size to its handler. See
    /// [`ModifierNodeSlices::publish_pointer_input_size`].
    pub fn pointer_input_size_sinks(&self) -> &[Rc<std::cell::Cell<Size>>] {
        self.rare().map_or(&[], |rare| &rare.pointer_input_sizes)
    }

    /// Publishes this layout node's resolved size to every pointer-input
    /// handler attached to it, so `PointerInputScope::size()` reports the
    /// node's real dimensions.
    ///
    /// Called by every pass that resolves a node's geometry (the layout `place`
    /// passes and the per-frame scene build), so the size is current before any
    /// pointer event for that frame is dispatched and tracks resizes.
    ///
    /// `size` is the node's layout box — the same box the dispatched
    /// [`PointerEvent`] positions are made local to — so handlers can compare
    /// event coordinates against it directly.
    pub fn publish_pointer_input_size(&self, size: Size) {
        if let Some(rare) = self.rare() {
            publish_pointer_input_size(rare, size);
        }
    }

    /// Where the node draws among its siblings: the sum of its `z_index`
    /// modifiers.
    pub fn z_index(&self) -> f32 {
        self.rare().map_or(0.0, |rare| rare.z_index)
    }

    /// The pointer's appearance over this node, when a `pointer_icon`
    /// modifier names one. The innermost declaration in the chain wins.
    pub fn pointer_icon(&self) -> Option<&PointerIcon> {
        self.rare()?.pointer_icon.as_ref()
    }

    /// Returns whether the node clips its content to its bounds.
    pub fn clip_to_bounds(&self) -> bool {
        self.clip_to_bounds
    }

    /// The bounds of the node's layer in a node of `node_size`, where its
    /// clip cuts and its transforms pivot: as in Compose, the rect of the
    /// coordinator its outermost clip or graphics layer wraps, so an offset
    /// declared after it moves the content inside the layer rather than the
    /// layer. The node's own rect when it has neither.
    pub fn layer_bounds(&self, node_size: Size) -> Rect {
        self.layer_coordinator
            .as_ref()
            .map_or_else(|| Rect::from_size(node_size), |layer| layer.rect(node_size))
    }

    /// Publishes geometry in window coordinates through the same transform as
    /// rendering. Pointer-input sizes stay in the node's local coordinates.
    #[doc(hidden)]
    pub fn publish_window_geometry(
        &self,
        origin: Point,
        transform: cranpose_ui_graphics::ProjectiveTransform,
        size: Size,
    ) {
        // Every sink is a rare slice: most nodes have none to publish to.
        let Some(rare) = self.rare() else {
            return;
        };
        if rare.text_window.is_some() || rare.viewport_window_rect.is_some() {
            let window = cranpose_ui_graphics::WindowCoordinates {
                size,
                local_to_window: cranpose_ui_graphics::ProjectiveTransform::translation(
                    origin.x, origin.y,
                )
                .then(transform),
            };
            if let Some(sink) = &rare.text_window {
                sink.set(window);
            }
            if let Some(sink) = &rare.viewport_window_rect {
                sink.set(window);
            }
        }
        publish_pointer_input_size(rare, size);
    }

    /// Returns whether a scroll container on this node is moving its content, for example in a
    /// fling or a settle.
    pub fn motion_context_animated(&self) -> bool {
        self.rare().is_some_and(|rare| rare.motion_context_animated)
    }

    /// Returns whether this node is a scroll container that draws its content translated by
    /// [`Self::translated_content_offset`].
    pub fn translated_content_context(&self) -> bool {
        self.rare()
            .is_some_and(|rare| rare.translated_content_context)
    }

    /// Returns an identifier of that scroll container that stays the same while it lives, or `None`
    /// when the node is not one.
    pub fn translated_content_context_identity(&self) -> Option<usize> {
        self.rare()?.translated_content_context_identity
    }

    /// Returns the offset in logical pixels by which the scroll container translates its content
    /// now, or `None` when the node has no such offset.
    pub fn translated_content_offset(&self) -> Option<Point> {
        self.rare()?
            .translated_content_offset_reader
            .as_ref()
            .map(|reader| reader())
    }

    /// Whether the node shows a `Text`'s or a text field's text.
    pub fn has_text(&self) -> bool {
        self.text.is_some()
    }

    /// Returns the plain text the node shows, or `None` when the node shows no text.
    pub fn text_content(&self) -> Option<SliceTextRef<'_, str>> {
        self.annotated_text()
            .map(|text| text.map(|text| text.text.as_str()))
    }

    /// Returns the text the node shows with its span styles, or `None` when the node shows no text.
    pub fn annotated_text(&self) -> Option<SliceTextRef<'_, Rc<crate::text::AnnotatedString>>> {
        Some(SliceTextRef(match self.text.as_ref()? {
            SliceText::Text(layout) => TextBorrow::Layout(layout.annotated_text()),
            SliceText::Field(field) => TextBorrow::Field(&field.content),
        }))
    }

    /// Where the text draws in a node of `node_size`: the rect its layout put
    /// the text in, after every layout modifier before it.
    pub fn text_content_rect(&self, node_size: Size) -> Rect {
        self.text_coordinator.as_ref().map_or(
            Rect {
                x: 0.0,
                y: 0.0,
                width: node_size.width,
                height: node_size.height,
            },
            |coordinator| coordinator.rect(node_size),
        )
    }

    /// Returns the style of the text the node shows, or `None` when the node shows no text.
    pub fn text_style(&self) -> Option<SliceTextRef<'_, TextStyle>> {
        Some(SliceTextRef(match self.text.as_ref()? {
            SliceText::Text(layout) => TextBorrow::Layout(layout.style()),
            SliceText::Field(field) => TextBorrow::Field(&field.style),
        }))
    }

    /// Returns the layout options of the text the node shows, or `None` when the node shows no
    /// text. A text field reports the default options.
    pub fn text_layout_options(&self) -> Option<TextLayoutOptions> {
        match self.text.as_ref()? {
            SliceText::Text(layout) => Some(layout.options()),
            SliceText::Field(_) => Some(TextLayoutOptions::default()),
        }
    }

    /// Returns the horizontal pan resolver for single-line text fields.
    ///
    /// The resolver takes the content viewport width (px) and returns the
    /// horizontal scroll offset that keeps the cursor visible. Renderers
    /// subtract this offset from the text origin so the glyphs pan together
    /// with the cursor and selection.
    pub fn text_pan_resolver(&self) -> Option<TextPanResolver> {
        match self.text.as_ref()? {
            SliceText::Field(field) => field.pan.clone(),
            SliceText::Text(_) => None,
        }
    }

    /// The write target for a text node's size and local-to-window
    /// transform, used by editable and selectable text for caret, selection
    /// and pointer geometry and a focused field's window bounds.
    pub fn text_window(
        &self,
    ) -> Option<&Rc<std::cell::Cell<cranpose_ui_graphics::WindowCoordinates>>> {
        self.rare()?.text_window.as_ref()
    }

    /// The write target for a scroll container's composited window rect, if this
    /// node carries a `report_window_rect` modifier. The layout pass writes the
    /// node's true on-screen viewport rect here so a `BringIntoViewResponder`
    /// can scroll a focused field's caret above the soft keyboard.
    pub fn viewport_window_rect(&self) -> Option<&Rc<dyn crate::modifier_nodes::WindowRectSink>> {
        self.rare()?.viewport_window_rect.as_ref()
    }

    /// Returns the text layout this node's `Text` or text field produced when
    /// layout last measured it, laid out at the same width.
    ///
    /// A text field's layout is its current text wrapped at the width its
    /// caret and selection are placed on. `None` when the node carries no text,
    /// or carries a `Text` that has not been measured yet.
    pub fn measured_text_layout(&self) -> Option<Rc<crate::text::PreparedTextLayout>> {
        match self.text.as_ref()? {
            SliceText::Text(layout) => layout.measured_layout(),
            SliceText::Field(field) => Some(field.layout.measured_layout(&field.style)),
        }
    }

    /// Returns the graphics layer that the layer modifiers of the chain combine into, or `None`
    /// when the chain has none: lent when the chain holds it, built when a layer block computes
    /// it.
    #[inline]
    pub fn graphics_layer(&self) -> Option<Cow<'_, GraphicsLayer>> {
        if let Some(resolve) = self
            .rare()
            .and_then(|rare| rare.graphics_layer_resolver.as_ref())
        {
            Some(Cow::Owned(resolve()))
        } else {
            self.graphics_layer.as_deref().map(Cow::Borrowed)
        }
    }

    /// Returns the rounded corner shape the chain sets, or `None` when it sets none.
    pub fn corner_shape(&self) -> Option<RoundedCornerShape> {
        self.corner_shape
    }

    /// Records the layer's coordinator when this is the chain's outermost
    /// clip or graphics layer.
    fn enter_layer(&mut self, site: &DrawSite) {
        if self.layer_coordinator.is_none() {
            self.layer_coordinator = Some(site.coordinator.clone());
        }
    }

    fn push_graphics_layer(
        &mut self,
        layer: GraphicsLayer,
        resolver: Option<Rc<dyn Fn() -> GraphicsLayer>>,
    ) {
        let existing_snapshot = self.graphics_layer.as_deref().cloned();
        let next_snapshot = existing_snapshot.as_ref().map_or_else(
            || layer.clone(),
            |current| merge_graphics_layers(current.clone(), layer.clone()),
        );
        let existing_resolver = self
            .rare()
            .and_then(|rare| rare.graphics_layer_resolver.clone());

        match &mut self.graphics_layer {
            Some(held) => **held = next_snapshot,
            None => self.graphics_layer = Some(Box::new(next_snapshot)),
        }
        let merged_resolver: Option<Rc<dyn Fn() -> GraphicsLayer>> =
            match (existing_resolver, resolver) {
                (None, None) => None,
                (Some(current_resolver), None) => Some(Rc::new(move || {
                    merge_graphics_layers(current_resolver(), layer.clone())
                })),
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
        if merged_resolver.is_some() {
            self.rare_mut().graphics_layer_resolver = merged_resolver;
        }
    }

    /// Returns these slices holding `handle`, which keeps the node chain they come from alive.
    pub fn with_chain_guard(mut self, handle: ModifierChainHandle) -> Self {
        self.rare_mut().chain_guard = Some(Rc::new(ChainGuard { _handle: handle }));
        self
    }

    /// Returns counts and capacities of the slices' data, for memory diagnostics.
    pub fn debug_stats(&self) -> ModifierNodeSlicesDebugStats {
        let draw_command_bytes = self.draw_commands.capacity() * size_of::<DrawCommand>();
        let pointer_input_capacity = self.rare().map_or(0, |rare| rare.pointer_inputs.capacity());
        let pointer_input_bytes = pointer_input_capacity * size_of::<Rc<dyn Fn(PointerEvent)>>();
        ModifierNodeSlicesDebugStats {
            draw_command_count: self.draw_commands.len(),
            draw_command_capacity: self.draw_commands.capacity(),
            pointer_input_count: self.pointer_inputs().len(),
            pointer_input_capacity,
            has_text_content: self.text.is_some(),
            has_text_style: self.text.is_some(),
            has_text_layout_options: self.text.is_some(),
            has_prepared_text_layout: self.text.is_some(),
            has_graphics_layer: self.graphics_layer.is_some(),
            has_graphics_layer_resolver: self
                .rare()
                .is_some_and(|rare| rare.graphics_layer_resolver.is_some()),
            heap_bytes: draw_command_bytes + pointer_input_bytes,
        }
    }

    /// Resets the slice collection for reuse, retaining vector capacity.
    pub fn clear(&mut self) {
        self.draw_commands.clear();
        self.layer_draw_boundary = None;
        self.clip_to_bounds = false;
        self.layer_coordinator = None;
        self.text = None;
        self.text_coordinator = None;
        self.graphics_layer = None;
        self.corner_shape = None;
        if let Some(rare) = self.rare.as_deref_mut() {
            rare.clear();
        }
    }
}

impl fmt::Debug for ModifierNodeSlices {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModifierNodeSlices")
            .field("draw_commands", &self.draw_commands.len())
            .field("pointer_inputs", &self.pointer_inputs().len())
            .field("pointer_icon", &self.pointer_icon())
            .field("clip_to_bounds", &self.clip_to_bounds)
            .field("motion_context_animated", &self.motion_context_animated())
            .field(
                "translated_content_context",
                &self.translated_content_context(),
            )
            .field(
                "translated_content_context_identity",
                &self.translated_content_context_identity(),
            )
            .field(
                "translated_content_offset",
                &self.translated_content_offset(),
            )
            .field("text_content", &self.annotated_text())
            .field("text_style", &self.text_style())
            .field("text_layout_options", &self.text_layout_options())
            .field("graphics_layer", &self.graphics_layer)
            .field(
                "graphics_layer_resolver",
                &self
                    .rare()
                    .is_some_and(|rare| rare.graphics_layer_resolver.is_some()),
            )
            .field("corner_shape", &self.corner_shape)
            .finish()
    }
}

#[cfg(test)]
#[path = "tests/slices_tests.rs"]
mod tests;
