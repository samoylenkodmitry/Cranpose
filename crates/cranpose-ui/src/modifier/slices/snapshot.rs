//! Collecting a chain's slices: the snapshot a node shares with the render
//! graph, and the walk over the chain that fills it.

use std::{
    any::{Any, TypeId},
    rc::Rc,
};

use cranpose_foundation::{ModifierNodeChain, NodeCapabilities};
use cranpose_ui_graphics::{EdgeInsets, RoundedCornerShape};
use smallvec::SmallVec;

use super::{
    super::{ModifierChainHandle, coordinator_rect::CoordinatorRect},
    DrawSite, FieldText, ModifierNodeSlices, SliceText,
};
use crate::{
    draw::DrawCommand,
    modifier::{
        Modifier,
        offset::OffsetPxNode,
        scroll::{MotionContextAnimatedNode, TranslatedContentContextNode},
    },
    modifier_nodes::{
        BackgroundElement, ClipToBoundsElement, CornerShapeElement, DrawCommandNode,
        GraphicsLayerNode, PaddingElement, PointerIconNode, SelectableTextNode,
        WindowRectReporterNode,
    },
    text_field_modifier_node::TextFieldModifierNode,
    text_modifier_node::TextModifierNode,
    widgets::nodes::LayoutParts,
};

/// Records `node`'s pointer icon when it declares one, leaving the icon already
/// collected in place when it does not.
///
/// The chain is walked head to tail, so the innermost declaration is the last
/// one written and the one that survives.
fn collect_pointer_icon(node: &dyn Any, slices: &mut ModifierNodeSlices) {
    if let Some(icon_node) = node.downcast_ref::<PointerIconNode>() {
        slices.rare_mut().pointer_icon = Some(icon_node.icon().clone());
    }
}

/// Collects modifier node slices directly from a reconciled [`ModifierNodeChain`]
/// that no layout places: its draws sit inside the padding before them.
pub fn collect_modifier_slices(chain: &ModifierNodeChain) -> ModifierNodeSlices {
    let mut slices = ModifierNodeSlices::default();
    collect_modifier_slices_into(chain, &mut slices, None, 1.0);
    slices
}

/// A node's modifier slices, shared with the render graph. A node whose
/// chain gives no slices, as one that only pads and sizes, holds none: it
/// reads the one empty snapshot such nodes share.
#[derive(Default)]
pub(crate) struct SlicesSnapshot(std::cell::Cell<Option<Rc<ModifierNodeSlices>>>);

impl SlicesSnapshot {
    pub(crate) fn get(&self) -> Rc<ModifierNodeSlices> {
        let snapshot = self.0.take();
        let shared = snapshot.clone();
        self.0.set(snapshot);
        shared.unwrap_or_else(|| {
            crate::thread_state::with(|state| {
                Rc::clone(state.empty_slices.get_or_init(Rc::default))
            })
        })
    }

    /// Collects `chain`'s slices into the snapshot. Its storage is reused
    /// when nothing else holds it; one the render graph still shares is left
    /// to the graph and replaced, not cloned only to be cleared. The draws
    /// and text read their place from the coordinator rects of `parts`,
    /// which the node's layout writes, and until then, or without `parts`,
    /// sit inside the padding before them on the device pixel grid of
    /// `density`.
    pub(crate) fn collect(
        &self,
        chain: &ModifierNodeChain,
        parts: Option<&Rc<LayoutParts>>,
        density: f32,
    ) {
        if !chain.capabilities().intersects(
            NodeCapabilities::POINTER_INPUT | NodeCapabilities::DRAW | NodeCapabilities::LAYOUT,
        ) {
            self.0.set(None);
            return;
        }
        let mut snapshot = self.0.take();
        if let Some(slices) = snapshot.as_mut().and_then(Rc::get_mut) {
            collect_modifier_slices_into(chain, slices, parts, density);
            if slices.is_empty() {
                snapshot = None;
            }
        } else {
            // A chain that gives no slices allocates none: it collects into
            // the thread's spare, which a chain that gives some takes.
            let spare = crate::thread_state::with(|state| state.spare_slices.take());
            let mut slices = spare.unwrap_or_default();
            collect_modifier_slices_into(chain, &mut slices, parts, density);
            snapshot = if slices.is_empty() {
                crate::thread_state::with(|state| state.spare_slices.set(Some(slices)));
                None
            } else {
                Some(Rc::new(slices))
            };
        }
        self.0.set(snapshot);
    }
}

