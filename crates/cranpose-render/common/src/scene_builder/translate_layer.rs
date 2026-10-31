// How a scene update brings a retained layer to its node's new layout in
// place: the layer keeps the child layers that stay, moves them, and builds
// only the children that enter.

use std::{
    borrow::Cow,
    ops::{Range, RangeInclusive},
    rc::Rc,
};

use cranpose_core::{MemoryApplier, NodeId, collections::map::HashMap};
use cranpose_ui::{ModifierNodeSlices, Point, Rect, Size};

use super::{
    DrawTarget, KeptChild, LowerContext, PlacedGeometry, SnapshotNodeData,
    TranslateAncestorContext, TranslateGeometry, Wrapping, children_have_origin_sinks,
    content_layer_mut, forget_dirty_subtree, hit_test_from_slices, layer_and_node_bounds,
    layer_identity, lower_child, modifier_slices_have_origin_sinks, move_kept_child, outer_draws,
    placed_child_geometry, placed_transform, read_node_data, remove_dirty_descendants,
    shaped_graphics_layer, take_wrapped_layer, text_node, translate_bail, translate_unwrapped,
    write_node_content, write_wrapper,
};
use crate::{
    NodeUpdate,
    graph::{LayerNode, RenderNode},
};

/// [`try_translate_scrolled_layer`], returning whether hit geometry may
/// have changed: a layer that neither held nor holds a hit target and lies
/// in no 3D space leaves the hit graph as it was, as
/// [`rebuild_tracking_hits`] would find. `None` when the layer cannot be
/// translated.
pub(super) fn translate_tracking_hits(
    applier: &MemoryApplier,
    layer: &mut LayerNode,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    ancestors: TranslateAncestorContext,
) -> Option<bool> {
    let held = holds_hit_geometry(layer);
    try_translate_scrolled_layer(applier, layer, dirty_nodes, ancestors)
        .then(|| held || holds_hit_geometry(layer))
}

/// Whether the hit graph reads `layer`: it is a hit target, holds one or is
/// a 3D space, the states [`HitGraphState::dirty_against`] compares.
fn holds_hit_geometry(layer: &LayerNode) -> bool {
    layer.hit_test.is_some() || layer.has_hit_targets || layer.graphics_layer.space.is_some()
}

fn try_translate_scrolled_layer(
    applier: &MemoryApplier,
    container: &mut LayerNode,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    ancestors: TranslateAncestorContext,
) -> bool {
    let Some(node_id) = layer_identity(container) else {
        return translate_bail("no node id");
    };
    read_node_data(applier, node_id, |data| {
        if container.wraps.is_none() {
            return translate_unwrapped(applier, container, dirty_nodes, ancestors, data);
        }
        let outer_count = data.modifier_slices.outer_draw_command_count();
        if outer_count == 0 {
            return translate_bail("outer draws removed");
        }
        let size = data.layout_state.size();
        let placement = data.layout_state.position();
        let slices = Rc::clone(&data.modifier_slices);
        let Some(inner) = content_layer_mut(container) else {
            return translate_bail("wrapped layer missing");
        };
        if !translate_layer_from_data(
            applier,
            inner,
            dirty_nodes,
            ancestors,
            data,
            Wrapping::Wrapped,
        ) {
            return false;
        }
        let draws = DrawTarget {
            size,
            motion_context_animated: inner.motion_context_animated,
            layer: None,
        };
        let outer = outer_draws(node_id, slices.draw_commands(), outer_count, draws)
            .expect("outer command count is nonzero");
        let layer =
            take_wrapped_layer(container, node_id).expect("the wrapped layer was found above");
        write_wrapper(
            container,
            layer,
            placement,
            outer,
            ancestors.parent_content_offset,
        );
        container.note_content_change(ancestors.update);
        true
    })
    .unwrap_or_else(|| translate_bail("container snapshot read failed"))
}

