use std::{cell::Ref, fmt, mem::size_of, ops::Deref, rc::Rc};

use cranpose_foundation::{ModifierNodeChain, NodeCapabilities, PointerEvent, PointerEventKind};
use cranpose_ui_graphics::{
    ColorFilter, EdgeInsets, GraphicsLayer, LayerShape, PointerIcon, Rect, RenderEffect,
    RoundedCornerShape, Size,
};
use smallvec::SmallVec;

use super::{
    ModifierChainHandle, Point,
    coordinator_geometry::{CoordinatorGeometry, CoordinatorRect},
};
use crate::{
    draw::DrawCommand,
    modifier::{
        Modifier,
        scroll::{MotionContextAnimatedNode, TranslatedContentContextNode},
    },
    modifier_nodes::{
        BackgroundNode, ClipToBoundsNode, CornerShapeNode, DrawCommandNode, GraphicsLayerNode,
        PaddingNode, PointerIconNode, SelectableTextNode, WindowRectReporterNode,
    },
    text::{TextLayoutOptions, TextStyle},
    text_field_modifier_node::{TextFieldLayoutHandle, TextFieldModifierNode, TextPanResolver},
    text_modifier_node::{TextModifierNode, TextPreparedLayoutHandle},
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

fn publish_pointer_input_size(rare: &RareSlices, size: cranpose_ui_graphics::Size) {
    for sink in &rare.pointer_input_sizes {
        sink.set(size);
    }
}

/// What few nodes' chains contribute: pointer input, window geometry sinks,
/// translated content and live layer resolvers.
#[derive(Clone, Default)]
struct RareSlices {
    pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    pointer_input_sizes: Vec<Rc<std::cell::Cell<cranpose_ui_graphics::Size>>>,
    pointer_icon: Option<PointerIcon>,
    motion_context_animated: bool,
    translated_content_context: bool,
    translated_content_context_identity: Option<usize>,
    translated_content_offset_reader: Option<Rc<dyn Fn() -> Point>>,
    text_window_transform: Option<Rc<std::cell::Cell<cranpose_ui_graphics::ProjectiveTransform>>>,
    viewport_window_rect: Option<Rc<dyn crate::modifier_nodes::WindowRectSink>>,
    graphics_layer_resolver: Option<Rc<dyn Fn() -> GraphicsLayer>>,
    chain_guard: Option<Rc<ChainGuard>>,
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
        self.text_window_transform = None;
        self.viewport_window_rect = None;
        self.graphics_layer_resolver = None;
        self.chain_guard = None;
    }
}

struct ChainGuard {
    _handle: ModifierChainHandle,
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModifierNodeSlicesDebugStats {
    pub draw_command_count: usize,
    pub draw_command_capacity: usize,
    pub pointer_input_count: usize,
    pub pointer_input_capacity: usize,
    pub has_text_content: bool,
    pub has_text_style: bool,
    pub has_text_layout_options: bool,
    pub has_prepared_text_layout: bool,
    pub has_graphics_layer: bool,
    pub has_graphics_layer_resolver: bool,
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
        (Some(filter), Some(next)) => Some(filter.compose(next)),
    }
}

impl ModifierNodeSlices {
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
    pub fn pointer_input_size_sinks(&self) -> &[Rc<std::cell::Cell<cranpose_ui_graphics::Size>>] {
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
    pub fn publish_pointer_input_size(&self, size: cranpose_ui_graphics::Size) {
        if let Some(rare) = self.rare() {
            publish_pointer_input_size(rare, size);
        }
    }

    /// The pointer's appearance over this node, when a `pointer_icon`
    /// modifier names one. The innermost declaration in the chain wins.
    pub fn pointer_icon(&self) -> Option<&PointerIcon> {
        self.rare()?.pointer_icon.as_ref()
    }

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
        if rare.text_window_transform.is_some() || rare.viewport_window_rect.is_some() {
            let local_to_window =
                cranpose_ui_graphics::ProjectiveTransform::translation(origin.x, origin.y)
                    .then(transform);
            if let Some(sink) = &rare.text_window_transform {
                sink.set(local_to_window);
            }
            if let Some(sink) = &rare.viewport_window_rect {
                sink.set(cranpose_ui_graphics::WindowCoordinates {
                    size,
                    local_to_window,
                });
            }
        }
        publish_pointer_input_size(rare, size);
    }