fn collect_modifier_slices_into(
    chain: &ModifierNodeChain,
    slices: &mut ModifierNodeSlices,
    parts: Option<&Rc<LayoutParts>>,
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

        // A plain element is a background, a corner shape or a layout
        // modifier, whose coordinator the draws after it belong past.
        let site = PlainSite {
            parts,
            ordinal: layout_ordinal,
            padding,
            displaceable: has_layout,
            modifier_index,
        };
        if let Some(insets) = backgrounds.take_plain(&node_ref, site, density, slices) {
            padding += insets;
            layout_ordinal += 1;
        }

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
                    coordinator: CoordinatorRect::new(parts, layout_ordinal, padding),
                    displaceable: has_layout,
                    modifier_index,
                };
                collect_draw_node(node, &draw, slices);
            }

            // `InPlaceReach::of` names the layout elements whose values no
            // slice reads; a slice that reads one here takes it off that list.
            if has_layout && node_caps.intersects(NodeCapabilities::LAYOUT) {
                if let Some(motion_context_node) = any.downcast_ref::<MotionContextAnimatedNode>() {
                    slices.rare_mut().motion_context_animated = motion_context_node.is_active();
                }

                collect_window_geometry_sink(
                    any,
                    // The text a selectable wraps is the layout node after it.
                    CoordinatorRect::new(parts, layout_ordinal + 1, padding),
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
                        Some(CoordinatorRect::new(parts, layout_ordinal, padding));
                    slices.text = Some(SliceText::Text(text_node.prepared_layout_handle()));
                }

                if let Some(text_field_node) = any.downcast_ref::<TextFieldModifierNode>() {
                    slices.text = Some(SliceText::Field(Rc::new(FieldText {
                        content: text_field_node.shown_text(),
                        style: text_field_node.style().clone(),
                        layout: text_field_node.layout_handle(),
                        pan: text_field_node.text_pan_resolver(),
                    })));
                    slices.rare_mut().text_window = Some(text_field_node.window_sink());

                    let coordinator = CoordinatorRect::new(parts, layout_ordinal, padding);
                    text_field_node.set_content_origin(coordinator.clone());
                    slices.text_coordinator = Some(coordinator);
                }
                layout_ordinal += 1;
            }
        });
    }

    backgrounds.insert_into(slices);
    // The slices stay for as long as the chain does, and so would the room
    // growth left past the commands. A collect into these again pushes as
    // many commands, mostly, which then fit without growing.
    slices.draw_commands.shrink_to_fit();
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

// Where a plain element sits in the walk of its chain: the coordinator the
// draws at it belong to, which is `ordinal` under `padding`, whether a
// layout modifier of the chain may displace them, and its entry.
#[derive(Clone, Copy)]
struct PlainSite<'a> {
    parts: Option<&'a Rc<LayoutParts>>,
    ordinal: usize,
    padding: EdgeInsets,
    displaceable: bool,
    modifier_index: usize,
}