/// The node of a container that keeps its children, and its node bounds at
/// its current size.
pub(super) struct TranslatedContainer {
    pub(super) node_id: NodeId,
    pub(super) node_bounds: Option<Rect>,
}

/// Checks that `container` can keep its children and brings its bounds and
/// graphics layer to its current size: a container that kept its size keeps
/// its layer as it is.
pub(super) fn translated_container(
    container: &mut LayerNode,
    layout_state: &cranpose_ui::widgets::LayoutState,
    modifier_slices: &ModifierNodeSlices,
    ancestors: TranslateAncestorContext,
    wrapping: Wrapping,
) -> Result<TranslatedContainer, &'static str> {
    if cranpose_core::DIAGNOSTICS.disable_scroll_translate.is_set() {
        return Err("fast path disabled by ablation switch");
    }
    let Some(node_id) = container.node_id else {
        return Err("no node id");
    };
    if !layout_state.is_placed() {
        return Err("container unplaced");
    }
    let outer_count = modifier_slices.outer_draw_command_count();
    if (outer_count > 0 && wrapping == Wrapping::Bare)
        || (ancestors.inherited_motion_context_animated
            || modifier_slices.motion_context_animated())
            != container.motion_context_animated
        || modifier_slices.translated_content_context() != container.translated_content_context
    {
        return Err("container outer draws or translated context changed");
    }
    let clip_to_bounds = modifier_slices.clip_to_bounds();
    if clip_to_bounds != container.clip_to_bounds {
        return Err("container clip changed");
    }
    // A resized container keeps its children: only its bounds and the clip
    // shaped to them follow its size. Any other change to its graphics layer
    // may change what its children inherit.
    if !container
        .graphics_layer
        .reads_as(shaped_graphics_layer(modifier_slices, container.local_bounds).as_deref())
    {
        return Err("container graphics layer changed");
    }
    let (local_bounds, node_bounds) = layer_and_node_bounds(modifier_slices, layout_state.size());
    if local_bounds != container.local_bounds {
        container.local_bounds = local_bounds;
        container
            .graphics_layer
            .replace(shaped_graphics_layer(modifier_slices, local_bounds).map(Cow::into_owned));
    }
    Ok(TranslatedContainer {
        node_id,
        node_bounds,
    })
}

/// What a scroll step reconciles a container's children in, kept between
/// steps so a step allocates nothing of its own. Every list is empty
/// between steps, so it holds no layer.
#[derive(Default)]
pub(crate) struct TranslateScratch {
    /// The placed children, in their new order.
    placed_fresh: Vec<PlacedChild>,
    /// The ids of the container's previous children, in their order.
    old_ids: Vec<NodeId>,
    old_index_by_id: HashMap<NodeId, usize>,
    /// The container's previous children by their index, until each is
    /// kept or recycled.
    retained: Vec<Option<Box<LayerNode>>>,
    /// The previous children that stay, in their new order; `None` where a
    /// child enters.
    kept: Vec<Option<Box<LayerNode>>>,
    /// The children built for the step, in their new order.
    entering: Vec<(NodeId, Box<LayerNode>)>,
}

/// A placed child of a container a scroll step translates.
pub(super) struct PlacedChild {
    pub(super) id: NodeId,
    pub(super) geometry: PlacedGeometry,
    /// The child's index among the container's previous children; `None`
    /// for a child that enters.
    old_index: Option<usize>,
    pub(super) kept: KeptChild,
}

/// How a translated container's placed children stand to its previous ones.
enum ChildOrder {
    /// The kept children lie together and in their order, at `kept` among
    /// the previous children, and the entering ones come before or after
    /// them: `entering_before` of the placed children, then the kept ones,
    /// then the rest. A scroll leaves its children so, and so do unchanged
    /// children.
    Shifted {
        kept: Range<usize>,
        entering_before: usize,
    },
    /// The kept children changed their order, or children enter between
    /// them.
    Mixed,
}

