// How a layout node lowers into the layer that draws it: its layer
// properties, its own draws and text around its child layers, and the
// wrapper of the draws chained before its graphics layer.

use std::{borrow::Cow, rc::Rc};

use cranpose_core::{MemoryApplier, NodeId, RecordId};
use cranpose_ui::{LayoutBox, ModifierNodeSlices, Point, Rect, Size, widgets::LayoutState};
use cranpose_ui_graphics::{CompositingStrategy, DrawPlacement, GraphicsLayer, LayerShape};

use super::{
    BuildNodeSnapshot, DrawPass, DrawTarget, Level, NodeCommands, SnapshotNodeData, TextNodeParts,
    TranslateAncestorContext, TranslateGeometry, append_draw_nodes, draw_nodes,
    inline_leaf::{lower_child, snapshot_lowers_inline},
    item_lists, layer_node_capacity,
    layer_shape::{clip_shape, shape_clip},
    note_space_change, read_placed_node_data, shaped_graphics_layer, text_node_from_parts,
};
use crate::{
    graph::{CachePolicy, HitTestNode, IsolationReasons, PrimitivePhase, ProjectiveTransform},
    layer_transform::{
        layer_scales_or_rotates, layer_transform_to_parent, layer_transform_to_window,
    },
    scene_store::{Item, ItemKind, LayerRef, RecordFlags, RecordKind, RecordSeed, SceneStore},
};

#[derive(Clone, Copy)]
pub(super) struct LowerContext {
    pub(super) inherited_motion_context_animated: bool,
    pub(super) inherited_translated_content_context: bool,
    pub(super) parent_abs: Option<AbsOrigin>,
    pub(super) parent_content_offset: Point,
}

impl LowerContext {
    pub(super) const ROOT: Self = Self {
        inherited_motion_context_animated: false,
        inherited_translated_content_context: false,
        parent_abs: Some(AbsOrigin::ROOT),
        parent_content_offset: Point { x: 0.0, y: 0.0 },
    };

    // What a child lowers with beneath `ancestors`.
    pub(super) fn beneath(ancestors: TranslateAncestorContext) -> Self {
        Self {
            inherited_motion_context_animated: ancestors.inherited_motion_context_animated,
            inherited_translated_content_context: ancestors.inherited_translated_content_context,
            parent_abs: Some(ancestors.parent_abs),
            parent_content_offset: ancestors.parent_content_offset,
        }
    }