impl PlainSite<'_> {
    fn coordinator(&self) -> CoordinatorRect {
        CoordinatorRect::new(self.parts, self.ordinal, self.padding)
    }
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
        coordinator: CoordinatorRect,
        slices: &ModifierNodeSlices,
    ) {
        let corner_shape = self.pending_shape.take();
        self.slots.push(BackgroundSlot {
            color,
            coordinator,
            insert_index: slices.draw_commands.len(),
            precedes_layer: slices.layer.is_none(),
            corner_shape,
        });
    }

    // Takes in the plain element at `node_ref`: a background, which draws
    // in the rect of the coordinator at `site`, a corner shape, a clip to
    // that coordinator's bounds, or a layout modifier, whose insets at
    // `density` it returns, zero for all but a padding. `None` for a node.
    fn take_plain(
        &mut self,
        node_ref: &cranpose_foundation::ModifierChainNodeRef<'_>,
        site: PlainSite<'_>,
        density: f32,
        slices: &mut ModifierNodeSlices,
    ) -> Option<EdgeInsets> {
        let (fns, words) = node_ref.plain_words()?;
        if let Some(background) = fns.read::<BackgroundElement>(words) {
            self.push(background.color(), site.coordinator(), slices);
        } else if let Some(shape) = fns.read::<CornerShapeElement>(words) {
            self.shape(shape.shape());
        } else if fns.read::<ClipToBoundsElement>(words).is_some() {
            slices.layer_mut().clip_to_bounds = true;
            slices.enter_layer(&DrawSite {
                coordinator: site.coordinator(),
                displaceable: site.displaceable,
                modifier_index: site.modifier_index,
            });
        }
        fns.capabilities()
            .contains(NodeCapabilities::LAYOUT)
            .then(|| match fns.read::<PaddingElement>(words) {
                Some(element) => crate::modifier_nodes::device_padding(element.padding(), density),
                None => EdgeInsets::default(),
            })
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
        slices.set_corner_shape(self.last_shape);
        for slot in self.slots.into_iter().rev() {
            let BackgroundSlot {
                color,
                coordinator,
                insert_index,
                precedes_layer,
                corner_shape,
            } = slot;
            slices.insert_background_draw(
                insert_index,
                precedes_layer,
                DrawCommand::Background(crate::BackgroundDraw::new(
                    color,
                    corner_shape,
                    coordinator,
                )),
            );
        }
    }
}

/// Where layout reports a node's window geometry: a scroll viewport's rect,
/// or the origin of a selectable text, whose content is `text`'s rect.
fn collect_window_geometry_sink(
    any: &dyn Any,
    text: CoordinatorRect,
    slices: &mut ModifierNodeSlices,
) {
    if let Some(reporter) = any.downcast_ref::<WindowRectReporterNode>() {
        slices.rare_mut().viewport_window_rect = Some(reporter.window_rect_sink());
    }
    if let Some(selectable) = any.downcast_ref::<SelectableTextNode>() {
        slices.rare_mut().text_window = Some(selectable.geometry().window_sink());
        selectable.geometry().set_content_origin(text);
    }
}

/// Collects what a draw-capable node contributes, drawn at `site`.
fn collect_draw_node(
    node: &dyn cranpose_foundation::ModifierNode,
    site: &DrawSite,
    slices: &mut ModifierNodeSlices,
) {
    let any = node.as_any();
    // The node's type, asked once: each check below compares it, where a
    // downcast would ask the node again.
    let kind = Any::type_id(any);
    if let Some(z_index_node) = as_kind::<crate::modifier::z_index::ZIndexNode>(any, kind) {
        slices.rare_mut().z_index += z_index_node.z_index();
    }

    if let Some(commands) = as_kind::<DrawCommandNode>(any, kind) {
        slices.draw_commands.extend(
            commands
                .observed_commands(site.modifier_index)
                .map(|command| placed_draw_command(command, site)),
        );
    }

    if let Some(draw_node) = node.as_draw_node() {
        collect_draw_closures(draw_node, site, slices);
    }

    let layer_node = as_kind::<GraphicsLayerNode>(any, kind)
        .or_else(|| as_kind::<OffsetPxNode>(any, kind).map(OffsetPxNode::layer));
    if let Some(layer_node) = layer_node {
        slices.push_graphics_layer(
            layer_node.layer_snapshot(),
            layer_node.layer_resolver(site.modifier_index),
        );
        slices.enter_layer(site);
    }
}

// `any` as a `T` when its type `kind` is `T`.
fn as_kind<T: Any>(any: &dyn Any, kind: TypeId) -> Option<&T> {
    if kind == TypeId::of::<T>() {
        any.downcast_ref::<T>()
    } else {
        None
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
                scope.draw_inset(insets, |inner| draw(inner));
            }
        })
    };
    match command {
        DrawCommand::Behind(draw) => DrawCommand::Behind(place(draw)),
        DrawCommand::WithContent(draw) => DrawCommand::WithContent(place(draw)),
        DrawCommand::Overlay(draw) => DrawCommand::Overlay(place(draw)),
        // A background sits in its own coordinator's rect already.
        background @ DrawCommand::Background(_) => background,
    }
}

/// Collects modifier node slices by instantiating a temporary node chain from a [`Modifier`].
pub fn collect_slices_from_modifier(modifier: &Modifier) -> ModifierNodeSlices {
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(modifier);
    collect_modifier_slices(handle.chain()).with_chain_guard(handle)
}
