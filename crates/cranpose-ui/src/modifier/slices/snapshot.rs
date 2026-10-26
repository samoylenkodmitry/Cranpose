//! Collecting a chain's slices: the snapshot a node shares with the render
//! graph, and the walk over the chain that fills it.

use std::rc::Rc;

use cranpose_foundation::{ModifierNodeChain, NodeCapabilities};
use cranpose_ui_graphics::{EdgeInsets, RoundedCornerShape};
use smallvec::SmallVec;

use super::{
    super::{ModifierChainHandle, coordinator_geometry::CoordinatorRect},
    DrawSite, FieldText, ModifierNodeSlices, SliceText,
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
    text_field_modifier_node::TextFieldModifierNode,
    text_modifier_node::TextModifierNode,
    widgets::nodes::LayoutParts,
};

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
                collect_draw_node(node, &draw, slices, &mut backgrounds);
            }

            // `InPlaceReach::of` names the layout elements whose values no
            // slice reads; a slice that reads one here takes it off that list.
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
        slices.rare_mut().text_window = Some(selectable.geometry().window_sink());
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

    if let Some(z_index_node) = any.downcast_ref::<crate::modifier::z_index::ZIndexNode>() {
        slices.rare_mut().z_index += z_index_node.z_index();
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
                scope.draw_inset(insets, |inner| draw(inner));
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