/// Fills `scratch` with `container`'s placed children, each with its index
/// among the previous ones, and returns how they stand to those.
fn translated_children(
    applier: &MemoryApplier,
    container: &LayerNode,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    fresh_children: &[NodeId],
    scratch: &mut TranslateScratch,
) -> Result<ChildOrder, &'static str> {
    let placed_fresh = &mut scratch.placed_fresh;
    placed_fresh.clear();
    for &id in fresh_children {
        let Some(geometry) = placed_child_geometry(applier, id) else {
            continue;
        };
        placed_fresh.push(PlacedChild {
            id,
            geometry,
            old_index: None,
            kept: KeptChild::Left,
        });
    }
    let children_unchanged = container.children.len() == placed_fresh.len()
        && container
            .children
            .iter()
            .zip(placed_fresh.iter())
            .all(|(child, placed)| {
                matches!(child, RenderNode::Layer(layer) if layer_identity(layer) == Some(placed.id))
            });
    let order = if children_unchanged {
        for (index, placed) in placed_fresh.iter_mut().enumerate() {
            placed.old_index = Some(index);
        }
        ChildOrder::Shifted {
            kept: 0..placed_fresh.len(),
            entering_before: 0,
        }
    } else {
        let old_ids = &mut scratch.old_ids;
        old_ids.clear();
        let (mut lowest, mut highest) = (NodeId(usize::MAX), NodeId(0));
        for child in &container.children {
            let RenderNode::Layer(layer) = child else {
                return Err("child without node id");
            };
            let Some(id) = layer_identity(layer) else {
                return Err("child without node id");
            };
            lowest = lowest.min(id);
            highest = highest.max(id);
            old_ids.push(id);
        }
        match_old_children(
            old_ids,
            lowest..=highest,
            placed_fresh,
            &mut scratch.old_index_by_id,
        )
    };
    check_retained_children(container, dirty_nodes, placed_fresh).map(|()| order)
}

/// The searches of the previous children a match makes before it maps them:
/// a search reads each id once, a map hashes each of them in.
const MATCH_SEARCHES: usize = 16;

/// Notes each placed child's index among the previous children `old_ids`,
/// whose ids lie in `old_id_range`.
/// Children that kept their order match in turn, as a scroll that brings
/// rows in at one end and lets them go at the other leaves them; a child
/// past a gap is searched for, and the match continues after it. A child
/// whose id lies outside the range enters without a search: a node made
/// since the previous children takes an id past all of theirs. Once the
/// order is mixed past [`MATCH_SEARCHES`] searches, `old_index_by_id` maps
/// the previous children for the rest. An id is a node's, so it is once
/// among the previous children. Returns how the placed children stand to
/// the previous ones.
fn match_old_children(
    old_ids: &[NodeId],
    old_id_range: RangeInclusive<NodeId>,
    placed_fresh: &mut [PlacedChild],
    old_index_by_id: &mut HashMap<NodeId, usize>,
) -> ChildOrder {
    let mut cursor = 0;
    let mut searches = 0;
    let mut mapped = false;
    let mut kept: Option<Range<usize>> = None;
    let mut entering_before = 0;
    let mut entering_after = 0;
    let mut shifted = true;
    for placed in placed_fresh {
        let old_index = if old_ids.get(cursor) == Some(&placed.id) {
            Some(cursor)
        } else if !old_id_range.contains(&placed.id) {
            None
        } else if searches < MATCH_SEARCHES {
            searches += 1;
            old_ids.iter().position(|&id| id == placed.id)
        } else {
            if !mapped {
                old_index_by_id.clear();
                old_index_by_id.extend(old_ids.iter().enumerate().map(|(index, &id)| (id, index)));
                mapped = true;
            }
            old_index_by_id.get(&placed.id).copied()
        };
        match (old_index, kept.as_mut()) {
            (Some(index), Some(kept)) => {
                shifted &= entering_after == 0 && index == kept.end;
                kept.end = index + 1;
            }
            (Some(index), None) => kept = Some(index..index + 1),
            (None, Some(_)) => entering_after += 1,
            (None, None) => entering_before += 1,
        }
        if let Some(index) = old_index {
            cursor = index + 1;
        }
        placed.old_index = old_index;
    }
    if shifted {
        ChildOrder::Shifted {
            kept: kept.unwrap_or(0..0),
            entering_before,
        }
    } else {
        ChildOrder::Mixed
    }
}