    pub fn motion_context_animated(&self) -> bool {
        self.rare().is_some_and(|rare| rare.motion_context_animated)
    }

    pub fn translated_content_context(&self) -> bool {
        self.rare()
            .is_some_and(|rare| rare.translated_content_context)
    }

    pub fn translated_content_context_identity(&self) -> Option<usize> {
        self.rare()?.translated_content_context_identity
    }

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

    pub fn text_content(&self) -> Option<SliceTextRef<'_, str>> {
        self.annotated_text()
            .map(|text| text.map(|text| text.text.as_str()))
    }

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

    pub fn text_style(&self) -> Option<SliceTextRef<'_, TextStyle>> {
        Some(SliceTextRef(match self.text.as_ref()? {
            SliceText::Text(layout) => TextBorrow::Layout(layout.style()),
            SliceText::Field(field) => TextBorrow::Field(&field.style),
        }))
    }

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

    /// The write target for a text node's local-to-window transform, used by
    /// editable and selectable text for caret, selection and pointer geometry.
    pub fn text_window_transform(
        &self,
    ) -> Option<&Rc<std::cell::Cell<cranpose_ui_graphics::ProjectiveTransform>>> {
        self.rare()?.text_window_transform.as_ref()
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

    pub fn graphics_layer(&self) -> Option<GraphicsLayer> {
        if let Some(resolve) = self
            .rare()
            .and_then(|rare| rare.graphics_layer_resolver.as_ref())
        {
            Some(resolve())
        } else {
            self.graphics_layer.as_deref().cloned()
        }
    }

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

    pub fn with_chain_guard(mut self, handle: ModifierChainHandle) -> Self {
        self.rare_mut().chain_guard = Some(Rc::new(ChainGuard { _handle: handle }));
        self
    }

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

/// Records `node`'s pointer icon when it declares one, leaving the icon already
/// collected in place when it does not.
///
/// The chain is walked head to tail, so the innermost declaration is the last
/// one written and the one that survives.
fn collect_pointer_icon(node: &dyn std::any::Any, slices: &mut ModifierNodeSlices) {
    if let Some(icon_node) = node.downcast_ref::<PointerIconNode>() {
        slices.rare_mut().pointer_icon = Some(icon_node.icon().clone());
    }
}

/// Collects modifier node slices directly from a reconciled [`ModifierNodeChain`]
/// that no layout places: its draws sit inside the padding before them.
pub fn collect_modifier_slices(chain: &ModifierNodeChain) -> ModifierNodeSlices {
    let mut slices = ModifierNodeSlices::default();
    collect_modifier_slices_into(chain, &mut slices, &Rc::default(), 1.0);
    slices
}

/// A node's modifier slices, shared with the render graph. A node whose
/// chain draws, lays out and handles pointers in none of its modifiers holds
/// none: it reads the one empty snapshot such nodes share.
#[derive(Default)]
pub(crate) struct SlicesSnapshot(std::cell::Cell<Option<Rc<ModifierNodeSlices>>>);

thread_local! {
    static EMPTY_SLICES: Rc<ModifierNodeSlices> = Rc::default();
}

impl SlicesSnapshot {
    pub(crate) fn get(&self) -> Rc<ModifierNodeSlices> {
        let snapshot = self.0.take();
        let shared = snapshot.clone();
        self.0.set(snapshot);
        shared.unwrap_or_else(|| EMPTY_SLICES.with(Rc::clone))
    }

    /// Collects `chain`'s slices into the snapshot. Its storage is reused
    /// when nothing else holds it; one the render graph still shares is left
    /// to the graph and replaced, not cloned only to be cleared. The draws
    /// and text read their place from `geometry`, which the node's layout
    /// writes, and until then sit inside the padding before them on the
    /// device pixel grid of `density`.
    pub(crate) fn collect(
        &self,
        chain: &ModifierNodeChain,
        geometry: &Rc<CoordinatorGeometry>,
        density: f32,
    ) {
        if !chain.capabilities().intersects(
            NodeCapabilities::POINTER_INPUT | NodeCapabilities::DRAW | NodeCapabilities::LAYOUT,
        ) {
            self.0.set(None);
            return;
        }
        let mut snapshot = self.0.take();
        if !snapshot
            .as_mut()
            .is_some_and(|slices| Rc::get_mut(slices).is_some())
        {
            snapshot = Some(Rc::default());
        }
        if let Some(slices) = snapshot.as_mut().and_then(Rc::get_mut) {
            collect_modifier_slices_into(chain, slices, geometry, density);
        }
        self.0.set(snapshot);
    }
}

fn collect_modifier_slices_into(
    chain: &ModifierNodeChain,
    slices: &mut ModifierNodeSlices,
    geometry: &Rc<CoordinatorGeometry>,
    density: f32,
) {
    slices.clear();

    let caps = chain.capabilities();
    let has_pointer = caps.intersects(NodeCapabilities::POINTER_INPUT);
    let has_draw = caps.intersects(NodeCapabilities::DRAW);
    let has_layout = caps.intersects(NodeCapabilities::LAYOUT);

    if !has_pointer && !has_draw && !has_layout {
        return;
    }

    let mut backgrounds = Backgrounds::default();
    let mut padding = EdgeInsets::default();
    // The layout nodes walked so far: the index of the coordinator the next
    // draw belongs to, which is the next layout node's (or a layout node's
    // own) or, past the last one, the node's content.
    let mut layout_ordinal = 0_usize;

    for (modifier_index, node_ref) in chain.head_to_tail().enumerate() {
        let node_caps = node_ref.kind_set();

        node_ref.with_node(|node| {
            let any = node.as_any();

            if has_pointer
                && node_caps.intersects(NodeCapabilities::POINTER_INPUT)
                && let Some(pointer_node) = node.as_pointer_input_node()
            {
                if let Some(handler) = pointer_node.pointer_input_handler() {
                    slices.rare_mut().pointer_inputs.push(handler);
                }
                if let Some(sink) = pointer_node.layout_size_sink() {
                    slices.rare_mut().pointer_input_sizes.push(sink);
                }
                collect_pointer_icon(any, slices);
            }

            if has_draw && node_caps.intersects(NodeCapabilities::DRAW) {
                let draw = DrawSite {
                    coordinator: CoordinatorRect::new(geometry, layout_ordinal, padding),
                    displaceable: has_layout,
                    modifier_index,
                };
                collect_draw_node(node, &draw, slices, &mut backgrounds);
            }

            if has_layout && node_caps.intersects(NodeCapabilities::LAYOUT) {
                if let Some(padding_node) = any.downcast_ref::<PaddingNode>() {
                    padding +=
                        crate::modifier_nodes::device_padding(padding_node.padding(), density);
                }

                if let Some(motion_context_node) = any.downcast_ref::<MotionContextAnimatedNode>() {
                    slices.rare_mut().motion_context_animated = motion_context_node.is_active();
                }

                collect_window_geometry_sink(
                    any,
                    // The text a selectable wraps is the layout node after it.
                    CoordinatorRect::new(geometry, layout_ordinal + 1, padding),
                    slices,
                );

                if let Some(translated_content_node) =
                    any.downcast_ref::<TranslatedContentContextNode>()
                {
                    let rare = slices.rare_mut();
                    rare.translated_content_context = translated_content_node.is_active();
                    rare.translated_content_context_identity =
                        Some(translated_content_node.identity());
                    rare.translated_content_offset_reader =
                        translated_content_node.content_offset_reader();
                }

                if let Some(text_node) = any.downcast_ref::<TextModifierNode>() {
                    slices.text_coordinator =
                        Some(CoordinatorRect::new(geometry, layout_ordinal, padding));
                    slices.text = Some(SliceText::Text(text_node.prepared_layout_handle()));
                }

                if let Some(text_field_node) = any.downcast_ref::<TextFieldModifierNode>() {
                    slices.text = Some(SliceText::Field(Rc::new(FieldText {
                        content: Rc::new(crate::text::AnnotatedString::from(
                            text_field_node.text(),
                        )),
                        style: text_field_node.style().clone(),
                        layout: text_field_node.layout_handle(),
                        pan: text_field_node.text_pan_resolver(),
                    })));
                    slices.rare_mut().text_window_transform =
                        Some(text_field_node.window_transform_sink());

                    let coordinator = CoordinatorRect::new(geometry, layout_ordinal, padding);
                    text_field_node.set_content_origin(coordinator.clone());
                    slices.text_coordinator = Some(coordinator);
                }
                layout_ordinal += 1;
            }
        });
    }

    backgrounds.insert_into(slices);
}

/// Where a draw modifier draws: in its coordinator's rect, which only moves
/// off the node's own rect when the chain has layout modifiers.
struct DrawSite {
    coordinator: CoordinatorRect,
    displaceable: bool,
    modifier_index: usize,
}

/// The chain's backgrounds as the walk finds them. Each draws at its place in
/// the draw order, in its coordinator's rect, as Compose draws every
/// background of a chain; a corner shape shapes the nearest background
/// before it, or the next one when none comes before.
#[derive(Default)]
struct Backgrounds {
    slots: SmallVec<[BackgroundSlot; 2]>,
    pending_shape: Option<RoundedCornerShape>,
    last_shape: Option<RoundedCornerShape>,
}

struct BackgroundSlot {
    color: crate::modifier::Color,
    coordinator: CoordinatorRect,
    insert_index: usize,
    precedes_layer: bool,
    corner_shape: Option<RoundedCornerShape>,
}

impl Backgrounds {
    fn push(
        &mut self,
        color: crate::modifier::Color,
        shape: Option<RoundedCornerShape>,
        site: &DrawSite,
        slices: &ModifierNodeSlices,
    ) {
        let corner_shape = shape.or_else(|| self.pending_shape.take());
        if shape.is_some() {
            self.last_shape = shape;
        }
        self.slots.push(BackgroundSlot {
            color,
            coordinator: site.coordinator.clone(),
            insert_index: slices.draw_commands.len(),
            precedes_layer: slices.layer_draw_boundary.is_none(),
            corner_shape,
        });
    }

    fn shape(&mut self, shape: RoundedCornerShape) {
        self.last_shape = Some(shape);
        match self.slots.last_mut() {
            Some(slot) => slot.corner_shape = Some(shape),
            None => self.pending_shape = Some(shape),
        }
    }

    /// Inserts every background at its place, the last first so the places
    /// of those before it still hold.
    fn insert_into(self, slices: &mut ModifierNodeSlices) {
        slices.corner_shape = self.last_shape;
        for slot in self.slots.into_iter().rev() {
            let BackgroundSlot {
                color,
                coordinator,
                insert_index,
                precedes_layer,
                corner_shape,
            } = slot;
            let draw_cmd = Rc::new(move |scope: &mut cranpose_ui_graphics::DrawScopeDefault| {
                use cranpose_ui_graphics::{CornerRadii, DrawScope as _};

                use crate::modifier::Brush;

                let rect = coordinator.rect(scope.size());
                let brush = Brush::solid(color);
                if let Some(shape) = corner_shape {
                    let radii: CornerRadii = shape.resolve(rect.width, rect.height);
                    scope.draw_round_rect_at(rect, brush, radii);
                } else {
                    scope.draw_rect_at(rect, brush);
                }
            });
            slices.insert_background_draw(
                insert_index,
                precedes_layer,
                DrawCommand::Behind(draw_cmd),
            );
        }
    }
}

/// Where layout reports a node's window geometry: a scroll viewport's rect,
/// or the origin of a selectable text, whose content is `text`'s rect.
fn collect_window_geometry_sink(
    any: &dyn std::any::Any,
    text: CoordinatorRect,
    slices: &mut ModifierNodeSlices,
) {
    if let Some(reporter) = any.downcast_ref::<WindowRectReporterNode>() {
        slices.rare_mut().viewport_window_rect = Some(reporter.window_rect_sink());
    }
    if let Some(selectable) = any.downcast_ref::<SelectableTextNode>() {
        slices.rare_mut().text_window_transform =
            Some(selectable.geometry().window_transform_sink());
        selectable.geometry().set_content_origin(text);
    }
}

/// Collects what a draw-capable node contributes, drawn at `site`.
fn collect_draw_node(
    node: &dyn cranpose_foundation::ModifierNode,
    site: &DrawSite,
    slices: &mut ModifierNodeSlices,
    backgrounds: &mut Backgrounds,
) {
    let any = node.as_any();
    if let Some(bg_node) = any.downcast_ref::<BackgroundNode>() {
        backgrounds.push(bg_node.color(), bg_node.shape(), site, slices);
    }

    if let Some(shape_node) = any.downcast_ref::<CornerShapeNode>() {
        backgrounds.shape(shape_node.shape());
    }

    if let Some(commands) = any.downcast_ref::<DrawCommandNode>() {
        slices.draw_commands.extend(
            commands
                .observed_commands(site.modifier_index)
                .map(|command| placed_draw_command(command, site)),
        );
    }

    if let Some(draw_node) = node.as_draw_node() {
        collect_draw_closures(draw_node, site, slices);
    }

    if let Some(layer_node) = any.downcast_ref::<GraphicsLayerNode>() {
        slices.mark_layer_draw_boundary();
        slices.push_graphics_layer(
            layer_node.layer_snapshot(),
            layer_node.layer_resolver(site.modifier_index),
        );
        slices.enter_layer(site);
    }

    if any.is::<ClipToBoundsNode>() {
        slices.mark_layer_draw_boundary();
        slices.clip_to_bounds = true;
        slices.enter_layer(site);
    }
}

/// A draw node's behind and overlay closures, or what its `draw` records
/// when it has no overlay closure.
fn collect_draw_closures(
    draw_node: &dyn cranpose_foundation::DrawModifierNode,
    site: &DrawSite,
    slices: &mut ModifierNodeSlices,
) {
    if let Some(closure) = draw_node.create_behind_draw_closure() {
        slices
            .draw_commands
            .push(placed_draw_command(DrawCommand::Behind(closure), site));
    }
    if let Some(closure) = draw_node.create_draw_closure() {
        slices
            .draw_commands
            .push(placed_draw_command(DrawCommand::Overlay(closure), site));
    }
}

/// `command` drawn where Compose draws a draw modifier: in its coordinator's
/// rect, sized to it, with what it records moved to that rect.
fn placed_draw_command(command: DrawCommand, site: &DrawSite) -> DrawCommand {
    if !site.displaceable {
        return command;
    }
    let place = |draw: crate::draw::DrawCommandFn| -> crate::draw::DrawCommandFn {
        let coordinator = site.coordinator.clone();
        Rc::new(move |scope: &mut cranpose_ui_graphics::DrawScopeDefault| {
            use cranpose_ui_graphics::DrawScope as _;

            let insets = coordinator.insets(scope.size());
            if insets.is_zero() {
                draw(scope);
            } else {
                scope.inset(insets, |inner| draw(inner));
            }
        })
    };
    match command {
        DrawCommand::Behind(draw) => DrawCommand::Behind(place(draw)),
        DrawCommand::WithContent(draw) => DrawCommand::WithContent(place(draw)),
        DrawCommand::Overlay(draw) => DrawCommand::Overlay(place(draw)),
    }
}

/// Collects modifier node slices by instantiating a temporary node chain from a [`Modifier`].
pub fn collect_slices_from_modifier(modifier: &Modifier) -> ModifierNodeSlices {
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(modifier);
    collect_modifier_slices(handle.chain()).with_chain_guard(handle)
}

#[cfg(test)]
#[path = "tests/slices_tests.rs"]
mod tests;
