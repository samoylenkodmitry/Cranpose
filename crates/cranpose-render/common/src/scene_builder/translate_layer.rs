// How a scene update brings a retained layer to its node's new layout in
// place: the layer keeps the child records that stay, moves them, and
// builds only the children that enter.

use std::{
    borrow::Cow,
    ops::{Range, RangeInclusive},
    rc::Rc,
};

use cranpose_core::{MemoryApplier, NodeId, RecordId, collections::map::HashMap};
use cranpose_ui::{ModifierNodeSlices, Point, Rect, Size};

use super::{
    DrawTarget, GraphChanges, KeptChild, LowerContext, PlacedGeometry, SnapshotNodeData,
    TranslateAncestorContext, TranslateGeometry, Wrapping, child_identity, content_layer,
    forget_dirty_node, hit_test_from_slices, item_lists, layer_and_node_bounds, layer_identity,
    lower_child, move_kept_child, outer_commands, outer_draws, placed_child_geometry,
    placed_transform, read_node_data, refresh_hit_targets, set_bounds, set_origin_sinks,
    shaped_graphics_layer, take_wrapped_layer, text_node, translate_bail, translate_unwrapped,
    write_node_content, write_wrapper,
};
use crate::{
    NodeUpdate,
    graph::LayerProperties,
    scene_store::{Item, LayerRef, RecordKind, SceneStore},
};

pub(super) fn translate_tracking_hits(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    layer: RecordId,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    ancestors: TranslateAncestorContext,
) -> Option<GraphChanges> {
    let held = holds_hit_geometry(store.layer(layer));
    try_translate_scrolled_layer(applier, store, layer, dirty_nodes, ancestors)
        .then(|| GraphChanges::hits(held || holds_hit_geometry(store.layer(layer))))
}

pub(super) fn holds_hit_geometry(layer: LayerRef<'_>) -> bool {
    layer.hit_test().is_some() || layer.has_hit_targets() || layer.graphics_layer().space.is_some()
}

fn try_translate_scrolled_layer(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    container: RecordId,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    ancestors: TranslateAncestorContext,
) -> bool {
    let Some(node_id) = layer_identity(store.layer(container)) else {
        return translate_bail("no node id");
    };
    read_node_data(applier, node_id, |data| {
        if store.layer(container).kind() != RecordKind::Wrapper {
            return translate_unwrapped(applier, store, container, dirty_nodes, ancestors, data);
        }
        let slices = Rc::clone(&data.modifier_slices);
        let Some(commands) = outer_commands(node_id, &slices) else {
            return translate_bail("outer draws removed");
        };
        let size = data.layout_state.size();
        let placement = data.layout_state.position();
        let Some(inner) = content_layer(store, container) else {
            return translate_bail("wrapped layer missing");
        };
        if !translate_layer_from_data(
            applier,
            store,
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
            motion_context_animated: store.layer(inner).motion_context_animated(),
            layer: None,
        };
        let layer = take_wrapped_layer(store, container, node_id)
            .expect("the wrapped layer was found above");
        let outer = outer_draws(store, container, commands, draws);
        write_wrapper(
            store,
            container,
            layer,
            placement,
            outer,
            ancestors.parent_content_offset,
        );
        store.note_content_change(container);
        true
    })
    .unwrap_or_else(|| translate_bail("container snapshot read failed"))
}

pub(super) struct TranslatedContainer {
    pub(super) node_id: NodeId,
}