/// Checks that every child kept as it is still fits its layer, and notes
/// which children keep their layers. A moved child that no longer fits is
/// drawn again: it becomes a content update. A moved child that fits leaves
/// the dirty set, which this step settles.
fn check_retained_children(
    container: &LayerNode,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    placed_fresh: &mut [PlacedChild],
) -> Result<(), &'static str> {
    for placed in placed_fresh {
        let Some(old_index) = placed.old_index else {
            continue;
        };
        let RenderNode::Layer(layer) = &container.children[old_index] else {
            return Err("retained child slot is not a layer");
        };
        let fits = Rect::from_size(placed.geometry.size) == layer.node_rect();
        placed.kept = match dirty_nodes.get_mut(&placed.id) {
            None if !fits => return Err("child resized"),
            None => KeptChild::Moved,
            Some(kind @ (NodeUpdate::Moved | NodeUpdate::MovedLayer)) if !fits => {
                *kind = NodeUpdate::Content;
                KeptChild::Left
            }
            Some(NodeUpdate::Moved) => {
                dirty_nodes.remove(&placed.id);
                KeptChild::Moved
            }
            // The child moves with the others; the walk beneath the container
            // then gives it its new layer properties or draws where it now lies.
            Some(NodeUpdate::Layer | NodeUpdate::Draws) if fits => KeptChild::Moved,
            Some(NodeUpdate::Content) if text_node::draws_text_alone(layer) => KeptChild::Retexted,
            Some(_) => KeptChild::Left,
        };
    }
    Ok(())
}

/// Moves `container`'s previous children that stay into `scratch.kept`, in
/// their new order, and the subtrees of those that leave into the layer
/// pool, so the entering children are built in their allocations.
/// Recycles the children that leave `container`. Their nodes leave the
/// dirty set with them: nothing remains to update for a node the scene no
/// longer shows, and one left dirty would go unmatched and rebuild the whole
/// scene.
fn recycle_leaving_children(
    container: &mut LayerNode,
    scratch: &mut TranslateScratch,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
) {
    let TranslateScratch {
        placed_fresh,
        retained,
        kept,
        ..
    } = scratch;
    retained.clear();
    retained.extend(container.children.drain(..).map(|child| match child {
        RenderNode::Layer(layer) => Some(layer),
        RenderNode::Primitive(_) | RenderNode::DrawRun(_) => None,
    }));
    kept.clear();
    kept.extend(
        placed_fresh
            .iter()
            .map(|placed| placed.old_index.and_then(|index| retained[index].take())),
    );
    for leaving in retained.drain(..).flatten() {
        forget_dirty_subtree(&leaving, dirty_nodes);
        crate::layer_recycling::recycle(leaving);
    }
}

/// Recycles the children that leave `container` around the `kept` ones,
/// which stay where they lie, already in their new order. Their nodes
/// leave the dirty set with them, as in [`recycle_leaving_children`].
fn recycle_children_around(
    container: &mut LayerNode,
    kept: Range<usize>,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
) {
    let mut recycle = |leaving| {
        if let RenderNode::Layer(leaving) = leaving {
            forget_dirty_subtree(&leaving, dirty_nodes);
            crate::layer_recycling::recycle(leaving);
        }
    };
    // Most steps keep every child.
    if kept.end < container.children.len() {
        container.children.drain(kept.end..).for_each(&mut recycle);
    }
    if kept.start > 0 {
        container.children.drain(..kept.start).for_each(recycle);
    }
}

