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
    DrawTarget, GraphChanges, GroupPaint, Inherited, KeptChild, LowerContext, OwnDraws,
    PlacedGeometry, SnapshotNodeData, TranslateAncestorContext, TranslateGeometry, Wrapping,
    child_identity, content_layer, forget_dirty_node, hit_test_from_slices, layer_and_node_bounds,
    layer_identity, lower_child, modifier_slices_have_origin_sinks, move_kept_child,
    outer_commands, outer_draws, placed_child_geometry, placing_transform, read_node_data,
    set_bounds, shaped_graphics_layer, take_wrapped_layer, text_node, translate_unwrapped,
    write_node_content, write_wrapper,
};
use crate::{
    NodeUpdate,
    graph::graphics_layers_read_alike,
    scene_store::{Item, LayerRef, RecordKind, SceneStore, TranslatedWrite},
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

// Whether `layer` or a record beneath it handles pointers, or it is a
// space.
pub(super) fn holds_hit_geometry(layer: LayerRef<'_>) -> bool {
    layer.has_hit_targets() || layer.graphics_layer().space.is_some()
}

fn try_translate_scrolled_layer(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    container: RecordId,
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    ancestors: TranslateAncestorContext,
) -> bool {
    let Some(node_id) = layer_identity(store.layer(container)) else {
        return false;
    };
    read_node_data(applier, node_id, |data| {
        if store.layer(container).kind() != RecordKind::Wrapper {
            return translate_unwrapped(applier, store, container, dirty_nodes, ancestors, data);
        }
        let slices = Rc::clone(&data.modifier_slices);
        let Some(commands) = outer_commands(node_id, &slices) else {
            return false;
        };
        let size = data.layout_state.size();
        let placement = data.layout_state.position();
        let Some(inner) = content_layer(store, container) else {
            return false;
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
            paint: None,
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
            ancestors.inherited.parent_content_offset,
        );
        store.note_content_change(container);
        true
    })
    .unwrap_or(false)
}

pub(super) struct TranslatedContainer {
    pub(super) node_id: NodeId,
}

pub(super) fn translated_container(
    store: &mut SceneStore,
    container: RecordId,
    layout_state: &cranpose_ui::widgets::LayoutState,
    modifier_slices: &ModifierNodeSlices,
    inherited: Inherited,
    wrapping: Wrapping,
) -> Option<TranslatedContainer> {
    if cranpose_core::DIAGNOSTICS.disable_scroll_translate.is_set() {
        return None;
    }
    let layer = store.layer(container);
    let node_id = layer.node_id()?;
    if !layout_state.is_placed() {
        return None;
    }
    let outer_count = modifier_slices.outer_draw_command_count();
    if (outer_count > 0 && wrapping == Wrapping::Bare)
        || (inherited.motion_context_animated || modifier_slices.motion_context_animated())
            != layer.motion_context_animated()
        || modifier_slices.translated_content_context() != layer.translated_content_context()
    {
        return None;
    }
    if modifier_slices.clip_to_bounds() != layer.clip_to_bounds() {
        return None;
    }
    // A resized container keeps its children: only its bounds and the clip
    // shaped to them follow its size. Any other change to its graphics layer
    // may change what its children inherit.
    let current = shaped_graphics_layer(modifier_slices, layer.local_bounds());
    if !graphics_layers_read_alike(store.graphics_layer(container), current.as_deref()) {
        return None;
    }
    let (local_bounds, node_bounds) = layer_and_node_bounds(modifier_slices, layout_state.size());
    let resized = local_bounds != layer.local_bounds();
    let mut changed = false;
    if resized {
        let shaped = shaped_graphics_layer(modifier_slices, local_bounds).map(Cow::into_owned);
        changed = store.write_graphics_layer(container, shaped);
    }
    changed |= set_bounds(store, container, local_bounds, node_bounds);
    if changed {
        store.note_property_change(container);
    }
    // A resize is more than the move of the content a translate step notes
    // alone.
    if resized {
        store.note_content_change(container);
    }
    Some(TranslatedContainer { node_id })
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
    children: &[Item],
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    fresh_children: &[NodeId],
    scratch: &mut TranslateScratch,
) -> Option<ChildOrder> {
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
            let id = child_identity(store, child)?;
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
    check_retained_children(store, children, dirty_nodes, placed_fresh).map(|()| order)
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
    children: &[Item],
    dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    placed_fresh: &mut [PlacedChild],
) -> Option<()> {
    for placed in placed_fresh {
        let Some(old_index) = placed.old_index else {
            continue;
        };
        let child = children[old_index].as_child()?;
        let layer = store.layer(child);
        let text_alone = || match layer.kind() {
            RecordKind::Leaf => text_node::text_alone(store.items(child)),
            RecordKind::Layer | RecordKind::Wrapper | RecordKind::DrawGroup => {
                text_node::draws_text_alone(store, child)
            }
        };
        let fits = Rect::from_size(placed.geometry.size) == layer.node_rect();
        placed.kept = match dirty_nodes.get_mut(&placed.id) {
            None if !fits => return None,
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
            Some(NodeUpdate::Content) if text_alone() => KeptChild::Retexted,
            Some(_) => KeptChild::Left,
        };
    }
    Some(())
}

// The items of a container a translate step parts: the children its own
// draws surround, those own draws, and the children that leave. The
// container holds them all until the step writes its new items.
pub(super) struct ItemParts {
    // The container's items before the step, then the children that leave.
    items: Vec<Item>,
    // Where the children lie among the items before the step.
    children: Range<usize>,
    // How many items the container held before the step.
    held: usize,
}

impl ItemParts {
    // Parts the items of `container`; a draw group counts as an own draw.
    pub(super) fn of(store: &mut SceneStore, container: RecordId) -> Self {
        let mut items = store.lists.take();
        items.extend_from_slice(store.items(container));
        let is_child = |item: &Item| {
            item.as_child()
                .is_some_and(|child| store.layer(child).kind() != RecordKind::DrawGroup)
        };
        let first = items.iter().position(is_child).unwrap_or(items.len());
        let end = items
            .iter()
            .rposition(is_child)
            .map_or(first, |last| last + 1);
        Self {
            held: items.len(),
            items,
            children: first..end,
        }
    }

    pub(super) fn children(&self) -> &[Item] {
        &self.items[self.children.clone()]
    }

    // The container's own items in draw order, for the write of its
    // node's draws: those before its children, then those after them.
    pub(super) fn own_draws(&self) -> OwnDraws<'_> {
        OwnDraws::of(
            &self.items[..self.children.start],
            &self.items[self.children.end..self.held],
        )
    }

    // Keeps the children in `kept`, in order, in `list`; the others leave.
    fn keep_range(
        &mut self,
        store: &SceneStore,
        kept: Range<usize>,
        list: &mut Vec<Item>,
        dirty_nodes: &mut HashMap<NodeId, NodeUpdate>,
    ) {
        for (index, position) in self.children.clone().enumerate() {
            let child = self.items[position];
            if kept.contains(&index) {
                list.push(child);
            } else {
                forget_dirty_node(store, child, dirty_nodes);
                self.items.push(child);
            }
        }
    }

    // Keeps the children the placed children of `scratch` take again; the
    // others leave.
    fn keep_placed(
        &mut self,
        store: &SceneStore,
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
        retained.extend(self.children().iter().copied().map(Some));
        kept.clear();
        kept.extend(
            placed_fresh
                .iter()
                .map(|placed| placed.old_index.and_then(|index| retained[index].take())),
        );
        for leaving in retained.drain(..).flatten() {
            forget_dirty_node(store, leaving, dirty_nodes);
            self.items.push(leaving);
        }
    }

    // Removes the children that left once the container holds its new
    // items: a record made since takes no row a child the container held
    // until then.
    pub(super) fn remove_gone(self, store: &mut SceneStore) {
        for &item in &self.items[self.held..] {
            store.remove_item(item);
        }
        self.give_back(store);
    }

    pub(super) fn give_back(self, store: &mut SceneStore) {
        store.lists.give(self.items);
    }
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
        if let Some(record) = lowered.as_child() {
            store.note_content_change(record);
        }
        forget_dirty_node(store, lowered, dirty_nodes);
        entering.push((placed.id, lowered));
    }
}