pub(super) fn translated_container(
    store: &mut SceneStore,
    container: RecordId,
    layout_state: &cranpose_ui::widgets::LayoutState,
    modifier_slices: &ModifierNodeSlices,
    ancestors: TranslateAncestorContext,
    wrapping: Wrapping,
) -> Result<TranslatedContainer, &'static str> {
    if cranpose_core::DIAGNOSTICS.disable_scroll_translate.is_set() {
        return Err("fast path disabled by ablation switch");
    }
    let layer = store.layer(container);
    let Some(node_id) = layer.node_id() else {
        return Err("no node id");
    };
    if !layout_state.is_placed() {
        return Err("container unplaced");
    }
    let outer_count = modifier_slices.outer_draw_command_count();
    if (outer_count > 0 && wrapping == Wrapping::Bare)
        || (ancestors.inherited_motion_context_animated
            || modifier_slices.motion_context_animated())
            != layer.motion_context_animated()
        || modifier_slices.translated_content_context() != layer.translated_content_context()
    {
        return Err("container outer draws or translated context changed");
    }
    if modifier_slices.clip_to_bounds() != layer.clip_to_bounds() {
        return Err("container clip changed");
    }
    // A resized container keeps its children: only its bounds and the clip
    // shaped to them follow its size. Any other change to its graphics layer
    // may change what its children inherit.
    let current = shaped_graphics_layer(modifier_slices, layer.local_bounds());
    let kept_layer = match layer.props() {
        Some(props) => props.graphics_layer.reads_as(current.as_deref()),
        None => LayerProperties::default().reads_as(current.as_deref()),
    };
    if !kept_layer {
        return Err("container graphics layer changed");
    }
    let (local_bounds, node_bounds) = layer_and_node_bounds(modifier_slices, layout_state.size());
    if local_bounds != layer.local_bounds() {
        let resized = shaped_graphics_layer(modifier_slices, local_bounds).map(Cow::into_owned);
        store.edit_props(container, |props| props.graphics_layer.replace(resized));
    }
    set_bounds(store, container, local_bounds, node_bounds);
    Ok(TranslatedContainer { node_id })
}

#[derive(Default)]
pub(crate) struct TranslateScratch {
    placed_fresh: Vec<PlacedChild>,
    old_ids: Vec<NodeId>,
    old_index_by_id: HashMap<NodeId, usize>,
    retained: Vec<Option<Item>>,
    kept: Vec<Option<Item>>,
    entering: Vec<(NodeId, Item)>,
}

pub(super) struct PlacedChild {
    pub(super) id: NodeId,
    pub(super) geometry: PlacedGeometry,
    old_index: Option<usize>,
    pub(super) kept: KeptChild,
}

enum ChildOrder {
    Shifted {
        kept: Range<usize>,
        entering_before: usize,
    },
    Mixed,
}

fn translated_children(
    applier: &MemoryApplier,
    store: &SceneStore,
    container: RecordId,
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
    let children = store.items(container);
    let children_unchanged = children.len() == placed_fresh.len()
        && children
            .iter()
            .zip(placed_fresh.iter())
            .all(|(&child, placed)| child_identity(store, child) == Some(placed.id));
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
        for &child in children {
            let Some(id) = child_identity(store, child) else {
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
    check_retained_children(store, container, dirty_nodes, placed_fresh).map(|()| order)
}

const MATCH_SEARCHES: usize = 16;

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

fn check_retained_children(
    store: &SceneStore,
    container: RecordId,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    placed_fresh: &mut [PlacedChild],
) -> Result<(), &'static str> {
    let children = store.items(container);
    for placed in placed_fresh {
        let Some(old_index) = placed.old_index else {
            continue;
        };
        let Some(child) = children[old_index].as_child() else {
            return Err("retained child slot is not a node");
        };
        let layer = store.layer(child);
        let text_alone = match layer.kind() {
            RecordKind::Leaf => text_node::text_alone(store.items(child)),
            RecordKind::Layer | RecordKind::Wrapper | RecordKind::DrawGroup => {
                text_node::draws_text_alone(store, child)
            }
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
            Some(NodeUpdate::Content) if text_alone => KeptChild::Retexted,
            Some(_) => KeptChild::Left,
        };
    }
    Ok(())
}

fn remove_leaving_children(
    store: &mut SceneStore,
    container: RecordId,
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
    for &child in store.items(container) {
        retained.push(Some(child));
    }
    store.set_items(container, &[]);
    kept.clear();
    kept.extend(
        placed_fresh
            .iter()
            .map(|placed| placed.old_index.and_then(|index| retained[index].take())),
    );
    for leaving in retained.drain(..).flatten() {
        forget_dirty_node(store, leaving, dirty_nodes);
        store.remove_item(leaving);
    }
}

fn remove_children_around(
    store: &mut SceneStore,
    container: RecordId,
    kept: Range<usize>,
    list: &mut Vec<Item>,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
) {
    let mut leaving = item_lists::take();
    for (index, &child) in store.items(container).iter().enumerate() {
        if kept.contains(&index) {
            list.push(child);
        } else {
            leaving.push(child);
        }
    }
    store.set_items(container, &[]);
    for &child in &leaving {
        forget_dirty_node(store, child, dirty_nodes);
        store.remove_item(child);
    }
    item_lists::give(leaving);
}

fn build_entering_children(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    container: RecordId,
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
        let Some(lowered) = lower_child(applier, store, container, placed.id, context) else {
            continue;
        };
        forget_dirty_node(store, lowered, dirty_nodes);
        entering.push((placed.id, lowered));
    }
}