/// Builds the children that enter `container`. They are built from the
/// applier's current state, so their nodes leave the dirty set: the walk
/// beneath would otherwise build each of them a second time.
fn build_entering_children(
    applier: &MemoryApplier,
    scratch: &mut TranslateScratch,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    children: TranslateAncestorContext,
) {
    let entering = &mut scratch.entering;
    entering.clear();
    let context = LowerContext::beneath(children);
    for placed in &scratch.placed_fresh {
        if placed.old_index.is_some() {
            continue;
        }
        let child_id = &placed.id;
        let Some(mut lowered) = lower_child(applier, *child_id, context) else {
            continue;
        };
        lowered.note_content_change(children.update);
        forget_dirty_subtree(&lowered, dirty_nodes);
        entering.push((*child_id, lowered));
    }
}

/// Records `container`'s own draws and text around the child layers its list
/// holds, in the order [`write_node_content`] writes a layer.
pub(super) fn write_own_content_around_children(
    container: &mut LayerNode,
    node_id: NodeId,
    modifier_slices: &ModifierNodeSlices,
    size: Size,
) {
    if modifier_slices.draw_commands()[modifier_slices.outer_draw_command_count()..].is_empty()
        && !modifier_slices.has_text()
    {
        return;
    }
    let kept = container.children.len();
    let draws = DrawTarget {
        size,
        motion_context_animated: container.motion_context_animated,
        layer: container.graphics_layer.get(),
    };
    write_node_content(
        &mut container.children,
        node_id,
        modifier_slices,
        draws,
        (kept, 0),
        |_| {},
    );
}

pub(super) fn apply_translated_container_state(
    container: &mut LayerNode,
    modifier_slices: &ModifierNodeSlices,
    layout_state: &cranpose_ui::widgets::LayoutState,
    parent_content_offset: Point,
    geometry: TranslateGeometry,
) {
    container.transform_to_parent = placed_transform(
        container.local_bounds,
        layout_state.position(),
        container.graphics_layer.get(),
        parent_content_offset,
    );
    container.content_offset = geometry.content_offset;
    if container.translated_content_context {
        container.translated_content_offset = modifier_slices
            .translated_content_offset()
            .unwrap_or(geometry.content_offset);
    }
    modifier_slices.publish_window_geometry(
        geometry.top_left,
        geometry.window_transform,
        layout_state.size(),
    );
    container.origin_in_parent = layout_state.position();
}

fn reconcile_translated_children(
    applier: &MemoryApplier,
    container: &mut LayerNode,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    scratch: &mut TranslateScratch,
    order: ChildOrder,
    children: TranslateAncestorContext,
) {
    if let ChildOrder::Shifted {
        entering_before, ..
    } = order
    {
        reconcile_shifted_children(
            applier,
            container,
            dirty_nodes,
            scratch,
            entering_before,
            children,
        );
        return;
    }
    let mut entering = scratch.entering.drain(..).peekable();
    for (placed, kept) in scratch.placed_fresh.iter().zip(scratch.kept.drain(..)) {
        if let Some(mut layer) = kept {
            move_kept_child(applier, &mut layer, placed, children, dirty_nodes);
            container.children.push(RenderNode::Layer(layer));
        } else if let Some((_, lowered)) = entering.next_if(|(id, _)| *id == placed.id) {
            dirty_nodes.remove(&placed.id);
            remove_dirty_descendants(&lowered, dirty_nodes);

            container.children.push(RenderNode::Layer(lowered));
        }
    }
}