    fn for_children(
        self,
        slices: &ModifierNodeSlices,
        content_offset: Point,
        parent_abs: Option<AbsOrigin>,
    ) -> Self {
        Self {
            inherited_motion_context_animated: self.inherited_motion_context_animated
                || slices.motion_context_animated(),
            inherited_translated_content_context: self.inherited_translated_content_context
                || slices.translated_content_context(),
            parent_abs,
            parent_content_offset: content_offset,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum HeadKind {
    Node(NodeId),
    Wrapper(NodeId),
    Group { node: NodeId, phase: PrimitivePhase },
}

impl HeadKind {
    fn record(self) -> (RecordKind, NodeId) {
        match self {
            Self::Node(node) => (RecordKind::Layer, node),
            Self::Wrapper(node) => (RecordKind::Wrapper, node),
            Self::Group { node, .. } => (RecordKind::DrawGroup, node),
        }
    }
}

pub(super) struct LayerHead {
    pub(super) kind: HeadKind,
    pub(super) local_bounds: Rect,
    pub(super) node_bounds: Option<Rect>,
    pub(super) transform_to_parent: ProjectiveTransform,
    pub(super) content_offset: Point,
    pub(super) motion_context_animated: bool,
    pub(super) translated_content_context: bool,
    pub(super) translated_content_offset: Point,
    pub(super) origin_in_parent: Point,
    pub(super) graphics_layer: Option<GraphicsLayer>,
    pub(super) clip_to_bounds: bool,
    pub(super) hit_test: Option<HitTestNode>,
    pub(super) has_origin_sinks: bool,
    pub(super) isolation: IsolationReasons,
    pub(super) cache_policy: CachePolicy,
}

struct NodeFrame {
    local_bounds: Rect,
    node_bounds: Option<Rect>,
    placement: Point,
    content_offset: Point,
    translated_content_offset: Point,
}

fn node_layer_head(
    node_id: NodeId,
    slices: &Rc<ModifierNodeSlices>,
    frame: NodeFrame,
    graphics_layer: Option<GraphicsLayer>,
    context: LowerContext,
) -> LayerHead {
    let clip_to_bounds = slices.clip_to_bounds();
    let translated_content_context = slices.translated_content_context();
    let properties = graphics_layer.as_ref().unwrap_or(&GraphicsLayer::DEFAULT);
    let isolation = isolation_reasons(properties);
    LayerHead {
        kind: HeadKind::Node(node_id),
        local_bounds: frame.local_bounds,
        node_bounds: frame.node_bounds,
        transform_to_parent: placed_transform(
            frame.local_bounds,
            frame.placement,
            graphics_layer.as_ref(),
            context.parent_content_offset,
        ),
        content_offset: frame.content_offset,
        motion_context_animated: context.inherited_motion_context_animated
            || slices.motion_context_animated(),
        translated_content_context,
        translated_content_offset: if translated_content_context {
            frame.translated_content_offset
        } else {
            Point::default()
        },
        origin_in_parent: frame.placement,
        cache_policy: layer_cache_policy(properties, isolation),
        isolation,
        graphics_layer,
        clip_to_bounds,
        hit_test: hit_test_from_slices(slices),
        has_origin_sinks: modifier_slices_have_origin_sinks(slices),
    }
}

// Starts writing `record` for `kind`, before its items: the records
// written beneath it are placed against `content_offset`.
fn begin_record(store: &mut SceneStore, record: RecordId, kind: HeadKind, content_offset: Point) {
    let (record_kind, node) = kind.record();
    store.set_kind(record, record_kind, Some(node));
    store.set_content_offset(record, content_offset);
}

fn flag_if(on: bool, flag: RecordFlags) -> RecordFlags {
    if on { flag } else { RecordFlags::default() }
}

pub(super) fn assign_layer(store: &mut SceneStore, record: RecordId, head: LayerHead) {
    let LayerHead {
        kind,
        local_bounds,
        node_bounds,
        transform_to_parent,
        content_offset,
        motion_context_animated,
        translated_content_context,
        translated_content_offset,
        origin_in_parent,
        graphics_layer,
        clip_to_bounds,
        hit_test,
        has_origin_sinks,
        isolation,
        cache_policy,
    } = head;
    let (record_kind, node) = kind.record();
    if store.node(record) != Some(node) {
        // A record that draws for another node starts a new run of changes.
        store.clear_props(record);
    }
    store.set_kind(record, record_kind, Some(node));
    store.set_content_offset(record, content_offset);
    let size = node_bounds.unwrap_or(local_bounds);
    store.set_size(record, size.width, size.height);
    store.set_placement(record, origin_in_parent, transform_to_parent);
    let space = graphics_layer
        .as_ref()
        .is_some_and(|layer| layer.space.is_some());
    store.set_graphics_layer(record, graphics_layer);
    store.edit_props(record, |props| {
        props.layer_bounds = node_bounds.map(|_| local_bounds);
        props.translated_content_offset = translated_content_offset;
        props.hit_test = hit_test;
        props.isolation = isolation;
        props.cache_policy = cache_policy;
    });
    if space {
        note_space_change();
    }
    let after_children = matches!(
        kind,
        HeadKind::Group {
            phase: PrimitivePhase::AfterChildren,
            ..
        }
    );
    let flags = flag_if(clip_to_bounds, RecordFlags::CLIP_TO_BOUNDS)
        | flag_if(motion_context_animated, RecordFlags::MOTION_ANIMATED)
        | flag_if(translated_content_context, RecordFlags::TRANSLATED_CONTENT)
        | flag_if(after_children, RecordFlags::AFTER_CHILDREN)
        | flag_if(
            has_origin_sinks || children_have_origin_sinks(store, record),
            RecordFlags::HAS_ORIGIN_SINKS,
        );
    store.set_flags(record, flags);
    refresh_hit_targets(store, record);
    store.note_property_change(record);
}

pub(super) fn refresh_hit_targets(store: &mut SceneStore, record: RecordId) {
    let layer = store.layer(record);
    let hit_targets = layer.hit_test().is_some() || layer.children().any(LayerRef::has_hit_targets);
    let flags = layer.flags().without(RecordFlags::HAS_HIT_TARGETS)
        | flag_if(hit_targets, RecordFlags::HAS_HIT_TARGETS);
    store.set_flags(record, flags);
}

pub(super) fn set_origin_sinks(
    store: &mut SceneStore,
    record: RecordId,
    slices: &ModifierNodeSlices,
) {
    let sinks =
        modifier_slices_have_origin_sinks(slices) || children_have_origin_sinks(store, record);
    let flags = store
        .layer(record)
        .flags()
        .without(RecordFlags::HAS_ORIGIN_SINKS)
        | flag_if(sinks, RecordFlags::HAS_ORIGIN_SINKS);
    store.set_flags(record, flags);
}

pub(super) fn add_child_origin_sinks(store: &mut SceneStore, record: RecordId) {
    if children_have_origin_sinks(store, record) {
        let flags = store
            .layer(record)
            .flags()
            .with(RecordFlags::HAS_ORIGIN_SINKS);
        store.set_flags(record, flags);
    }
}

fn children_have_origin_sinks(store: &SceneStore, record: RecordId) -> bool {
    store
        .layer(record)
        .children()
        .any(LayerRef::has_origin_sinks)
}

pub(super) fn set_bounds(
    store: &mut SceneStore,
    record: RecordId,
    local_bounds: Rect,
    node_bounds: Option<Rect>,
) {
    let size = node_bounds.unwrap_or(local_bounds);
    store.set_size(record, size.width, size.height);
    store.edit_props(record, |props| {
        props.layer_bounds = node_bounds.map(|_| local_bounds);
    });
}

pub(super) fn placed_transform(
    local_bounds: Rect,
    placement: Point,
    graphics_layer: Option<&GraphicsLayer>,
    parent_content_offset: Point,
) -> ProjectiveTransform {
    // Most layers only move: one translation places them.
    let graphics_layer = match graphics_layer {
        Some(layer) if layer_scales_or_rotates(layer) => layer,
        moved => {
            let offset = moved.map_or_else(Point::default, |layer| Point {
                x: layer.translation_x,
                y: layer.translation_y,
            });
            return ProjectiveTransform::translation(
                placement.x + offset.x + parent_content_offset.x,
                placement.y + offset.y + parent_content_offset.y,
            );
        }
    };
    let transform = layer_transform_to_parent(local_bounds, placement, graphics_layer);
    if parent_content_offset == Point::default() {
        transform
    } else {
        transform.then(ProjectiveTransform::translation(
            parent_content_offset.x,
            parent_content_offset.y,
        ))
    }
}

pub(super) fn write_node_content(
    store: &mut SceneStore,
    record: RecordId,
    list: &mut Vec<Item>,
    slices: &ModifierNodeSlices,
    draws: DrawTarget,
    (kept, written): (usize, usize),
    write_children: impl FnOnce(&mut SceneStore, &mut Vec<Item>),
) {
    let node_id = store
        .node(record)
        .expect("a node's content is written into a record of that node");
    let size = draws.size;
    let outer_count = slices.outer_draw_command_count();
    let commands = NodeCommands {
        node_id,
        commands: &slices.draw_commands()[outer_count..],
        first_index: outer_count,
    };
    list.reserve(layer_node_capacity(
        commands.commands,
        written,
        slices.has_text(),
    ));
    let first_kept = list.len() - kept;
    let first_draw = list.len();
    append_draw_nodes(
        store,
        record,
        list,
        commands,
        DrawPass::Record(DrawPlacement::Behind),
        draws,
        PrimitivePhase::BeforeChildren,
    );
    if let Some(text) = text_node_from_parts(&TextNodeParts::of(node_id, slices, size)) {
        list.push(store.add_text(text));
    }
    // The kept children go after the draws and text just written.
    list[first_kept..].rotate_left(kept);
    let first_draw = first_draw - kept;
    write_children(store, list);
    append_draw_nodes(
        store,
        record,
        list,
        commands,
        DrawPass::OverlayFromOutput(first_draw),
        draws,
        PrimitivePhase::AfterChildren,
    );
}

// Writes a node's layer into `target`, whose items are empty: in `target`
// itself, or, when the node chains draws before its graphics layer, in a
// record under `target`, which becomes the wrapper of those draws.
// `write_content` writes the items of the node's layer.
fn write_layer_with_outer(
    store: &mut SceneStore,
    target: RecordId,
    head: LayerHead,
    outer: Option<(NodeCommands<'_>, DrawTarget<'_>)>,
    parent_content_offset: Point,
    write_content: impl FnOnce(&mut SceneStore, RecordId, &mut Vec<Item>, Option<&GraphicsLayer>),
) {
    let Some((outer, draws)) = outer else {
        begin_record(store, target, head.kind, head.content_offset);
        let mut list = item_lists::take();
        write_content(store, target, &mut list, head.graphics_layer.as_ref());
        store.set_items(target, &list);
        item_lists::give(list);
        assign_layer(store, target, head);
        return;
    };
    let HeadKind::Node(node_id) = head.kind else {
        unreachable!("only a node's layer chains outer draws");
    };
    let placement = head.origin_in_parent;
    begin_record(store, target, HeadKind::Wrapper(node_id), Point::default());
    let outer = outer_draws(store, target, outer, draws);
    let layer = store.insert(RecordSeed {
        kind: RecordKind::Layer,
        node: Some(node_id),
        parent: Some(target),
    });
    begin_record(store, layer, head.kind, head.content_offset);
    let mut list = item_lists::take();
    write_content(store, layer, &mut list, head.graphics_layer.as_ref());
    store.set_items(layer, &list);
    item_lists::give(list);
    assign_layer(store, layer, head);
    write_wrapper(
        store,
        target,
        layer,
        placement,
        outer,
        parent_content_offset,
    );
}

pub(super) fn write_snapshot_layer(
    store: &mut SceneStore,
    snapshot: BuildNodeSnapshot,
    context: LowerContext,
    target: RecordId,
) {
    let BuildNodeSnapshot {
        node_id,
        placement,
        size,
        content_offset,
        slices,
        graphics_layer,
        children,
    } = snapshot;
    let (local_bounds, node_bounds) = layer_and_node_bounds(&slices, size);
    let child_context = context.for_children(&slices, content_offset, None);
    let head = node_layer_head(
        node_id,
        &slices,
        NodeFrame {
            local_bounds,
            node_bounds,
            placement,
            content_offset,
            translated_content_offset: content_offset,
        },
        graphics_layer,
        context,
    );
    let draws = DrawTarget {
        size,
        motion_context_animated: head.motion_context_animated,
        layer: None,
    };
    let outer = outer_commands(node_id, &slices).map(|commands| (commands, draws));
    let parent_content_offset = context.parent_content_offset;
    write_layer_with_outer(
        store,
        target,
        head,
        outer,
        parent_content_offset,
        |store, record, list, layer| {
            let draws = DrawTarget { layer, ..draws };
            let written = (0, children.len());
            write_node_content(
                store,
                record,
                list,
                &slices,
                draws,
                written,
                |store, list| {
                    for child in children {
                        list.push(lower_snapshot_child(store, record, child, child_context));
                    }
                },
            );
        },
    );
}

// Lowers `snapshot`, a child in a layout tree snapshot, into a record under
// `parent`: a leaf when it needs no layer of its own, as
// [`super::inline_leaf::lower_child`] lowers a node of the applier.
fn lower_snapshot_child(
    store: &mut SceneStore,
    parent: RecordId,
    snapshot: BuildNodeSnapshot,
    context: LowerContext,
) -> Item {
    let record = store.insert(RecordSeed {
        kind: RecordKind::Layer,
        node: Some(snapshot.node_id),
        parent: Some(parent),
    });
    if !snapshot_lowers_inline(&snapshot) {
        write_snapshot_layer(store, snapshot, context, record);
        return Item::child(record);
    }
    let BuildNodeSnapshot {
        node_id,
        placement,
        size,
        content_offset,
        slices,
        ..
    } = snapshot;
    let motion_context_animated =
        context.inherited_motion_context_animated || slices.motion_context_animated();
    let draws = DrawTarget {
        size,
        motion_context_animated,
        layer: None,
    };
    store.set_kind(record, RecordKind::Leaf, Some(node_id));
    store.set_content_offset(record, content_offset);
    let mut list = item_lists::take();
    write_node_content(store, record, &mut list, &slices, draws, (0, 0), |_, _| {});
    store.set_items(record, &list);
    item_lists::give(list);
    if store
        .items(record)
        .iter()
        .any(|item| item.kind() == ItemKind::Child)
    {
        let (local_bounds, node_bounds) = layer_and_node_bounds(&slices, size);
        let frame = NodeFrame {
            local_bounds,
            node_bounds,
            placement,
            content_offset,
            translated_content_offset: content_offset,
        };
        assign_layer(
            store,
            record,
            node_layer_head(node_id, &slices, frame, None, context),
        );
        return Item::child(record);
    }
    store.set_size(record, size.width, size.height);
    let offset = context.parent_content_offset;
    store.set_placement(
        record,
        placement,
        ProjectiveTransform::translation(placement.x + offset.x, placement.y + offset.y),
    );
    store.set_flags(
        record,
        flag_if(motion_context_animated, RecordFlags::MOTION_ANIMATED),
    );
    store.note_content_change(record);
    Item::child(record)
}

#[derive(Clone, Copy)]
pub(super) struct AbsOrigin {
    pub(super) content_origin: Point,
    pub(super) window_transform: ProjectiveTransform,
}

impl AbsOrigin {
    pub(super) const ROOT: Self = Self {
        content_origin: Point { x: 0.0, y: 0.0 },
        window_transform: ProjectiveTransform::identity(),
    };

    pub(super) fn children_of(self, layer: LayerRef<'_>) -> Self {
        let origin = layer.origin_in_parent();
        let content_offset = layer.content_offset();
        let top_left = Point {
            x: self.content_origin.x + origin.x,
            y: self.content_origin.y + origin.y,
        };
        Self {
            content_origin: Point {
                x: self.content_origin.x + origin.x + content_offset.x,
                y: self.content_origin.y + origin.y + content_offset.y,
            },
            window_transform: layer_transform_to_window(
                self.window_transform,
                top_left,
                layer.local_bounds(),
                layer.graphics_layer(),
            ),
        }
    }
}

pub(super) fn lower_root_into(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    node_id: NodeId,
) -> bool {
    read_placed_node_data(applier, node_id, Level::Root, |data| {
        let root = root_record(store, node_id);
        write_node_layer(applier, store, node_id, data, LowerContext::ROOT, root);
    })
    .is_some()
}

pub(super) fn root_record(store: &mut SceneStore, node_id: NodeId) -> RecordId {
    match store.root() {
        Some(root) => root,
        None => store.insert(RecordSeed {
            kind: RecordKind::Layer,
            node: Some(node_id),
            parent: None,
        }),
    }
}

pub(super) fn hit_test_from_slices(slices: &Rc<ModifierNodeSlices>) -> Option<HitTestNode> {
    slices_hit_something(slices).then(|| HitTestNode {
        handlers: Rc::clone(slices),
    })
}

pub(super) fn layer_and_node_bounds(
    slices: &ModifierNodeSlices,
    size: Size,
) -> (Rect, Option<Rect>) {
    let node_bounds = Rect::from_size(size);
    let layer_bounds = slices.layer_bounds(size);
    (
        layer_bounds,
        (layer_bounds != node_bounds).then_some(node_bounds),
    )
}

pub(super) fn slices_hit_something(slices: &ModifierNodeSlices) -> bool {
    !slices.pointer_inputs().is_empty() || slices.pointer_icon().is_some()
}

pub(super) fn prepare_node_layer(
    node_id: NodeId,
    slices: &Rc<ModifierNodeSlices>,
    layout_state: &LayoutState,
    context: LowerContext,
) -> (LayerHead, LowerContext) {
    let size = layout_state.size();
    let placement = layout_state.position();
    let (local_bounds, node_bounds) = layer_and_node_bounds(slices, size);
    let graphics_layer = shaped_graphics_layer(slices, local_bounds);
    let geometry = context.parent_abs.map(|parent_abs| {
        TranslateGeometry::new(
            layout_state,
            graphics_layer.as_deref().unwrap_or(&GraphicsLayer::DEFAULT),
            local_bounds,
            parent_abs,
        )
    });
    if let Some(geometry) = geometry {
        slices.publish_window_geometry(geometry.top_left, geometry.window_transform, size);
    } else {
        slices.publish_pointer_input_size(size);
    }
    let content_offset = layout_state.content_offset();
    let child_context = context.for_children(
        slices,
        content_offset,
        geometry.map(TranslateGeometry::child_abs),
    );
    let head = node_layer_head(
        node_id,
        slices,
        NodeFrame {
            local_bounds,
            node_bounds,
            placement,
            content_offset,
            translated_content_offset: slices.translated_content_offset().unwrap_or(content_offset),
        },
        graphics_layer.map(Cow::into_owned),
        context,
    );
    (head, child_context)
}

#[expect(clippy::print_stderr, reason = "CRANPOSE_SCENE_UPDATE_DIAG asks")]
pub(super) fn write_node_layer(
    applier: &MemoryApplier,
    store: &mut SceneStore,
    node_id: NodeId,
    data: SnapshotNodeData<'_>,
    context: LowerContext,
    target: RecordId,
) {
    let SnapshotNodeData {
        layout_state,
        modifier_slices: slices,
        children,
        window_root: _,
    } = data;
    let size = layout_state.size();
    if cranpose_core::DIAGNOSTICS.scene_update_diag.is_set() {
        let placement = layout_state.position();
        eprintln!(
            "[scene-update-diag] build layer node={node_id:?} size=({:.2},{:.2}) pos=({:.2},{:.2})",
            size.width, size.height, placement.x, placement.y,
        );
    }
    let (head, child_context) = prepare_node_layer(node_id, &slices, &layout_state, context);
    let draws = DrawTarget {
        size,
        motion_context_animated: head.motion_context_animated,
        layer: None,
    };
    let outer = outer_commands(node_id, &slices).map(|commands| (commands, draws));
    write_layer_with_outer(
        store,
        target,
        head,
        outer,
        context.parent_content_offset,
        |store, record, list, layer| {
            let draws = DrawTarget { layer, ..draws };
            let written = (0, children.len());
            write_node_content(
                store,
                record,
                list,
                &slices,
                draws,
                written,
                |store, list| {
                    for &child_id in children {
                        if let Some(child) =
                            lower_child(applier, store, record, child_id, child_context)
                        {
                            list.push(child);
                        }
                    }
                },
            );
        },
    );
}

pub(super) struct OuterDraws {
    behind: Vec<Item>,
    overlay: Vec<Item>,
}

pub(super) fn outer_commands(
    node_id: NodeId,
    slices: &ModifierNodeSlices,
) -> Option<NodeCommands<'_>> {
    let count = slices.outer_draw_command_count();
    (count > 0).then(|| NodeCommands {
        node_id,
        commands: &slices.draw_commands()[..count],
        first_index: 0,
    })
}

pub(super) fn outer_draws(
    store: &mut SceneStore,
    wrapper: RecordId,
    commands: NodeCommands<'_>,
    draws: DrawTarget,
) -> OuterDraws {
    let behind = draw_nodes(
        store,
        wrapper,
        commands,
        DrawPlacement::Behind,
        draws,
        PrimitivePhase::BeforeChildren,
    );
    let mut overlay = item_lists::take();
    append_draw_nodes(
        store,
        wrapper,
        &mut overlay,
        commands,
        DrawPass::OverlayFrom(behind.iter()),
        draws,
        PrimitivePhase::AfterChildren,
    );
    OuterDraws { behind, overlay }
}

pub(super) fn write_wrapper(
    store: &mut SceneStore,
    wrapper: RecordId,
    layer: RecordId,
    placement: Point,
    outer: OuterDraws,
    parent_content_offset: Point,
) {
    let inner = store.layer(layer);
    let node_id = inner.node_id().expect("a wrapped layer draws for a node");
    let node_rect = inner.node_rect();
    let motion_context_animated = inner.motion_context_animated();
    let transform = layer_transform_to_parent(
        inner.local_bounds(),
        Point::default(),
        inner.graphics_layer(),
    );
    store.set_content_offset(wrapper, Point::default());
    store.set_placement(layer, Point::default(), transform);
    let OuterDraws {
        mut behind,
        overlay,
    } = outer;
    behind.push(Item::child(layer));
    behind.extend_from_slice(&overlay);
    store.set_items(wrapper, &behind);
    item_lists::give(behind);
    item_lists::give(overlay);
    let head = LayerHead {
        kind: HeadKind::Wrapper(node_id),
        local_bounds: node_rect,
        node_bounds: None,
        transform_to_parent: placed_transform(node_rect, placement, None, parent_content_offset),
        content_offset: Point::default(),
        motion_context_animated,
        translated_content_context: false,
        translated_content_offset: Point::default(),
        origin_in_parent: placement,
        graphics_layer: None,
        clip_to_bounds: false,
        hit_test: None,
        has_origin_sinks: false,
        isolation: IsolationReasons::default(),
        cache_policy: CachePolicy::None,
    };
    assign_layer(store, wrapper, head);
}

pub(super) fn take_wrapped_layer(
    store: &mut SceneStore,
    wrapper: RecordId,
    node_id: NodeId,
) -> Option<RecordId> {
    let mut wrapped = None;
    let mut list = item_lists::take();
    list.extend_from_slice(store.items(wrapper));
    store.set_items(wrapper, &[]);
    for &item in &list {
        match item.as_child() {
            Some(child)
                if wrapped.is_none()
                    && store.layer(child).kind() == RecordKind::Layer
                    && store.layer(child).node_id() == Some(node_id) =>
            {
                wrapped = Some(child);
            }
            _ => store.remove_item(item),
        }
    }
    item_lists::give(list);
    wrapped
}

pub(super) fn layer_identity(layer: LayerRef<'_>) -> Option<NodeId> {
    layer.node_id().or_else(|| layer.wraps())
}

pub(super) fn layout_box_to_snapshot(
    node: &LayoutBox,
    parent: Option<&LayoutBox>,
) -> BuildNodeSnapshot {
    let placement = parent
        .map(|parent_box| Point {
            x: node.rect.x - parent_box.rect.x - parent_box.content_offset.x,
            y: node.rect.y - parent_box.rect.y - parent_box.content_offset.y,
        })
        .unwrap_or_default();
    let mut children = Vec::with_capacity(node.children.len());
    if node
        .children
        .iter()
        .any(|child| child.node_data.z_index != 0.0)
    {
        let mut ordered: Vec<&LayoutBox> = node.children.iter().collect();
        // Stable: equal z indices keep the composition order.
        ordered.sort_by(|a, b| a.node_data.z_index.total_cmp(&b.node_data.z_index));
        children.extend(
            ordered
                .into_iter()
                .map(|child| layout_box_to_snapshot(child, Some(node))),
        );
    } else {
        children.extend(
            node.children
                .iter()
                .map(|child| layout_box_to_snapshot(child, Some(node))),
        );
    }
    let mut graphics_layer = node.node_data.modifier_slices.graphics_layer();
    let has_graphics_layer = graphics_layer.is_some();
    shape_clip(
        &mut graphics_layer,
        clip_shape(&node.node_data.modifier_slices),
        Rect {
            x: 0.0,
            y: 0.0,
            width: node.rect.width,
            height: node.rect.height,
        },
    );
    let has_graphics_layer = has_graphics_layer
        || graphics_layer
            .as_ref()
            .is_some_and(|layer| layer.render_effect.is_some());

    BuildNodeSnapshot {
        node_id: node.node_id,
        placement,
        size: Size {
            width: node.rect.width,
            height: node.rect.height,
        },
        content_offset: node.content_offset,
        slices: Rc::clone(&node.node_data.modifier_slices),
        graphics_layer: graphics_layer
            .filter(|_| has_graphics_layer)
            .map(Cow::into_owned),
        children,
    }
}

pub(super) fn modifier_slices_have_origin_sinks(slices: &ModifierNodeSlices) -> bool {
    slices.text_window().is_some() || slices.viewport_window_rect().is_some()
}

fn layer_cache_policy(layer: &GraphicsLayer, isolation: IsolationReasons) -> CachePolicy {
    if isolation.has_any() || layer_scales_or_rotates(layer) {
        CachePolicy::Auto
    } else {
        CachePolicy::None
    }
}

pub(super) fn isolation_reasons(layer: &GraphicsLayer) -> IsolationReasons {
    IsolationReasons {
        explicit_offscreen: layer.compositing_strategy == CompositingStrategy::Offscreen,
        shape_clip: layer.clip && !matches!(layer.shape, LayerShape::Rectangle),
        effect: layer.render_effect.is_some(),
        backdrop: layer.backdrop_effect.is_some(),
        group_opacity: layer.compositing_strategy != CompositingStrategy::ModulateAlpha
            && layer.alpha < 1.0,
        blend_mode: layer.blend_mode != cranpose_ui::BlendMode::SrcOver,
    }
}