pub(super) fn write_own_content_around_children(
    store: &mut SceneStore,
    container: RecordId,
    list: &mut Vec<Item>,
    node_id: NodeId,
    modifier_slices: &ModifierNodeSlices,
    size: Size,
) {
    if modifier_slices.draw_commands()[modifier_slices.outer_draw_command_count()..].is_empty()
        && !modifier_slices.has_text()
    {
        return;
    }
    let kept = list.len();
    let motion_context_animated = store.layer(container).motion_context_animated();
    // The draws take the container's paint while they are written beneath it.
    let graphics_layer = store
        .props_mut(container)
        .map(|props| std::mem::take(&mut props.graphics_layer))
        .unwrap_or_default();
    let draws = DrawTarget {
        size,
        motion_context_animated,
        layer: graphics_layer.get(),
    };
    write_node_content(
        store,
        container,
        list,
        node_id,
        modifier_slices,
        draws,
        (kept, 0),
        |_, _| {},
    );
    if let Some(props) = store.props_mut(container) {
        props.graphics_layer = graphics_layer;
    }
}

pub(super) fn remove_own_draws(store: &mut SceneStore, container: RecordId) {
    let is_child = |item: &Item| {
        item.as_child()
            .is_some_and(|child| store.layer(child).kind() != RecordKind::DrawGroup)
    };
    let items = store.items(container);
    let first = items.iter().position(is_child).unwrap_or(items.len());
    let end = items
        .iter()
        .rposition(is_child)
        .map_or(first, |last| last + 1);
    // A container that draws nothing of its own has nothing to remove.
    if first == 0 && end == items.len() {
        return;
    }
    let mut list = item_lists::take();
    list.extend_from_slice(items);
    store.set_items(container, &list[first..end]);
    for &own in list[..first].iter().chain(&list[end..]) {
        store.remove_item(own);
    }
    item_lists::give(list);
}

pub(super) fn apply_translated_container_state(
    store: &mut SceneStore,
    container: RecordId,
    modifier_slices: &ModifierNodeSlices,
    layout_state: &cranpose_ui::widgets::LayoutState,
    parent_content_offset: Point,
    geometry: TranslateGeometry,
) {
    let layer = store.layer(container);
    let position = layout_state.position();
    let transform = placed_transform(
        layer.local_bounds(),
        position,
        layer.props().and_then(|props| props.graphics_layer.get()),
        parent_content_offset,
    );
    let translated = layer.translated_content_context();
    store.set_content_offset(container, geometry.content_offset);
    store.set_placement(container, position, transform);
    if translated {
        let offset = modifier_slices
            .translated_content_offset()
            .unwrap_or(geometry.content_offset);
        store.edit_props(container, |props| props.translated_content_offset = offset);
    }
    modifier_slices.publish_window_geometry(
        geometry.top_left,
        geometry.window_transform,
        layout_state.size(),
    );
    store.note_place_change(container);
}