pub(super) fn write_own_content_around_children(
    store: &mut SceneStore,
    container: RecordId,
    (list, own): (&mut Vec<Item>, &mut OwnDraws<'_>),
    modifier_slices: &ModifierNodeSlices,
    size: Size,
) {
    if modifier_slices.draw_commands()[modifier_slices.outer_draw_command_count()..].is_empty()
        && !modifier_slices.has_text()
    {
        return;
    }
    let kept = list.len();
    let draws = DrawTarget {
        size,
        motion_context_animated: store.layer(container).motion_context_animated(),
        paint: Some(GroupPaint::Record(container)),
    };
    write_node_content(
        store,
        container,
        (list, own),
        modifier_slices,
        draws,
        (kept, 0),
        |_, _| {},
    );
}

pub(super) fn apply_translated_container_state(
    store: &mut SceneStore,
    container: RecordId,
    modifier_slices: &ModifierNodeSlices,
    layout_state: &cranpose_ui::widgets::LayoutState,
    parent_content_offset: Point,
    geometry: TranslateGeometry,
) {
    let position = layout_state.position();
    let transform = placing_transform(
        store.layer(container).local_bounds(),
        position,
        store.graphics_layer(container),
        parent_content_offset,
    );
    store.set_placement(container, position, transform);
    modifier_slices.publish_window_geometry(
        geometry.top_left,
        geometry.window_transform,
        layout_state.size(),
    );
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
        inherited,
        parent_abs,
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
    if translated_container(
        store,
        container,
        &layout_state,
        &modifier_slices,
        inherited,
        wrapping,
    )
    .is_none()
    {
        return false;
    }
    // The container's own draws and text are recorded again around the
    // children it keeps: an unchanged draw reuses its recording, while
    // building the container anew lowered every child layer beneath it too.
    let mut parts = ItemParts::of(store, container);
    let mut list = store.lists.take();
    let translated = with_translate_scratch(|scratch| {
        let order = translated_children(
            applier,
            store,
            parts.children(),
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
                parts.keep_range(store, kept.clone(), &mut list, dirty_nodes);
            }
            ChildOrder::Mixed => parts.keep_placed(store, scratch, dirty_nodes),
        }
        // The children are placed against the container's new content offset.
        store.set_content_offset(container, geometry.content_offset);
        build_entering_children(applier, store, container, scratch, dirty_nodes, children);
        apply_translated_container_state(
            store,
            container,
            &modifier_slices,
            &layout_state,
            inherited.parent_content_offset,
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
        Some(())
    });
    if translated.is_none() {
        store.lists.give(list);
        parts.give_back(store);
        return false;
    }
    let mut own = parts.own_draws();
    finish_translated_container(
        store,
        container,
        (list, &mut own),
        &modifier_slices,
        layout_state.size(),
    );
    own.finish(store);
    parts.remove_gone(store);
    true
}

// Writes the items of `container` a translate step kept and built in
// `list`, around its own draws and text, and brings its handlers and facts
// up to date.
fn finish_translated_container(
    store: &mut SceneStore,
    container: RecordId,
    (mut list, own): (Vec<Item>, &mut OwnDraws<'_>),
    modifier_slices: &Rc<ModifierNodeSlices>,
    size: Size,
) {
    write_own_content_around_children(store, container, (&mut list, own), modifier_slices, size);
    let write = translated_write(store, container, modifier_slices, Some(&list));
    store.write_translated(container, write);
    store.lists.give(list);
}

// What a translate step writes into `container`, a layer of a node with
// `slices` placed against the content offset the store holds, past its
// children: `items` when the step wrote them.
pub(super) fn translated_write<'a>(
    store: &SceneStore,
    container: RecordId,
    slices: &Rc<ModifierNodeSlices>,
    items: Option<&'a [Item]>,
) -> TranslatedWrite<'a> {
    let translated = store.layer(container).translated_content_context();
    TranslatedWrite {
        translated_content_offset: translated.then(|| {
            slices
                .translated_content_offset()
                .unwrap_or_else(|| store.content_offset(container))
        }),
        items,
        hit_test: hit_test_from_slices(slices),
        origin_sinks: modifier_slices_have_origin_sinks(slices),
    }
}