/// Moves the kept children, which lie in `container` in their new order, to
/// their placements, and puts the entering children before and after them:
/// those of the first `entering_before` placed children before.
fn reconcile_shifted_children(
    applier: &MemoryApplier,
    container: &mut LayerNode,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    scratch: &mut TranslateScratch,
    entering_before: usize,
    children: TranslateAncestorContext,
) {
    let TranslateScratch {
        placed_fresh,
        entering,
        ..
    } = scratch;
    for (child, placed) in container
        .children
        .iter_mut()
        .zip(&placed_fresh[entering_before..])
    {
        let RenderNode::Layer(layer) = child else {
            unreachable!("retained child identities were checked");
        };
        move_kept_child(applier, layer, placed, children, dirty_nodes);
    }
    if entering.is_empty() {
        return;
    }
    // The children built for the step lie in their placed order, and those
    // placed before the kept ones come first.
    let mut built = entering.iter().peekable();
    let built_before = placed_fresh[..entering_before]
        .iter()
        .filter(|placed| built.next_if(|(id, _)| *id == placed.id).is_some())
        .count();
    for (id, lowered) in entering.iter() {
        dirty_nodes.remove(id);
        remove_dirty_descendants(lowered, dirty_nodes);
    }
    let layer = |(_, lowered): (NodeId, Box<LayerNode>)| RenderNode::Layer(lowered);
    if built_before > 0 {
        container
            .children
            .splice(0..0, entering.drain(..built_before).map(layer));
    }
    container.children.extend(entering.drain(..).map(layer));
}

/// Runs `f` on the thread's translate scratch where it lies, or on a new one
/// while another translate holds it.
fn with_translate_scratch<R>(f: impl FnOnce(&mut TranslateScratch) -> R) -> R {
    crate::thread_state::with(|state| match state.translate_scratch.try_borrow_mut() {
        Ok(mut scratch) => f(&mut scratch),
        Err(_) => f(&mut TranslateScratch::default()),
    })
}

pub(super) fn translate_layer_from_data(
    applier: &MemoryApplier,
    container: &mut LayerNode,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    ancestors: TranslateAncestorContext,
    data: SnapshotNodeData<'_>,
    wrapping: Wrapping,
) -> bool {
    let TranslateAncestorContext {
        update,
        parent_content_offset,
        parent_abs,
        ..
    } = ancestors;
    let SnapshotNodeData {
        layout_state,
        modifier_slices,
        children: fresh_children,
        window_root,
    } = data;
    let layout_state = if window_root {
        layout_state.at_origin()
    } else {
        layout_state
    };
    let container_plan = match translated_container(
        container,
        &layout_state,
        &modifier_slices,
        ancestors,
        wrapping,
    ) {
        Ok(plan) => plan,
        Err(reason) => return translate_bail(reason),
    };
    let TranslatedContainer {
        node_id,
        node_bounds,
    } = container_plan;
    container.node_bounds = node_bounds;
    // The container's own draws and text are recorded again around the
    // children it keeps: an unchanged draw reuses its recording, while
    // building the container anew lowered every child layer beneath it too.
    crate::layer_recycling::recycle_primitives(container);
    let translated = with_translate_scratch(|scratch| {
        let order = translated_children(applier, container, dirty_nodes, fresh_children, scratch)?;
        let geometry = TranslateGeometry::new(
            &layout_state,
            &container.graphics_layer,
            container.local_bounds,
            parent_abs,
        );
        let children = geometry.children_context(container, ancestors);
        match &order {
            ChildOrder::Shifted { kept, .. } => {
                recycle_children_around(container, kept.clone(), dirty_nodes);
            }
            ChildOrder::Mixed => recycle_leaving_children(container, scratch, dirty_nodes),
        }
        build_entering_children(applier, scratch, dirty_nodes, children);

        apply_translated_container_state(
            container,
            &modifier_slices,
            &layout_state,
            parent_content_offset,
            geometry,
        );

        reconcile_translated_children(applier, container, dirty_nodes, scratch, order, children);
        Ok(())
    });
    if let Err(reason) = translated {
        return translate_bail(reason);
    }
    write_own_content_around_children(container, node_id, &modifier_slices, layout_state.size());
    container.hit_test = hit_test_from_slices(&modifier_slices);

    container.has_origin_sinks = modifier_slices_have_origin_sinks(&modifier_slices)
        || children_have_origin_sinks(&container.children);
    container.refresh_child_facts();
    container.note_content_change(update);

    true
}