fn reconcile_translated_children(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    list: &mut Vec<Item>,
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
            store,
            list,
            dirty_nodes,
            scratch,
            entering_before,
            children,
        );
        return;
    }
    let mut entering = scratch.entering.drain(..).peekable();
    for (placed, kept) in scratch.placed_fresh.iter().zip(scratch.kept.drain(..)) {
        if let Some(child) = kept {
            if let Some(record) = child.as_child() {
                move_kept_child(applier, store, record, placed, children, dirty_nodes);
            }
            list.push(child);
        } else if let Some((_, lowered)) = entering.next_if(|(id, _)| *id == placed.id) {
            list.push(lowered);
        }
    }
}

fn reconcile_shifted_children(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    list: &mut Vec<Item>,
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
    for (&child, placed) in list.iter().zip(&placed_fresh[entering_before..]) {
        if let Some(record) = child.as_child() {
            move_kept_child(applier, store, record, placed, children, dirty_nodes);
        }
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
    if built_before > 0 {
        list.splice(0..0, entering.drain(..built_before).map(|(_, item)| item));
    }
    list.extend(entering.drain(..).map(|(_, item)| item));
}

fn with_translate_scratch<R>(f: impl FnOnce(&mut TranslateScratch) -> R) -> R {
    crate::thread_state::with(|state| match state.translate_scratch.try_borrow_mut() {
        Ok(mut scratch) => f(&mut scratch),
        Err(_) => f(&mut TranslateScratch::default()),
    })
}

pub(super) fn translate_layer_from_data(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    container: RecordId,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    ancestors: TranslateAncestorContext,
    data: SnapshotNodeData<'_>,
    wrapping: Wrapping,
) -> bool {
    let TranslateAncestorContext {
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
    let node_id = match translated_container(
        store,
        container,
        &layout_state,
        &modifier_slices,
        ancestors,
        wrapping,
    ) {
        Ok(plan) => plan.node_id,
        Err(reason) => return translate_bail(reason),
    };
    // The container's own draws and text are recorded again around the
    // children it keeps: an unchanged draw reuses its recording, while
    // building the container anew lowered every child layer beneath it too.
    remove_own_draws(store, container);
    let mut list = item_lists::take();
    let translated = with_translate_scratch(|scratch| {
        let order = translated_children(
            applier,
            store,
            container,
            dirty_nodes,
            fresh_children,
            scratch,
        )?;
        let layer = store.layer(container);
        let geometry = TranslateGeometry::new(
            &layout_state,
            layer.graphics_layer(),
            layer.local_bounds(),
            parent_abs,
        );
        let children = geometry.children_context(layer, ancestors);
        match &order {
            ChildOrder::Shifted { kept, .. } => {
                remove_children_around(store, container, kept.clone(), &mut list, dirty_nodes);
            }
            ChildOrder::Mixed => remove_leaving_children(store, container, scratch, dirty_nodes),
        }
        // The children are placed against the container's new content offset.
        store.set_content_offset(container, geometry.content_offset);
        build_entering_children(applier, store, container, scratch, dirty_nodes, children);
        apply_translated_container_state(
            store,
            container,
            &modifier_slices,
            &layout_state,
            parent_content_offset,
            geometry,
        );
        reconcile_translated_children(
            applier,
            store,
            &mut list,
            dirty_nodes,
            scratch,
            order,
            children,
        );
        Ok(())
    });
    if let Err(reason) = translated {
        item_lists::give(list);
        return translate_bail(reason);
    }
    finish_translated_container(
        store,
        container,
        list,
        node_id,
        &modifier_slices,
        layout_state.size(),
    );
    true
}

// Writes the items of `container` a translate step kept and built in
// `list`, around its own draws and text, and brings its handlers and facts
// up to date.
fn finish_translated_container(
    store: &mut SceneStore,
    container: RecordId,
    mut list: Vec<Item>,
    node_id: NodeId,
    modifier_slices: &Rc<ModifierNodeSlices>,
    size: Size,
) {
    write_own_content_around_children(store, container, &mut list, node_id, modifier_slices, size);
    store.set_items(container, &list);
    item_lists::give(list);
    let hit_test = hit_test_from_slices(modifier_slices);
    store.edit_props(container, |props| props.hit_test = hit_test);
    set_origin_sinks(store, container, modifier_slices);
    refresh_hit_targets(store, container);
    store.note_content_change(container);
}
